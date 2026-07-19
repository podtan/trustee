//! Trustee API — REST + WebSocket server for the Trustee agent.
//!
//! Wraps a [`trustee_core::session::Session`] and exposes it over HTTP.
//! Static frontend files are served from [`trustee_web`].
//!
//! Authentication is optional. When `[oidc]` or `[dev]` sections are present
//! in the config TOML, all `/api/v1/*` endpoints require a valid JWT or dev
//! token. Otherwise, all endpoints are open.

pub mod auth;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;

pub use auth::{AuthConfig, AuthState};
pub use state::ServerState;

/// Run the API server.
///
/// Creates a `Session` with the given config, starts a background task to
/// drain workflow messages and broadcast them to WebSocket clients, then
/// serves the REST + WebSocket + static files on `addr`.
///
/// If `[oidc]` or `[dev]` sections are found in the config TOML, auth is
/// enabled — all `/api/v1/*` endpoints (except health) require a valid token.
pub async fn run(
    config_toml: String,
    secrets: std::collections::HashMap<String, String>,
    build_info: trustee_core::types::BuildInfo,
    addr: SocketAddr,
) -> Result<()> {
    // Parse auth config from TOML (returns None if no [oidc] or [dev] sections)
    let auth_state = AuthConfig::from_toml(&config_toml).map(|cfg| {
        let is_dev = cfg.dev_config.local_dev_mode;
        tracing::info!(
            "Auth enabled: {} mode, issuer={}",
            if is_dev { "development" } else { "production" },
            cfg.issuer_url
        );
        Arc::new(AuthState::new(cfg))
    });

    // Build the session
    let (mut session, workflow_rx) = trustee_core::session::Session::new();
    session.config_toml = Some(config_toml);
    session.secrets = Some(secrets);
    session.build_info = Some(build_info);
    session.parse_auto_handoff_config();

    // Create the broadcast channel for WebSocket fan-out
    let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(256);

    // Wrap session in shared state
    let state = ServerState::new(session, ws_tx, auth_state);

    // Start background message drain task (owns workflow_rx directly — no deadlock)
    state.clone().spawn_drain_task(workflow_rx);

    // Build router
    //
    // Auth middleware approach: since axum 0.8's from_fn_with_state has
    // trait bound issues with nested routers, we apply auth checking at
    // the handler level via a helper. Each protected route's handler
    // calls auth::check_auth() first. This is simpler and avoids type
    // complexity.
    let app = axum::Router::new()
        // Public routes
        .route("/api/v1/health", get(routes::health))
        .nest("/auth", auth::auth_routes())
        // Protected API routes
        .route("/api/v1/session", get(routes::get_session))
        .route("/api/v1/session/command", post(routes::post_command))
        .route("/api/v1/session/cancel", post(routes::post_cancel))
        .route("/api/v1/session/handoff", post(routes::post_handoff))
        .route("/api/v1/session/stream", get(routes::ws_handler))
        // Static files from trustee-web
        .route("/", get(routes::serve_index))
        .route("/{file}", get(routes::serve_static))
        .layer(CorsLayer::permissive())
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Trustee API listening on http://{}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

//! Trustee API — REST + WebSocket server for the Trustee agent.
//!
//! Wraps a [`trustee_core::session::Session`] and exposes it over HTTP.
//! Static frontend files are served from [`trustee_web`].

mod routes;
mod state;

use std::net::SocketAddr;

use anyhow::Result;
use axum::routing::{get, post};
use tower_http::cors::CorsLayer;

pub use state::ServerState;

/// Run the API server.
///
/// Creates a `Session` with the given config, starts a background task to
/// drain workflow messages and broadcast them to WebSocket clients, then
/// serves the REST + WebSocket + static files on `addr`.
pub async fn run(
    config_toml: String,
    secrets: std::collections::HashMap<String, String>,
    build_info: trustee_core::types::BuildInfo,
    addr: SocketAddr,
) -> Result<()> {
    // Build the session
    let mut session = trustee_core::session::Session::new();
    session.config_toml = Some(config_toml);
    session.secrets = Some(secrets);
    session.build_info = Some(build_info);
    session.parse_auto_handoff_config();

    // Create the broadcast channel for WebSocket fan-out
    let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(256);

    // Wrap session in shared state
    let state = ServerState::new(session, ws_tx);

    // Start background message drain task
    state.clone().spawn_drain_task();

    // Build router
    let app = axum::Router::new()
        // API routes
        .route("/api/v1/health", get(routes::health))
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

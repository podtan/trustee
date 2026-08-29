//! Trustee API — REST + WebSocket server for the Trustee agent.
//!
//! Wraps a [`trustee_core::session::Session`] and exposes it over HTTP.
//! Static frontend files are served from [`trustee_web`].
//!
//! Authentication is optional. When `[oidc]` or `[dev]` sections are present
//! in the config TOML, all `/api/v1/*` endpoints require a valid JWT or dev
//! token. Otherwise, all endpoints are open.

pub mod auth;
pub mod tls;
mod routes;
mod state;
mod thq_register;

// Embedded Cedar policy defaults (compiled into binary)
const EMBEDDED_CEDAR_POLICY: &str = include_str!("../policies/trustee_default.cedar");
const EMBEDDED_CEDAR_SCHEMA: &str = include_str!("../policies/trustee_schema.cedarschema");

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
///
/// By default serves over HTTPS using a self-signed certificate from
/// `~/.trustee/certs/`. If `use_tls` is false, serves plain HTTP.
pub async fn run(
    config_toml: String,
    secrets: std::collections::HashMap<String, String>,
    build_info: trustee_core::types::BuildInfo,
    addr: SocketAddr,
    use_tls: bool,
) -> Result<()> {
    // Parse auth config from TOML (returns None if no [oidc] or [dev] sections)
    let auth_state = if let Some(cfg) = AuthConfig::from_toml(&config_toml) {
        let is_dev = cfg.dev_config.local_dev_mode;
        tracing::info!(
            "Auth enabled: {} mode, issuer={}",
            if is_dev { "development" } else { "production" },
            cfg.issuer_url
        );

        // Parse optional Cedar authorization config
        let cedar_authorizer = parse_cedar_config(&config_toml).await;

        Some(Arc::new(AuthState::with_cedar(cfg, cedar_authorizer)))
    } else {
        None
    };

    // Parse THQ registration config before config_toml is moved into session
    let thq_config = thq_register::ThqConfig::from_toml(&config_toml);

    // Build the session — keep copies of secrets/build_info for per-user sessions
    let config_toml_for_state = config_toml.clone();
    let secrets_for_state = secrets.clone();
    let build_info_for_state = build_info.clone();
    let (mut session, workflow_rx) = trustee_core::session::Session::new();
    session.config_toml = Some(config_toml);
    session.secrets = Some(secrets);
    session.build_info = Some(build_info);
    session.parse_auto_handoff_config();

    // Extract agent name from config TOML for stateless operation
    if let Some(ref config_toml_str) = session.config_toml {
        if let Ok(table) = config_toml_str.parse::<toml::Value>() {
            if let Some(name) = table.get("agent").and_then(|a| a.get("name")).and_then(|n| n.as_str()) {
                session.agent_name = name.to_string();
            }
        }
    }

    // Create the broadcast channel for WebSocket fan-out
    let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(256);

    // Wrap session in shared state (with shared config/secrets/build_info for per-user sessions)
    // Parse knobs: [web].max_sessions_per_user, [users].allow_llm_overlay
    let (max_sessions, allow_llm_overlay) = {
        let config_str: &str = &config_toml_for_state;
        match toml::from_str::<toml::Value>(config_str) {
            Ok(v) => {
                let max_sessions = v
                    .get("web")
                    .and_then(|w| w.as_table())
                    .and_then(|w| w.get("max_sessions_per_user").and_then(|v| v.as_integer()))
                    .map(|v| v as usize)
                    .unwrap_or(4);
                let allow_llm_overlay = v
                    .get("users")
                    .and_then(|u| u.as_table())
                    .and_then(|u| u.get("allow_llm_overlay").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
                (max_sessions, allow_llm_overlay)
            }
            Err(_) => (4, false),
        }
    };

    let state = ServerState::new(session, ws_tx, auth_state)
        .with_config_toml(config_toml_for_state)
        .with_secrets(secrets_for_state)
        .with_build_info(build_info_for_state)
        .with_max_sessions_per_user(max_sessions)
        .with_allow_llm_overlay(allow_llm_overlay);

    // Start background message drain task (owns workflow_rx directly — no deadlock)
    state.clone().spawn_drain_task(workflow_rx);

    // THQ auto-registration with Torpi (if [thq] section is present in config)
    if let Some(cfg) = thq_config {
        thq_register::spawn(cfg);
    } else {
        tracing::debug!("THQ registration not configured (no [thq] section)");
    }

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
        .route("/api/v1/models", get(routes::list_models))
        .route("/api/v1/session", get(routes::get_session))
        .route("/api/v1/session/command", post(routes::post_command))
        .route("/api/v1/session/cancel", post(routes::post_cancel))
        .route("/api/v1/session/handoff", post(routes::post_handoff))
        .route("/api/v1/session/stream", get(routes::ws_handler))
        // Session naming
        .route("/api/v1/session/name", post(routes::set_session_name))
        .route("/api/v1/session/new", post(routes::new_session))
        .route("/api/v1/project/name", post(routes::set_project_name))
        // Session discovery & resume
        // Session discovery & resume (checkpoint-based, existing)
        .route("/api/v1/sessions", get(routes::list_sessions).post(routes::create_session))
        .route("/api/v1/sessions/live", get(routes::list_live_sessions))
        .route("/api/v1/sessions/{id}", get(routes::get_session_detail).delete(routes::destroy_session))
        .route("/api/v1/sessions/{id}/live", get(routes::get_live_session))
        .route("/api/v1/sessions/{id}/resume", post(routes::resume_session))
        .route("/api/v1/sessions/{id}/history", get(routes::get_session_history))
        // MSU: session-scoped live routes
        .route("/api/v1/sessions/{id}/command", post(routes::post_command_session))
        .route("/api/v1/sessions/{id}/cancel", post(routes::post_cancel_session))
        .route("/api/v1/sessions/{id}/handoff", post(routes::post_handoff_session))
        .route("/api/v1/sessions/{id}/name", post(routes::set_session_name_session))
        .route("/api/v1/sessions/{id}/stream", get(routes::ws_session_handler))
        // Static files from trustee-web
        .route("/", get(routes::serve_index))
        .route("/{file}", get(routes::serve_static))
        .layer(CorsLayer::permissive())
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;

    if use_tls {
        // Install ring as the process-level crypto provider (required when
        // rustls is built with default-features=false to avoid ambiguity
        // with aws-lc-rs pulled in transitively by other crates).
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Ensure self-signed certs exist
        let cert_dir = tls::default_cert_dir();
        let (cert_path, key_path) = tls::ensure_certs(&cert_dir)?;

        // Load TLS config
        let tls_config = tls::load_tls_config(&cert_path, &key_path)?;
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(tls_config));

        tracing::info!("Trustee API listening on https://{}", addr);

        // Manual accept loop — spawn hyper-util auto connection per TLS stream
        loop {
            let (tcp_stream, peer_addr) = match listener.accept().await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::warn!("TCP accept failed: {}", e);
                    continue;
                }
            };

            let acceptor = acceptor.clone();
            let app = app.clone();

            tokio::spawn(async move {
                let tls_stream = match acceptor.accept(tcp_stream).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!("TLS accept failed from {}: {}", peer_addr, e);
                        return;
                    }
                };

                // Use hyper-util auto builder with the tower service from axum.
                // serve_connection_with_upgrades is required for WebSocket support.
                let io = hyper_util::rt::TokioIo::new(tls_stream);
                let svc = hyper_util::service::TowerToHyperService::new(app);

                let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection_with_upgrades(io, svc)
                    .await;
            });
        }
    } else {
        tracing::info!("Trustee API listening on http://{}", addr);
        axum::serve(listener, app).await?;
    }

    Ok(())
}

/// Parse [cedar] section from config TOML and create a CedarAuthorizer if enabled.
///
/// Configuration:
/// - `[cedar] enabled = true/false` (default: false)
/// - `[cedar] policy_path = "/path/to/policies.cedar"` (filesystem override)
/// - `[cedar] schema_path = "/path/to/schema.cedarschema"` (filesystem override)
/// - `[cedar] policy_store_url = "https://..."` (remote policy store)
///
/// When enabled without filesystem paths, uses embedded defaults.
async fn parse_cedar_config(config_toml: &str) -> Option<Arc<pep::cedar::CedarAuthorizer>> {
    let table: toml::Table = match toml::from_str(config_toml) {
        Ok(t) => t,
        Err(_) => return None,
    };

    let cedar_section = table.get("cedar")?.as_table()?;
    let enabled = cedar_section
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !enabled {
        tracing::debug!("Cedar authorization disabled (default)");
        return None;
    }

    tracing::info!("Cedar authorization enabled — initializing authorizer");

    // Default policy/schema paths point to ~/{agent_name}/policies/ (created by trustee init).
    // Agent name is read from [agent] name in config, defaulting to "trustee".
    let agent_name = table
        .get("agent")
        .and_then(|a| a.as_table())
        .and_then(|a| a.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("trustee");

    let home_policies_dir = dirs::home_dir()
        .map(|h| h.join(format!(".{}", agent_name)).join("policies"))
        .unwrap_or_else(|| std::path::PathBuf::from("/nonexistent"));

    let default_policy_path = home_policies_dir.join("trustee_default.cedar");
    let default_schema_path = home_policies_dir.join("trustee_schema.cedarschema");

    let policy_path = cedar_section
        .get("policy_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or(default_policy_path);

    let schema_path = cedar_section
        .get("schema_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| Some(default_schema_path));

    let policy_store_url = cedar_section
        .get("policy_store_url")
        .and_then(|v| v.as_str())
        .map(String::from);

    let policy_store_token = cedar_section
        .get("policy_store_token")
        .and_then(|v| v.as_str())
        .map(String::from);

    let cedar_config = pep::cedar::CedarConfig {
        policy_path,
        schema_path,
        entities_path: None,
        default_decision: pep::cedar::DefaultDecision::Deny,
        validate_on_load: true,
        policy_store_url,
        policy_store_token,
        embedded_policy: Some(EMBEDDED_CEDAR_POLICY),
        embedded_schema: Some(EMBEDDED_CEDAR_SCHEMA),
    };

    match pep::cedar::CedarAuthorizer::new_with_policy_store(cedar_config).await {
        Ok(auth) => {
            tracing::info!("Cedar authorizer initialized successfully");
            Some(Arc::new(auth))
        }
        Err(e) => {
            tracing::error!("Failed to initialize Cedar authorizer: {}", e);
            tracing::warn!("Cedar was enabled but initialization failed — auth will proceed WITHOUT Cedar");
            None
        }
    }
}

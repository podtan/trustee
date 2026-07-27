//! Shared server state: per-user session registry, broadcast channels, and auth state.
//!
//! ## Multi-User Architecture (TMU Phase 2)
//!
//! Each authenticated user gets their own [`UserSession`] containing:
//! - An independent `Session` (workflow state, output, etc.)
//! - A dedicated broadcast channel for WebSocket fan-out
//! - A per-user token store for MCP credential isolation
//!
//! Sessions are keyed by user identity (`sub` claim from JWT, or `dev:email` for
//! dev mode). Unauthenticated deployments use a single `"default"` key, preserving
//! backward compatibility with single-user CLI operation.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc, Mutex};
use trustee_core::session::Session;
use trustee_core::types::TuiMessage;

use crate::auth::AuthState;

/// Per-user session bundle.
///
/// Each user gets their own Session instance, broadcast channel, and
/// token store. This struct is stored in the [`SessionRegistry`]
/// and accessed via the user's identity key.
pub struct UserSession {
    /// The agent session, protected by a mutex.
    pub session: Arc<Mutex<Session>>,
    /// Broadcast sender for this user's WebSocket fan-out.
    pub ws_tx: broadcast::Sender<String>,
    /// Per-user in-memory token store for MCP credential isolation.
    ///
    /// Replaces the process-wide FileTokenStore that was vulnerable to
    /// cross-user token leakage via __web_session.json. Each user's
    /// MCP `web-session` tokens are stored here, isolated from other users.
    pub token_store: Arc<pep::MemoryTokenStore>,
}

impl UserSession {
    /// Create a new per-user session from an existing Session.
    ///
    /// Creates a fresh broadcast channel (256 capacity) for WebSocket fan-out
    /// and a per-user MemoryTokenStore for MCP credential isolation.
    pub fn new(session: Session) -> Self {
        let (ws_tx, _ws_rx) = broadcast::channel::<String>(256);
        let token_store = Arc::new(pep::MemoryTokenStore::new());
        Self {
            session: Arc::new(Mutex::new(session)),
            ws_tx,
            token_store,
        }
    }
}

/// Concurrent registry of per-user sessions.
///
/// Keyed by user identity string:
/// - Authenticated: JWT `sub` claim (e.g., Kanidm UUID)
/// - Dev mode: `dev:{email}`
/// - No auth: `"default"`
///
/// Falls back to the `"default"` entry when no user key is provided,
/// preserving backward compatibility.
pub type SessionRegistry = Arc<DashMap<String, UserSession>>;

/// Shared state accessible by all axum handlers.
#[derive(Clone)]
pub struct ServerState {
    /// Per-user session registry (TMU Phase 2).
    pub sessions: SessionRegistry,
    /// Broadcast sender for backward compat — delegates to the default user's channel.
    /// New code should use `user_ws_tx(user_key)` instead.
    pub ws_tx: broadcast::Sender<String>,
    /// Auth state (None = auth disabled, all endpoints open).
    pub auth: Option<Arc<AuthState>>,
    /// Shared config TOML (all users share the same agent config).
    pub config_toml: Option<String>,
}

impl ServerState {
    /// Create new shared state from a default session, broadcast sender, and optional auth.
    ///
    /// The provided session becomes the `"default"` user's session. When auth is
    /// enabled, authenticated users get their own sessions created on demand.
    pub fn new(
        session: Session,
        ws_tx: broadcast::Sender<String>,
        auth: Option<Arc<AuthState>>,
    ) -> Self {
        let sessions = Arc::new(DashMap::new());

        // Store the default session under the "default" key
        // Use the provided ws_tx as the default user's broadcast channel
        let token_store = Arc::new(pep::MemoryTokenStore::new());
        sessions.insert(
            "default".to_string(),
            UserSession {
                session: Arc::new(Mutex::new(session)),
                ws_tx: ws_tx.clone(),
                token_store,
            },
        );

        Self {
            sessions,
            ws_tx,
            auth,
            config_toml: None,
        }
    }

    /// Set the shared config TOML.
    pub fn with_config_toml(mut self, config_toml: String) -> Self {
        self.config_toml = Some(config_toml);
        self
    }

    /// Get or create a session for the given user key, returning the session + ws_tx.
    ///
    /// This is the main entry point for route handlers. It ensures the user
    /// has a session, spawns a drain task if newly created, and returns
    /// references to the session mutex, broadcast sender, and token store.
    pub async fn ensure_user_session(
        &self,
        user_key: &str,
    ) -> (Arc<Mutex<Session>>, broadcast::Sender<String>, Arc<pep::MemoryTokenStore>) {
        // Fast path: user already has a session
        if let Some(entry) = self.sessions.get(user_key) {
            return (
                entry.session.clone(),
                entry.ws_tx.clone(),
                entry.token_store.clone(),
            );
        }

        // Slow path: create new session for this user
        let (mut session, workflow_rx) = Session::new();

        // Copy shared config
        if let Some(ref config_toml) = self.config_toml {
            session.config_toml = Some(config_toml.clone());
            session.parse_auto_handoff_config();

            if let Ok(table) = config_toml.parse::<toml::Value>() {
                if let Some(name) = table.get("agent").and_then(|a| a.get("name")).and_then(|n| n.as_str()) {
                    session.agent_name = name.to_string();
                }
            }
        }

        // Isolate checkpoint storage per user by setting a unique project_id.
        // The project_id becomes the storage partition key in ABK's checkpoint
        // system. By prefixing with the user_key, each user's checkpoints are
        // stored in separate directories, preventing cross-user access.
        // The "default" user (no auth) keeps the legacy behavior (no project_id).
        if user_key != "default" {
            session.project_id = Some(format!("user:{user_key}"));
        }

        let user_session = UserSession::new(session);
        let session_arc = user_session.session.clone();
        let ws_tx = user_session.ws_tx.clone();
        let token_store = user_session.token_store.clone();

        self.sessions.insert(user_key.to_string(), user_session);

        // Spawn drain task for this user's workflow receiver
        self.spawn_user_drain_task(
            user_key.to_string(),
            session_arc.clone(),
            ws_tx.clone(),
            workflow_rx,
        );

        (session_arc, ws_tx, token_store)
    }

    /// Spawn a background drain task for a specific user's workflow receiver.
    ///
    /// This replaces the old global drain task — each user gets their own.
    fn spawn_user_drain_task(
        &self,
        user_key: String,
        session: Arc<Mutex<Session>>,
        ws_tx: broadcast::Sender<String>,
        mut workflow_rx: mpsc::UnboundedReceiver<TuiMessage>,
    ) {
        tokio::spawn(async move {
            while let Some(msg) = workflow_rx.recv().await {
                // Process the message through Session's handler (updates state)
                {
                    let mut session = session.lock().await;
                    session.handle_workflow_message(msg.clone());

                    let state_str = match session.workflow_state {
                        trustee_core::types::WorkflowState::Idle => "Idle",
                        trustee_core::types::WorkflowState::Running => "Running",
                        trustee_core::types::WorkflowState::Cancelling => "Cancelling",
                    };
                    let state_msg = serde_json::json!({
                        "type": "StateChanged",
                        "state": state_str
                    });
                    let _ = ws_tx.send(state_msg.to_string());
                }

                // Broadcast the raw message to WebSocket clients
                let json = serde_json::to_string(&SerializableMessage(&msg)).unwrap_or_default();
                let _ = ws_tx.send(json);
            }
            tracing::debug!("Drain task ended for user: {}", user_key);
        });
    }

    /// Spawn the default user's drain task (backward compatibility).
    ///
    /// Called during server startup for the initial session.
    pub fn spawn_drain_task(self, mut workflow_rx: mpsc::UnboundedReceiver<TuiMessage>) {
        // Get the default session's arc
        let default_entry = self.sessions.get("default").expect("default session must exist");
        let session = default_entry.session.clone();
        let ws_tx = default_entry.ws_tx.clone();
        drop(default_entry);

        tokio::spawn(async move {
            while let Some(msg) = workflow_rx.recv().await {
                {
                    let mut session = session.lock().await;
                    session.handle_workflow_message(msg.clone());

                    let state_str = match session.workflow_state {
                        trustee_core::types::WorkflowState::Idle => "Idle",
                        trustee_core::types::WorkflowState::Running => "Running",
                        trustee_core::types::WorkflowState::Cancelling => "Cancelling",
                    };
                    let state_msg = serde_json::json!({
                        "type": "StateChanged",
                        "state": state_str
                    });
                    let _ = ws_tx.send(state_msg.to_string());
                }

                let json = serde_json::to_string(&SerializableMessage(&msg)).unwrap_or_default();
                let _ = ws_tx.send(json);
            }
        });
    }

    /// Resolve the user key from request headers.
    ///
    /// Returns `"default"` when auth is disabled.
    /// Returns the JWT `sub` claim (or `dev:email` for dev mode) when auth is enabled.
    pub async fn resolve_user_key(&self, headers: &axum::http::HeaderMap) -> String {
        let Some(ref auth) = self.auth else {
            return "default".to_string();
        };

        // Try Bearer header first
        if let Some(token) = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
        {
            // Dev token
            if token.starts_with("dev:") {
                let parts: Vec<&str> = token.splitn(4, ':').collect();
                if parts.len() >= 4 {
                    return format!("dev:{}", parts[1]);
                }
            }
            // Real JWT — extract sub claim
            if let Ok(claims) = auth.validate_token(&token).await {
                return claims.sub;
            }
        }

        // Try cookie
        let cookie_session_id = headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|cookies| {
                cookies
                    .split(';')
                    .map(|c| c.trim())
                    .find_map(|c| c.strip_prefix(&format!("{}=", auth.config.cookie_name)))
                    .map(|s| s.to_string())
            });

        if let Some(session_id) = cookie_session_id {
            // Dev token in cookie
            if session_id.starts_with("dev:") {
                let parts: Vec<&str> = session_id.splitn(4, ':').collect();
                if parts.len() >= 4 {
                    return format!("dev:{}", parts[1]);
                }
            }

            // Resolve session_id → access_token → sub claim
            if let Ok(access_token) = auth.session_manager.get_token(&session_id).await {
                if let Ok(claims) = auth.validate_token(&access_token).await {
                    return claims.sub;
                }
            }
        }

        "default".to_string()
    }
}

/// Wrapper to serialize `TuiMessage` as JSON with a `type` discriminator.
struct SerializableMessage<'a>(&'a TuiMessage);

impl<'a> serde::Serialize for SerializableMessage<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        match self.0 {
            TuiMessage::OutputLine(line) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "OutputLine")?;
                s.serialize_field("line", line)?;
                s.end()
            }
            TuiMessage::StreamDelta(delta) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "StreamDelta")?;
                s.serialize_field("delta", delta)?;
                s.end()
            }
            TuiMessage::ReasoningDelta(delta) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "ReasoningDelta")?;
                s.serialize_field("delta", delta)?;
                s.end()
            }
            TuiMessage::WorkflowCompleted => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "WorkflowCompleted")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::WorkflowError(err) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "WorkflowError")?;
                s.serialize_field("error", err)?;
                s.end()
            }
            TuiMessage::ResumeInfo(_) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "ResumeInfo")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::TodoUpdate(content) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "TodoUpdate")?;
                s.serialize_field("content", content)?;
                s.end()
            }
            TuiMessage::WorkflowCancelled => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "WorkflowCancelled")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::HandoffReady(_) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "HandoffReady")?;
                s.serialize_field("state", "Idle")?;
                s.end()
            }
            TuiMessage::ToolPending { tool_name, hint } => {
                let mut s = serializer.serialize_struct("msg", 3)?;
                s.serialize_field("type", "ToolPending")?;
                s.serialize_field("tool_name", tool_name)?;
                s.serialize_field("hint", hint)?;
                s.end()
            }
            TuiMessage::ToolDone { tool_name, success, hint } => {
                let mut s = serializer.serialize_struct("msg", 4)?;
                s.serialize_field("type", "ToolDone")?;
                s.serialize_field("tool_name", tool_name)?;
                s.serialize_field("success", success)?;
                s.serialize_field("hint", hint)?;
                s.end()
            }
            TuiMessage::ContextTokensUpdated(count) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "ContextTokensUpdated")?;
                s.serialize_field("count", count)?;
                s.end()
            }
            TuiMessage::McpServerStatus { name, connected, tool_count, error } => {
                let mut s = serializer.serialize_struct("msg", 5)?;
                s.serialize_field("type", "McpServerStatus")?;
                s.serialize_field("name", name)?;
                s.serialize_field("connected", connected)?;
                s.serialize_field("tool_count", tool_count)?;
                s.serialize_field("error", error)?;
                s.end()
            }
        }
    }
}

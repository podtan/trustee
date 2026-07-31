//! Shared server state: per-user multi-session registry, broadcast channels, and auth state.
//!
//! ## Multi-Session Per User (MSU)
//!
//! Each authenticated user gets their own [`UserSessions`] containing N independent
//! [`UserSessionEntry`] instances (default max 4). Each entry has:
//! - An independent `Session` (workflow state, output, etc.)
//! - A dedicated broadcast channel for WebSocket fan-out
//! - Creation and last-active timestamps
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

// ---------------------------------------------------------------------------
// Multi-session types
// ---------------------------------------------------------------------------

/// A single session with its own broadcast channel.
pub struct UserSessionEntry {
    /// The agent session, protected by a mutex.
    pub session: Arc<Mutex<Session>>,
    /// Broadcast sender for this session's WebSocket fan-out.
    pub ws_tx: broadcast::Sender<String>,
    /// When this session was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last time a command was submitted or state changed.
    /// Updated on every /sessions/{id}/command and /sessions/{id}/cancel call.
    pub last_active: Arc<Mutex<chrono::DateTime<chrono::Utc>>>,
}

/// All sessions belonging to one authenticated user.
pub struct UserSessions {
    /// session_id → session entry
    pub sessions: DashMap<String, UserSessionEntry>,
    /// Shared token store for all this user's sessions (MCP credential isolation).
    pub token_store: Arc<pep::MemoryTokenStore>,
    /// Which session_id is "active" for legacy /session/* routes.
    pub active_session_id: Mutex<String>,
}

/// Summary of an active session for listing (serializable for API responses).
#[derive(Debug, serde::Serialize)]
pub struct SessionListItem {
    pub session_id: String,
    pub session_name: Option<String>,
    pub workflow_state: String,
    pub created_at: String,
    pub last_active: String,
}

/// Errors from multi-session operations.
#[derive(Debug)]
pub enum SessionError {
    /// User has reached max_sessions_per_user limit.
    MaxSessionsReached(usize),
    /// Session ID not found for this user.
    NotFound(String),
    /// Session is not Idle (cannot destroy/overwrite a running session).
    NotIdle(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::MaxSessionsReached(n) => {
                write!(f, "Maximum {} sessions per user reached", n)
            }
            SessionError::NotFound(id) => write!(f, "Session {} not found", id),
            SessionError::NotIdle(state) => write!(f, "Session is not idle (state: {})", state),
        }
    }
}

impl std::error::Error for SessionError {}

/// Top-level registry: user_key → user's session collection.
pub type SessionRegistry = Arc<DashMap<String, UserSessions>>;

// ---------------------------------------------------------------------------
// ServerState
// ---------------------------------------------------------------------------

/// Shared state accessible by all axum handlers.
#[derive(Clone)]
pub struct ServerState {
    /// Per-user multi-session registry (MSU).
    pub sessions: SessionRegistry,
    /// Broadcast sender for backward compat — delegates to the default user's channel.
    pub ws_tx: broadcast::Sender<String>,
    /// Auth state (None = auth disabled, all endpoints open).
    pub auth: Option<Arc<AuthState>>,
    /// Shared config TOML (all users share the same agent config).
    pub config_toml: Option<String>,
    /// Shared secrets (injected into every per-user session).
    pub secrets: Option<std::collections::HashMap<String, String>>,
    /// Shared build info (injected into every per-user session).
    pub build_info: Option<trustee_core::types::BuildInfo>,
    /// Global concurrency limiter — limits the number of simultaneous workflows
    /// across all users. Default: 8 concurrent workflows.
    pub workflow_semaphore: Arc<tokio::sync::Semaphore>,
    /// Maximum number of concurrent sessions per user. Default: 4.
    pub max_sessions_per_user: usize,
}

impl ServerState {
    /// Create new shared state from a default session, broadcast sender, and optional auth.
    pub fn new(
        session: Session,
        ws_tx: broadcast::Sender<String>,
        auth: Option<Arc<AuthState>>,
    ) -> Self {
        let sessions = Arc::new(DashMap::new());

        // Store the default user's UserSessions with an initial session
        let token_store = Arc::new(pep::MemoryTokenStore::new());
        let (ws_tx_entry, _) = broadcast::channel::<String>(256);

        let now = chrono::Utc::now();
        let initial_entry = UserSessionEntry {
            session: Arc::new(Mutex::new(session)),
            ws_tx: ws_tx_entry,
            created_at: now,
            last_active: Arc::new(Mutex::new(now)),
        };

        let user_sessions = UserSessions {
            sessions: DashMap::new(),
            token_store,
            active_session_id: Mutex::new(String::new()),
        };
        user_sessions.sessions.insert("default".to_string(), initial_entry);

        sessions.insert("default".to_string(), user_sessions);

        Self {
            sessions,
            ws_tx,
            auth,
            config_toml: None,
            secrets: None,
            build_info: None,
            workflow_semaphore: Arc::new(tokio::sync::Semaphore::new(8)),
            max_sessions_per_user: 4,
        }
    }

    pub fn with_config_toml(mut self, config_toml: String) -> Self {
        self.config_toml = Some(config_toml);
        self
    }

    pub fn with_secrets(mut self, secrets: std::collections::HashMap<String, String>) -> Self {
        self.secrets = Some(secrets);
        self
    }

    pub fn with_build_info(mut self, build_info: trustee_core::types::BuildInfo) -> Self {
        self.build_info = Some(build_info);
        self
    }

    pub fn with_max_concurrent_workflows(mut self, max: usize) -> Self {
        self.workflow_semaphore = Arc::new(tokio::sync::Semaphore::new(max));
        self
    }

    /// Set the max sessions per user.
    pub fn with_max_sessions_per_user(mut self, max: usize) -> Self {
        self.max_sessions_per_user = max;
        self
    }

    // -----------------------------------------------------------------------
    // MSU: Multi-session methods
    // -----------------------------------------------------------------------

    /// Create a new session for a user. Returns the session_id.
    ///
    /// Creates a fresh `Session::new()`, copies shared config, sets per-user
    /// isolation, creates a broadcast channel, spawns a drain task, and inserts
    /// into the user's session DashMap. The new session becomes the "active" one.
    pub async fn create_session(
        &self,
        user_key: &str,
        session_name: Option<String>,
    ) -> Result<String, SessionError> {
        // Get or create the user's UserSessions entry
        let user_sessions = self
            .sessions
            .entry(user_key.to_string())
            .or_insert_with(|| UserSessions {
                sessions: DashMap::new(),
                token_store: Arc::new(pep::MemoryTokenStore::new()),
                active_session_id: Mutex::new(String::new()),
            });

        // Check session limit
        if user_sessions.sessions.len() >= self.max_sessions_per_user {
            return Err(SessionError::MaxSessionsReached(self.max_sessions_per_user));
        }

        // Create new Session
        let (mut session, workflow_rx) = Session::new();

        // Copy shared config
        if let Some(ref config_toml) = self.config_toml {
            session.config_toml = Some(config_toml.clone());
            session.parse_auto_handoff_config();
            if let Ok(table) = config_toml.parse::<toml::Value>() {
                if let Some(name) = table
                    .get("agent")
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                {
                    session.agent_name = name.to_string();
                }
            }
        }

        session.secrets = self.secrets.clone();
        session.build_info = self.build_info.clone();

        // Per-user isolation
        self.apply_user_isolation(&mut session, user_key);

        // Apply session_name if provided
        session.session_name = session_name;

        // Create broadcast channel
        let (ws_tx_entry, _) = broadcast::channel::<String>(256);

        // Generate session_id
        let session_id = format!(
            "session_{}_{}",
            chrono::Utc::now().format("%Y_%m_%d_%H_%M"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        let now = chrono::Utc::now();

        // Insert into user's sessions DashMap
        user_sessions.sessions.insert(
            session_id.clone(),
            UserSessionEntry {
                session: Arc::new(Mutex::new(session)),
                ws_tx: ws_tx_entry.clone(),
                created_at: now,
                last_active: Arc::new(Mutex::new(now)),
            },
        );

        // Set as active session
        *user_sessions.active_session_id.lock().await = session_id.clone();

        // Spawn drain task
        let session_arc = user_sessions
            .sessions
            .get(&session_id)
            .map(|e| e.session.clone());
        if let Some(session_arc) = session_arc {
            self.spawn_user_drain_task(
                session_id.clone(),
                session_arc,
                ws_tx_entry,
                workflow_rx,
            );
        }

        Ok(session_id)
    }

    /// Get a specific session by user_key + session_id.
    /// Updates last_active on the session entry.
    pub async fn get_session(
        &self,
        user_key: &str,
        session_id: &str,
    ) -> Option<(Arc<Mutex<Session>>, broadcast::Sender<String>)> {
        let user_sessions = self.sessions.get(user_key)?;
        let entry = user_sessions.sessions.get(session_id)?;

        // Update last_active
        let now = chrono::Utc::now();
        *entry.last_active.lock().await = now;

        Some((entry.session.clone(), entry.ws_tx.clone()))
    }

    /// List all active sessions for a user, sorted by last_active desc.
    pub async fn list_sessions(&self, user_key: &str) -> Vec<SessionListItem> {
        let Some(user_sessions) = self.sessions.get(user_key) else {
            return Vec::new();
        };

        let mut items = Vec::new();
        for entry in user_sessions.sessions.iter() {
            let session = entry.session.lock().await;
            let workflow_state = match session.workflow_state {
                trustee_core::types::WorkflowState::Idle => "Idle",
                trustee_core::types::WorkflowState::Running => "Running",
                trustee_core::types::WorkflowState::Cancelling => "Cancelling",
            };
            let last_active = entry.last_active.lock().await;
            items.push(SessionListItem {
                session_id: entry.key().clone(),
                session_name: session.session_name.clone(),
                workflow_state: workflow_state.to_string(),
                created_at: entry.created_at.to_rfc3339(),
                last_active: last_active.to_rfc3339(),
            });
        }
        drop(user_sessions);

        // Sort by last_active descending
        items.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        items
    }

    /// Destroy a session. The session must be Idle.
    pub async fn destroy_session(
        &self,
        user_key: &str,
        session_id: &str,
    ) -> Result<(), SessionError> {
        let user_sessions = self
            .sessions
            .get(user_key)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;

        // Check workflow state before removing
        {
            let entry = user_sessions
                .sessions
                .get(session_id)
                .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;
            let session = entry.session.lock().await;
            if session.workflow_state != trustee_core::types::WorkflowState::Idle {
                let state_str = match session.workflow_state {
                    trustee_core::types::WorkflowState::Running => "Running",
                    trustee_core::types::WorkflowState::Cancelling => "Cancelling",
                    _ => "Unknown",
                };
                return Err(SessionError::NotIdle(state_str.to_string()));
            }
        }

        // Remove from DashMap
        user_sessions.sessions.remove(session_id);

        // If this was the active session, pick a new active
        let mut active_id = user_sessions.active_session_id.lock().await;
        if &*active_id == session_id {
            // Pick the most recently active remaining session
            let mut newest: Option<(String, chrono::DateTime<chrono::Utc>)> = None;
            for entry in user_sessions.sessions.iter() {
                let la = entry.last_active.lock().await;
                if newest.as_ref().map_or(true, |(_, t)| *la > *t) {
                    newest = Some((entry.key().clone(), *la));
                }
            }
            *active_id = newest.map(|(id, _)| id).unwrap_or_default();
        }

        Ok(())
    }

    /// Get or create the user's "active" session for legacy routes.
    ///
    /// Behavior:
    /// 1. If user has no sessions → create one
    /// 2. If active session exists → return it
    /// 3. If active session was destroyed → create a new one
    ///
    /// Returns: (session_id, session_arc, ws_tx, token_store)
    pub async fn ensure_active_session(
        &self,
        user_key: &str,
    ) -> (
        String,
        Arc<Mutex<Session>>,
        broadcast::Sender<String>,
        Arc<pep::MemoryTokenStore>,
    ) {
        // Get or create user's UserSessions
        let token_store = {
            let user_sessions = self
                .sessions
                .entry(user_key.to_string())
                .or_insert_with(|| UserSessions {
                    sessions: DashMap::new(),
                    token_store: Arc::new(pep::MemoryTokenStore::new()),
                    active_session_id: Mutex::new(String::new()),
                });
            user_sessions.token_store.clone()
        };

        // Check if active session exists
        let active_id = {
            let user_sessions = self.sessions.get(user_key).unwrap();
            let guard = user_sessions.active_session_id.lock().await;
            guard.clone()
        };

        if !active_id.is_empty() {
            if let Some((session, ws_tx)) = self.get_session(user_key, &active_id).await {
                return (active_id, session, ws_tx, token_store);
            }
            // Active session was destroyed, fall through to create
        }

        // Need to create a new session
        // For the "default" user, we may already have a "default" session entry
        // from ServerState::new() — check for it
        let existing_session: Option<(String, Arc<Mutex<Session>>, broadcast::Sender<String>)> = {
            let user_sessions = self.sessions.get(user_key).unwrap();
            let result = user_sessions.sessions.iter().next().map(|first| {
                (
                    first.key().clone(),
                    first.session.clone(),
                    first.ws_tx.clone(),
                )
            });
            result
        };
        if let Some((id, session, ws_tx)) = existing_session {
            let now = chrono::Utc::now();
            if let Some(entry) = self.sessions.get(user_key) {
                if let Some(e) = entry.sessions.get(&id) {
                    *e.last_active.lock().await = now;
                }
                *entry.active_session_id.lock().await = id.clone();
            }

            return (id, session, ws_tx, token_store);
        }

        // Create a brand new session
        let session_id = self
            .create_session(user_key, None)
            .await
            .unwrap_or_else(|_| "default".to_string());

        let (session, ws_tx) = self
            .get_session(user_key, &session_id)
            .await
            .expect("just-created session must exist");

        (session_id, session, ws_tx, token_store)
    }

    /// DEPRECATED: Use ensure_active_session() instead.
    /// Kept for backward compatibility — same 3-tuple return type.
    pub async fn ensure_user_session(
        &self,
        user_key: &str,
    ) -> (Arc<Mutex<Session>>, broadcast::Sender<String>, Arc<pep::MemoryTokenStore>) {
        let (_id, session, ws_tx, token_store) = self.ensure_active_session(user_key).await;
        (session, ws_tx, token_store)
    }

    /// Set a session as the user's active session.
    pub async fn set_active_session(&self, user_key: &str, session_id: &str) {
        if let Some(user_sessions) = self.sessions.get(user_key) {
            if user_sessions.sessions.contains_key(session_id) {
                *user_sessions.active_session_id.lock().await = session_id.to_string();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Read-only helpers (no session creation side effects)
    // -----------------------------------------------------------------------

    /// Resolve a user's home_dir without creating an in-memory session.
    ///
    /// This is the read-only equivalent of the isolation logic in
    /// `apply_user_isolation`. Used by endpoints that only need to read
    /// checkpoint data from disk (history, session list, session detail)
    /// and must NOT create ghost sessions as a side effect.
    pub fn get_user_home_dir(&self, user_key: &str) -> Option<std::path::PathBuf> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(user_key.as_bytes());
        let hash_bytes = hasher.finalize();
        let user_hash = format!(
            "{:016x}",
            u64::from_be_bytes(hash_bytes[..8].try_into().unwrap())
        );
        dirs::home_dir().map(|home| home.join(".trustee").join("users").join(&user_hash))
    }

    /// Resolve config_toml and home_dir without creating an in-memory session.
    ///
    /// Returns `(config_toml, home_dir)`. If config is not loaded,
    /// config_toml will be None.
    pub fn get_user_config_and_home(&self, user_key: &str) -> (Option<String>, Option<std::path::PathBuf>) {
        (self.config_toml.clone(), self.get_user_home_dir(user_key))
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Apply per-user isolation: SHA-256 hash → home_dir + project_id.
    fn apply_user_isolation(&self, session: &mut Session, user_key: &str) {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(user_key.as_bytes());
        let hash_bytes = hasher.finalize();
        let user_hash = format!(
            "{:016x}",
            u64::from_be_bytes(hash_bytes[..8].try_into().unwrap())
        );

        if let Some(home) = dirs::home_dir() {
            session.home_dir = Some(home.join(".trustee").join("users").join(&user_hash));
        }
        session.project_id = Some(format!("web{}", &user_hash[..16]));
    }

    /// Spawn a background drain task for a specific session's workflow receiver.
    fn spawn_user_drain_task(
        &self,
        session_id: String,
        session: Arc<Mutex<Session>>,
        ws_tx: broadcast::Sender<String>,
        mut workflow_rx: mpsc::UnboundedReceiver<TuiMessage>,
    ) {
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

                let json =
                    serde_json::to_string(&SerializableMessage(&msg)).unwrap_or_default();
                let _ = ws_tx.send(json);
            }
            tracing::debug!("Drain task ended for session: {}", session_id);
        });
    }

    /// Spawn the default user's drain task (backward compatibility).
    /// Called during server startup for the initial session.
    pub fn spawn_drain_task(self, mut workflow_rx: mpsc::UnboundedReceiver<TuiMessage>) {
        // Get the default user's first session
        let default_user = self
            .sessions
            .get("default")
            .expect("default user must exist");
        let first_entry = default_user
            .sessions
            .iter()
            .next()
            .expect("default user must have at least one session");
        let session = first_entry.session.clone();
        let ws_tx = first_entry.ws_tx.clone();
        let session_id = first_entry.key().clone();
        drop(first_entry);
        drop(default_user);

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

                let json =
                    serde_json::to_string(&SerializableMessage(&msg)).unwrap_or_default();
                let _ = ws_tx.send(json);
            }
            tracing::debug!("Drain task ended for session: {}", session_id);
        });
    }

    /// Resolve the user key from request headers.
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
            if token.starts_with("dev:") {
                let parts: Vec<&str> = token.splitn(4, ':').collect();
                if parts.len() >= 4 {
                    return format!("dev:{}", parts[1]);
                }
            }
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
                    .find_map(|c| {
                        c.strip_prefix(&format!("{}=", auth.config.cookie_name))
                            .map(|s| s.to_string())
                    })
            });

        if let Some(session_id) = cookie_session_id {
            if session_id.starts_with("dev:") {
                let parts: Vec<&str> = session_id.splitn(4, ':').collect();
                if parts.len() >= 4 {
                    return format!("dev:{}", parts[1]);
                }
            }

            if let Ok(access_token) = auth.session_manager.get_token(&session_id).await {
                if let Ok(claims) = auth.validate_token(&access_token).await {
                    return claims.sub;
                }
            }
        }

        "default".to_string()
    }
}

// ---------------------------------------------------------------------------
// SerializableMessage (unchanged)
// ---------------------------------------------------------------------------

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
            TuiMessage::ResumeInfo(info) => match info {
                Some(ri) => {
                    let mut s = serializer.serialize_struct("msg", 5)?;
                    s.serialize_field("type", "ResumeInfo")?;
                    s.serialize_field("state", "Idle")?;
                    s.serialize_field("session_id", &ri.session_id)?;
                    s.serialize_field("checkpoint_id", &ri.checkpoint_id)?;
                    s.serialize_field("iteration", &ri.iteration)?;
                    s.end()
                }
                None => {
                    let mut s = serializer.serialize_struct("msg", 2)?;
                    s.serialize_field("type", "ResumeInfo")?;
                    s.serialize_field("state", "Idle")?;
                    s.end()
                }
            },
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
            TuiMessage::ToolPending {
                tool_name,
                hint,
            } => {
                let mut s = serializer.serialize_struct("msg", 3)?;
                s.serialize_field("type", "ToolPending")?;
                s.serialize_field("tool_name", tool_name)?;
                s.serialize_field("hint", hint)?;
                s.end()
            }
            TuiMessage::ToolDone {
                tool_name,
                success,
                hint,
            } => {
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
            TuiMessage::McpServerStatus {
                name,
                connected,
                tool_count,
                error,
            } => {
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

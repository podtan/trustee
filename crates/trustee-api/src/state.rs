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
    /// Number of handoff rotations this session has performed (Bug 5). The
    /// in-memory registry key never changes across a rotation — only the
    /// checkpoint-chain name does — so a card whose name changed without this
    /// hint looks like it silently renamed. 0 = never rotated.
    pub handoff_count: u32,
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

/// Cached per-user MCP tool loader (16C).
///
/// One entry per user hash. `loader: None` means the user's effective
/// config has MCP disabled — agents run MCP-less via abk's
/// `McpSource::Prebuilt(None)` (semantically identical to
/// `[mcp] enabled = false` today, but with zero config re-parsing).
pub struct McpLoaderEntry {
    /// Built loader; `None` = MCP disabled for this user.
    pub loader: Option<std::sync::Arc<abk::agent::McpToolLoader>>,
    /// Content fingerprint of the effective `[mcp]` config (SHA-256, first
    /// 8 bytes as u64). Content, never mtime — overlays are rewritten in place.
    pub fingerprint: u64,
    pub built_at: chrono::DateTime<chrono::Utc>,
    /// Set when the last build failed; the message surfaces to the user's
    /// next dispatch (fail loud). Other users are never affected.
    pub degraded: Option<String>,
    /// When `degraded` was set. Rebuilds are held back for
    /// [`MCP_BUILD_RETRY_BACKOFF`] — a poison entry never sticks forever.
    pub failed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Minimum delay before retrying a failed MCP loader build (16C).
pub const MCP_BUILD_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

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
    /// Whether per-user config overlays may override the `[llm]` section.
    /// Default: `false` — overlays are limited to `[mcp]`.
    pub allow_llm_overlay: bool,
    /// Per-user McpToolLoader cache (16C), keyed by user hash (never the raw key).
    pub mcp_loaders: Arc<DashMap<String, McpLoaderEntry>>,
    /// Single-flight build locks per user hash (16C).
    mcp_build_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
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
            allow_llm_overlay: false,
            mcp_loaders: Arc::new(DashMap::new()),
            mcp_build_locks: Arc::new(DashMap::new()),
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

    /// Allow per-user config overlays to override the `[llm]` section.
    /// Default: `false` (overlays limited to `[mcp]`).
    pub fn with_allow_llm_overlay(mut self, allow: bool) -> Self {
        self.allow_llm_overlay = allow;
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
        identity: Option<String>,
        activate: bool,
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

        // Apply agent identity if provided
        session.identity = identity;

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

        // Set as active session only when the caller opts in.
        //
        // `resume_session` passes `activate: false` so resuming an arbitrary
        // checkpoint never hijacks the caller's current active live session
        // pointer. `new_session` / `create_session` (fresh-start paths) pass
        // `activate: true` to keep the existing "new session becomes active"
        // behavior. This keeps the per-user active pointer under UI control
        // rather than letting any authenticated client overwrite it.
        if activate {
            *user_sessions.active_session_id.lock().await = session_id.clone();
        }

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

    /// Get a session by EITHER its live MSU registry key OR its
    /// checkpoint/session identity (`session.session_id`).
    ///
    /// The web frontend tracks `currentSessionId` from the `ResumeInfo` WS
    /// message, which carries the auto-derived checkpoint id
    /// (`session_YYYY_MM_DD_HH_MM_uuid8`) — NOT the live MSU registry key
    /// (`"default"` or the key from `create_session()`). External clients
    /// like Torpi/THQ pass the live registry key. This resolver accepts both:
    ///
    /// 1. Try registry-key lookup first (precise, used by Torpi/THQ).
    /// 2. Fall back to scanning the user's live sessions for one whose
    ///    `session.session_id` matches the requested id (used by the
    ///    embedded web UI after a command or resume).
    ///
    /// Returns `(live_registry_key, session_arc, ws_tx)`, or `None` if not
    /// found. The live key is returned so callers that need to set it as
    /// active (or otherwise reference the registry) use the real key.
    pub async fn get_session_by_any_id(
        &self,
        user_key: &str,
        id: &str,
    ) -> Option<(String, Arc<Mutex<Session>>, broadcast::Sender<String>)> {
        // Fast path: registry key match.
        let user_sessions = self.sessions.get(user_key)?;
        if let Some(entry) = user_sessions.sessions.get(id) {
            // Update last_active
            let now = chrono::Utc::now();
            *entry.last_active.lock().await = now;
            return Some((id.to_string(), entry.session.clone(), entry.ws_tx.clone()));
        }

        // Slow path: scan live sessions for a matching session.session_id.
        for entry in user_sessions.sessions.iter() {
            let session = entry.session.lock().await;
            if session.session_id.as_deref() == Some(id) {
                let key = entry.key().clone();
                let ws_tx = entry.ws_tx.clone();
                drop(session);
                // Update last_active
                let now = chrono::Utc::now();
                *entry.last_active.lock().await = now;
                return Some((key, entry.session.clone(), ws_tx));
            }
        }

        None
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
                handoff_count: session.handoff_count,
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
            .create_session(user_key, None, None, true)
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
        let hash = trustee_core::user_hash(user_key);
        dirs::home_dir().map(|home| home.join(".trustee").join("users").join(&hash))
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
    ///
    /// Hashing goes through the single consolidated [`trustee_core::user_hash`]
    /// so the web path and the CLI path can never drift apart.
    fn apply_user_isolation(&self, session: &mut Session, user_key: &str) {
        let user_hash = trustee_core::user_hash(user_key);

        // Set per-user home directory for checkpoint isolation
        let user_home = if let Some(home) = dirs::home_dir() {
            let user_home = home.join(".trustee").join("users").join(&user_hash);
            session.home_dir = Some(user_home.clone());
            Some(user_home)
        } else {
            None
        };

        session.project_id = Some(format!("web{}", &user_hash[..16]));

        // ── Per-user .env (Task 2) ──────────────────────────────────────
        //
        // Load per-user secrets from ~/.trustee/users/{hash}/.env
        // These are merged on top of shared secrets (per-user wins).
        // They are NEVER set as process env vars — used only for ${VAR}
        // substitution in the config TOML below.
        let shared_secrets = session.secrets.clone().unwrap_or_default();
        let mut merged_secrets = shared_secrets.clone();

        if let Some(ref user_home) = user_home {
            if let Ok(merged) = self.load_user_secrets(user_home, &merged_secrets) {
                merged_secrets = merged;
            }
        }

        // ── Per-user config overlay (Task 3) ────────────────────────────
        if let Some(ref user_home) = user_home {
            if let Some(merged) = self.merge_user_config(user_home, session.config_toml.as_deref()) {
                session.config_toml = Some(merged);
                tracing::debug!("Merged per-user config into session");
            }
        }

        // ── ${VAR} substitution (Task 4) ────────────────────────────────
        if let Some(ref mut config_toml) = session.config_toml {
            substitute_env_vars(config_toml, &merged_secrets);
        }

        // ── Strip per-user secrets (Task 5) ────────────────────────────
        session.secrets = Some(shared_secrets);
    }

    /// Load secrets from a user's ~/.trustee/users/{hash}/.env and merge
    /// on top of `base` (user wins). Returns the merged map.
    fn load_user_secrets(
        &self,
        user_home: &std::path::Path,
        base: &std::collections::HashMap<String, String>,
    ) -> std::io::Result<std::collections::HashMap<String, String>> {
        let user_env_path = user_home.join(".env");
        if !user_env_path.exists() {
            return Ok(base.clone());
        }
        let content = std::fs::read_to_string(&user_env_path)?;
        let mut merged = base.clone();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                merged.insert(key, value);
            }
        }
        tracing::debug!("Loaded per-user secrets from {}", user_env_path.display());
        Ok(merged)
    }

    /// Deep-merge a user's per-user config overlay on top of the shared
    /// config. Returns the merged TOML string, or None if no overlay exists.
    ///
    /// Overlay allowlist (task 16B): only allowlisted top-level sections of
    /// the user overlay are merged; anything else is dropped loudly.
    /// - `[mcp]` is always allowed — per-user MCP tool sets are the point of
    ///   the overlay convention.
    /// - `[llm]` is allowed only when the instance opted in via the
    ///   `[users].allow_llm_overlay` knob (default **false**).
    ///
    /// Everything else (`[server]`, `[auth]`, `[storage]`, `[web]`, …) is
    /// boot-time, instance-level config and must not be rewritable through a
    /// per-user file. This is predictability hardening, not a security
    /// boundary: those sections were never re-read from session config.
    fn merge_user_config(
        &self,
        user_home: &std::path::Path,
        shared_config: Option<&str>,
    ) -> Option<String> {
        let user_config_path = user_home.join("config").join("trustee.toml");
        if !user_config_path.exists() {
            return None;
        }
        let user_config_toml = std::fs::read_to_string(&user_config_path).ok()?;
        let shared = shared_config
            .unwrap_or("")
            .parse::<toml::Value>()
            .ok()?;
        let overlay = user_config_toml.parse::<toml::Value>().ok()?;

        let allowed =
            |section: &str| section == "mcp" || (self.allow_llm_overlay && section == "llm");
        // Mask the user in logs: user_home's dir name IS the hash — never
        // log the raw key (keys may be emails). Log a short hash prefix.
        let dir_name = user_home
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>");
        let masked_user = dir_name.get(..8).unwrap_or(dir_name);

        let mut filtered_overlay = toml::map::Map::new();
        if let Some(table) = overlay.as_table() {
            for (section, value) in table {
                if allowed(section) {
                    filtered_overlay.insert(section.clone(), value.clone());
                } else {
                    tracing::warn!(
                        "user config overlay: dropping non-allowlisted section [{}] for user {}",
                        section,
                        masked_user
                    );
                }
            }
        }

        if filtered_overlay.is_empty() {
            // Nothing survived the allowlist — no-op, keep shared config as-is.
            return None;
        }
        let overlay = toml::Value::Table(filtered_overlay);

        let mut shared = shared;
        deep_merge_toml(&mut shared, &overlay);
        let merged = toml::to_string(&shared).ok()?;
        tracing::debug!("Merged per-user config from {}", user_config_path.display());
        Some(merged)
    }

    /// Resolve the fully-merged config TOML for a user WITHOUT creating a session.
    ///
    /// This is the read-only equivalent of the config resolution in
    /// `apply_user_isolation`: shared config + per-user overlay + ${VAR}
    /// substitution. Used by endpoints that need to inspect config (e.g.
    /// listing available LLM models) without spawning a ghost session.
    ///
    /// Returns None if no shared config is loaded.
    pub fn resolve_user_config(&self, user_key: &str) -> Option<String> {
        let config_toml = self.config_toml.clone()?;

        // Resolve user home dir (same hash scheme as apply_user_isolation)
        let user_home = self.get_user_home_dir(user_key)?;

        // Start from shared secrets; merge per-user .env on top
        let mut merged_secrets = self.secrets.clone().unwrap_or_default();
        if let Ok(merged) = self.load_user_secrets(&user_home, &merged_secrets) {
            merged_secrets = merged;
        }

        // Merge per-user config overlay
        let mut resolved = config_toml;
        if let Some(merged) = self.merge_user_config(&user_home, Some(&resolved)) {
            resolved = merged;
        }

        // Substitute ${VAR} from merged secrets
        substitute_env_vars(&mut resolved, &merged_secrets);

        Some(resolved)
    }

    /// Get or build the per-user MCP tool loader (16C).
    ///
    /// Cache semantics:
    /// - fingerprint = content hash of the effective `[mcp]` section
    ///   (shared + allowlist-filtered overlay + ${VAR} substitution, i.e.
    ///   exactly what `resolve_user_config` produces) — stale on ANY change.
    /// - hit + match → `Arc` clone, zero network I/O.
    /// - miss/stale → single-flight build (one build per user at a time;
    ///   late arrivals re-check and reuse the winner's entry).
    /// - `Ok(None)` = MCP disabled for this user → agent runs MCP-less via
    ///   abk `McpSource::Prebuilt(None)` (same semantics as
    ///   `[mcp] enabled = false`, no per-task re-evaluation).
    /// - build failure → cached degraded entry with
    ///   [`MCP_BUILD_RETRY_BACKOFF`]; the error is returned so THIS user's
    ///   dispatch fails loud while other users are unaffected.
    pub async fn get_or_build_mcp_loader(
        &self,
        user_key: &str,
        token_store: &Arc<pep::MemoryTokenStore>,
    ) -> Result<Option<std::sync::Arc<abk::agent::McpToolLoader>>, String> {
        let user_hash = trustee_core::user_hash(user_key);

        // Effective per-user config — the fingerprint source of truth.
        let resolved = self.resolve_user_config(user_key);
        let fingerprint = fingerprint_mcp_section(resolved.as_deref());

        // Fast path: fresh, non-degraded entry.
        if let Some(entry) = self.mcp_loaders.get(&user_hash) {
            if entry.degraded.is_none() {
                if entry.fingerprint == fingerprint {
                    return Ok(entry.loader.clone());
                }
            } else if let (Some(err), Some(failed_at)) = (&entry.degraded, entry.failed_at) {
                let backoff = chrono::Duration::from_std(MCP_BUILD_RETRY_BACKOFF)
                    .unwrap_or_else(|_| chrono::Duration::seconds(30));
                if chrono::Utc::now() < failed_at + backoff {
                    return Err(err.clone());
                }
            }
        }

        // Single-flight: one build per user at a time.
        let lock = self
            .mcp_build_locks
            .entry(user_hash.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        // Double-check: another task may have built while we waited.
        if let Some(entry) = self.mcp_loaders.get(&user_hash) {
            if entry.degraded.is_none() && entry.fingerprint == fingerprint {
                return Ok(entry.loader.clone());
            }
        }

        match self
            .build_mcp_loader(&user_hash, resolved.as_deref(), fingerprint, token_store)
            .await
        {
            Ok(entry) => {
                self.mcp_loaders.insert(user_hash.clone(), entry);
                Ok(self.mcp_loaders.get(&user_hash).unwrap().loader.clone())
            }
            Err(err) => {
                tracing::warn!(
                    "MCP loader build FAILED for user {}; dispatch fails loud, retry after {:?}",
                    &user_hash[..8.min(user_hash.len())],
                    MCP_BUILD_RETRY_BACKOFF
                );
                self.mcp_loaders.insert(
                    user_hash,
                    McpLoaderEntry {
                        loader: None,
                        fingerprint,
                        built_at: chrono::Utc::now(),
                        degraded: Some(err.clone()),
                        failed_at: Some(chrono::Utc::now()),
                    },
                );
                Err(err)
            }
        }
    }

    /// Build a loader entry from the effective config. No caching here —
    /// the caller owns insertion and degraded handling.
    async fn build_mcp_loader(
        &self,
        user_hash: &str,
        resolved: Option<&str>,
        fingerprint: u64,
        token_store: &Arc<pep::MemoryTokenStore>,
    ) -> Result<McpLoaderEntry, String> {
        let mcp_config: Option<abk::config::McpConfig> = match resolved {
            Some(toml_str) => {
                let value = toml_str
                    .parse::<toml::Value>()
                    .map_err(|e| format!("config parse failed: {}", e))?;
                match value.get("mcp") {
                    Some(section) => {
                        use serde::Deserialize as _;
                        Some(
                            abk::config::McpConfig::deserialize(section.clone())
                                .map_err(|e| format!("invalid [mcp] config: {}", e))?,
                        )
                    }
                    None => None,
                }
            }
            None => None,
        };

        let loader = match mcp_config {
            Some(cfg) if cfg.enabled => {
                let built = abk::agent::McpToolLoader::with_token_store(
                    &cfg,
                    Some(token_store.clone() as std::sync::Arc<dyn pep::token_store::TokenStore>),
                )
                .await
                .map_err(|e| format!("MCP loader build failed: {}", e))?;

                // THE parity-evidence log line (16C acceptance + migration task):
                // one INFO line per build; a task loop that reuses the cache
                // shows exactly one line per user per fingerprint.
                let servers: Vec<String> = built
                    .server_statuses
                    .iter()
                    .map(|s| {
                        if s.connected {
                            format!("{}(up,{}tools)", s.name, s.tool_count)
                        } else {
                            format!("{}(DOWN)", s.name)
                        }
                    })
                    .collect();
                tracing::info!(
                    "MCP loader built for user {}: servers=[{}] total_tools={}",
                    &user_hash[..8.min(user_hash.len())],
                    servers.join(", "),
                    built.tool_count
                );
                Some(std::sync::Arc::new(built))
            }
            _ => None,
        };

        Ok(McpLoaderEntry {
            loader,
            fingerprint,
            built_at: chrono::Utc::now(),
            degraded: None,
            failed_at: None,
        })
    }

    /// Spawn a background drain task for a specific session's workflow receiver.
    fn spawn_user_drain_task(
        &self,
        session_id: String,
        session: Arc<Mutex<Session>>,
        ws_tx: broadcast::Sender<String>,
        mut workflow_rx: mpsc::UnboundedReceiver<TuiMessage>,
    ) {
        // Bug 6: broadcast StateChanged only on actual transitions. Without
        // this, every WS message was accompanied by a duplicate StateChanged,
        // doubling traffic and re-triggering frontend updateState on hot
        // StreamDelta/ReasoningDelta bursts. New subscribers learn the current
        // state from the WS snapshot, so the initial None is safe.
        let mut last_broadcast_state: Option<String> = None;
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
                    if last_broadcast_state.as_deref() != Some(state_str) {
                        last_broadcast_state = Some(state_str.to_string());
                        let state_msg = serde_json::json!({
                            "type": "StateChanged",
                            "state": state_str
                        });
                        let _ = ws_tx.send(state_msg.to_string());
                    }
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

        // Bug 6: transition-only StateChanged broadcasts (see
        // spawn_user_drain_task). The task starts in Running state because
        // spawn_drain_task is only called after a command began executing.
        let mut last_broadcast_state = Some("Running".to_string());
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
                    if last_broadcast_state.as_deref() != Some(state_str) {
                        last_broadcast_state = Some(state_str.to_string());
                        let state_msg = serde_json::json!({
                            "type": "StateChanged",
                            "state": state_str
                        });
                        let _ = ws_tx.send(state_msg.to_string());
                    }
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
            TuiMessage::HandoffReady(briefing) => {
                let mut s = serializer.serialize_struct("msg", 3)?;
                s.serialize_field("type", "HandoffReady")?;
                s.serialize_field("state", "Idle")?;
                s.serialize_field("briefing", briefing)?;
                s.end()
            }
            TuiMessage::SessionRotated { old, new } => {
                let mut s = serializer.serialize_struct("msg", 3)?;
                s.serialize_field("type", "SessionRotated")?;
                s.serialize_field("old", old)?;
                s.serialize_field("new", new)?;
                s.end()
            }
            TuiMessage::HandoffFailed => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "HandoffFailed")?;
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
            TuiMessage::SessionTitleUpdated(title) => {
                let mut s = serializer.serialize_struct("msg", 2)?;
                s.serialize_field("type", "SessionTitleUpdated")?;
                s.serialize_field("title", title)?;
                s.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-user config helpers
// ---------------------------------------------------------------------------

/// Deep-merge a TOML overlay on top of a base value (in-place).
///
/// - Tables: recursively merge key-by-key (overlay wins on conflict).
/// - Arrays: overlay replaces base entirely (no merging).
/// - Scalars: overlay replaces base.
/// - If a key exists in overlay but not base, it's added.
fn deep_merge_toml(base: &mut toml::Value, overlay: &toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_val) in overlay_table {
                match base_table.get_mut(key) {
                    Some(base_val) => {
                        // Both exist — recurse if both are tables, else replace
                        deep_merge_toml(base_val, overlay_val);
                    }
                    None => {
                        // Key only in overlay — insert
                        base_table.insert(key.clone(), overlay_val.clone());
                    }
                }
            }
        }
        // Non-table: overlay replaces base
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
}

/// Replace `${VAR_NAME}` references in a string with values from a secrets map.
///
/// Falls back to process environment if the variable is not in the map.
/// Variables not found in either are left as-is.
fn substitute_env_vars(s: &mut String, secrets: &std::collections::HashMap<String, String>) {
    // Simple state machine: scan for ${, read until }, replace.
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            // Find closing }
            if let Some(end) = s[i + 2..].find('}') {
                let var_name = &s[i + 2..i + 2 + end];
                // Look up in per-user secrets first, then process env
                if let Some(value) = secrets.get(var_name) {
                    result.push_str(value);
                } else if let Ok(value) = std::env::var(var_name) {
                    result.push_str(&value);
                } else {
                    // Not found — leave as-is
                    result.push_str(&s[i..i + 2 + end + 1]);
                }
                i = i + 2 + end + 1;
            } else {
                // No closing } — copy as-is
                result.push('$');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    *s = result;
}

// ---------------------------------------------------------------------------
// Tests (task 16B: overlay allowlist + consolidated user hash)
// ---------------------------------------------------------------------------

/// Fingerprint the effective `[mcp]` section of a resolved config (16C).
///
/// Content hash (SHA-256, first 8 bytes as u64) — never mtime: overlays are
/// rewritten in place. Configs without `[mcp]` fingerprint to a stable
/// constant, so the disabled state caches too.
fn fingerprint_mcp_section(resolved: Option<&str>) -> u64 {
    use sha2::{Digest, Sha256};
    let section = resolved
        .and_then(|s| s.parse::<toml::Value>().ok())
        .and_then(|v| v.get("mcp").cloned());
    let bytes = match section {
        Some(v) => v.to_string().into_bytes(),
        None => b"<no-mcp>".to_vec(),
    };
    let digest = Sha256::digest(&bytes);
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 digest >= 8 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default-knob ServerState for overlay tests.
    fn test_state() -> ServerState {
        let (session, _rx) = Session::new();
        let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(16);
        ServerState::new(session, ws_tx, None)
    }

    /// Unique temp dir with a `config/` subdir (std-only; no tempfile dep).
    fn temp_user_home(tag: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "trustee-state-test-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(dir.join("config")).expect("create temp user home");
        dir
    }

    fn parse(toml_str: &str) -> toml::Value {
        toml_str.parse::<toml::Value>().expect("valid test TOML")
    }

    /// (a) Overlay with [mcp] + [server] + [auth]: [mcp] applied (user wins),
    /// shared [server] preserved untouched, overlay [auth] dropped entirely.
    #[test]
    fn overlay_allowlist_drops_non_allowlisted_sections() {
        let state = test_state();
        let home = temp_user_home("allowlist");
        std::fs::write(
            home.join("config").join("trustee.toml"),
            "[server]\nport = 1\n\n[auth]\nmode = \"kanidm\"\n\n[mcp]\nmode = \"user\"\n",
        )
        .expect("write overlay");

        let shared = "[server]\nport = 8080\n\n[mcp]\nmode = \"shared\"\n";
        let merged = state
            .merge_user_config(&home, Some(shared))
            .expect("overlay has allowlisted content");

        let merged_val = parse(&merged);
        let expected = parse("[server]\nport = 8080\n\n[mcp]\nmode = \"user\"\n");
        assert_eq!(merged_val, expected, "merged config must be shared + [mcp] overlay only");
        assert!(merged_val.get("auth").is_none(), "overlay [auth] must be dropped");
    }

    /// (b) No overlay file → merge is a no-op (None), shared config untouched.
    #[test]
    fn no_overlay_file_returns_none() {
        let state = test_state();
        let home = temp_user_home("empty");
        assert!(state
            .merge_user_config(&home, Some("[server]\nport = 8080\n"))
            .is_none());
    }

    /// Overlay whose sections are ALL non-allowlisted → no-op (None).
    #[test]
    fn overlay_with_no_allowlisted_sections_is_noop() {
        let state = test_state();
        let home = temp_user_home("all-dropped");
        std::fs::write(
            home.join("config").join("trustee.toml"),
            "[server]\nport = 1\n\n[storage]\npath = \"/tmp/x\"\n",
        )
        .expect("write overlay");
        assert!(state
            .merge_user_config(&home, Some("[server]\nport = 8080\n"))
            .is_none());
    }

    /// (c) [llm] dropped when allow_llm_overlay=false (default), applied when
    /// the instance opted in via with_allow_llm_overlay(true).
    #[test]
    fn llm_overlay_dropped_by_default_and_kept_when_enabled() {
        let shared = "[llm]\nprovider = \"openai\"\n\n[mcp]\nmode = \"shared\"\n";
        let overlay = "[llm]\nprovider = \"anthropic\"\n";

        // Default: knob false → [llm]-only overlay is a no-op.
        let state = test_state();
        assert!(!state.allow_llm_overlay);
        let home = temp_user_home("llm-off");
        std::fs::write(home.join("config").join("trustee.toml"), overlay).expect("write overlay");
        assert!(state.merge_user_config(&home, Some(shared)).is_none());

        // Opted in: [llm] kept, user value wins; shared [mcp] untouched.
        let state = test_state().with_allow_llm_overlay(true);
        assert!(state.allow_llm_overlay);
        let home = temp_user_home("llm-on");
        std::fs::write(home.join("config").join("trustee.toml"), overlay).expect("write overlay");
        let merged = state
            .merge_user_config(&home, Some(shared))
            .expect("[llm] overlay applies when opted in");
        let merged_val = parse(&merged);
        assert_eq!(merged_val["llm"]["provider"].as_str(), Some("anthropic"));
        assert_eq!(merged_val["mcp"]["mode"].as_str(), Some("shared"));
    }

    /// (d) Web-path home dir resolves through the consolidated
    /// trustee_core::user_hash (determinism/known-vectors covered in
    /// trustee-core's own tests).
    #[test]
    fn user_home_dir_uses_consolidated_hash() {
        let state = test_state();
        if let Some(home) = state.get_user_home_dir("farzan@example.com") {
            assert_eq!(
                home.file_name().and_then(|n| n.to_str()),
                Some(trustee_core::user_hash("farzan@example.com")).as_deref()
            );
            let users_root = dirs::home_dir().unwrap().join(".trustee").join("users");
            assert_eq!(home.parent(), Some(&users_root).map(|p| p.as_path()));
        }
        // dirs::home_dir() unavailable in sandbox → covered by core tests.
    }

    /// 16D integration guard: dev-agent principals (user_key `agent-<name>`)
    /// get their OWN session bucket and home-dir namespace — one agent can
    /// never see another's sessions, and both go through the same
    /// user_hash isolation path as humans.
    #[tokio::test]
    async fn agent_principals_get_isolated_session_buckets() {
        let state = test_state();
        let key_a = "agent-farzan";
        let key_b = "agent-paydar";

        let (sid_a, session_a, _tx_a, _ts_a) = state.ensure_active_session(key_a).await;
        let (_sid_b, _session_b, _tx_b, _ts_b) = state.ensure_active_session(key_b).await;

        // Each agent resolves its own session; B cannot see A's by id.
        assert!(
            state.get_session_by_any_id(key_a, &sid_a).await.is_some(),
            "owner bucket resolves its own session"
        );
        assert!(
            state.get_session_by_any_id(key_b, &sid_a).await.is_none(),
            "cross-agent session access must be 404/None"
        );
        assert!(Arc::strong_count(&session_a) >= 1);

        // Same isolation path as humans: home dir = users_root/user_hash(key).
        if let Some(home) = state.get_user_home_dir(key_a) {
            assert_eq!(
                home.file_name().and_then(|n| n.to_str()),
                Some(trustee_core::user_hash(key_a)).as_deref()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 16C — per-user McpToolLoader cache
// ---------------------------------------------------------------------------

#[cfg(test)]
mod mcp_loader_cache_tests {
    use super::*;

    fn state_with_shared(shared: &str) -> ServerState {
        let (session, _rx) = Session::new();
        let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(16);
        let mut state = ServerState::new(session, ws_tx, None);
        state.config_toml = Some(shared.to_string());
        state
    }

    /// Deterministic throwaway user: real user-home path under
    /// ~/.trustee/users/{hash} (that IS the resolution path under test),
    /// with config/ subdir; caller cleans up.
    struct TempUser {
        key: String,
        home: std::path::PathBuf,
    }

    impl TempUser {
        fn new(tag: &str) -> Self {
            let key = format!("16c-{tag}-{}@test.invalid", std::process::id());
            let home = dirs::home_dir()
                .expect("HOME available in test env")
                .join(".trustee")
                .join("users")
                .join(trustee_core::user_hash(&key));
            std::fs::create_dir_all(home.join("config")).expect("create user home");
            Self { key, home }
        }

        fn write_overlay(&self, toml_str: &str) {
            std::fs::write(self.home.join("config").join("trustee.toml"), toml_str)
                .expect("write overlay");
        }
    }

    impl Drop for TempUser {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    #[tokio::test]
    async fn no_mcp_config_caches_disabled_marker() {
        let state = state_with_shared("[server]\nport = 8080\n");
        let user = TempUser::new("nomcp");
        let ts = Arc::new(pep::MemoryTokenStore::new());

        let first = state.get_or_build_mcp_loader(&user.key, &ts).await.unwrap();
        assert!(first.is_none(), "no [mcp] anywhere → disabled marker");
        assert_eq!(state.mcp_loaders.len(), 1, "exactly one cache entry");

        // Second call is a fingerprint hit — same disabled marker, still one entry.
        let second = state.get_or_build_mcp_loader(&user.key, &ts).await.unwrap();
        assert!(second.is_none());
        assert_eq!(state.mcp_loaders.len(), 1);
    }

    #[tokio::test]
    async fn concurrent_cold_builds_single_flight() {
        let state = Arc::new(state_with_shared("[server]\nport = 8080\n"));
        let user = TempUser::new("singleflight");
        let ts = Arc::new(pep::MemoryTokenStore::new());

        let mut handles = Vec::new();
        for _ in 0..5 {
            let state = state.clone();
            let key = user.key.clone();
            let ts = ts.clone();
            handles.push(tokio::spawn(async move {
                state.get_or_build_mcp_loader(&key, &ts).await
            }));
        }
        for h in handles {
            h.await.unwrap().expect("all five succeed");
        }
        assert_eq!(state.mcp_loaders.len(), 1, "single-flight → one entry");
    }

    #[tokio::test]
    async fn fingerprint_change_triggers_rebuild() {
        let state = state_with_shared("[server]\nport = 8080\n");
        let user = TempUser::new("fpchange");
        let ts = Arc::new(pep::MemoryTokenStore::new());

        // v1: an enabled MCP server pointing at a blackhole port — abk keeps
        // the loader with a DOWN status (connect refused is fast on loopback).
        user.write_overlay("[mcp]\nenabled = true\n\n[[mcp.servers]]\nname = \"v1\"\nurl = \"http://127.0.0.1:9/sse\"\n");
        let v1 = state.get_or_build_mcp_loader(&user.key, &ts).await.unwrap();
        assert!(v1.is_some(), "enabled [mcp] → real loader");
        let fp1 = state
            .mcp_loaders
            .get(&trustee_core::user_hash(&user.key))
            .unwrap()
            .fingerprint;

        // v2: different server URL → different content fingerprint → rebuild.
        std::thread::sleep(std::time::Duration::from_millis(5));
        user.write_overlay("[mcp]\nenabled = true\n\n[[mcp.servers]]\nname = \"v2\"\nurl = \"http://127.0.0.1:9/other\"\n");
        let v2 = state.get_or_build_mcp_loader(&user.key, &ts).await.unwrap();
        assert!(v2.is_some());
        let entry = state
            .mcp_loaders
            .get(&trustee_core::user_hash(&user.key))
            .unwrap();
        assert_ne!(
            entry.fingerprint, fp1,
            "fingerprint must change with content"
        );
        assert!(entry.degraded.is_none());

        // The old Arc stays valid for in-flight sessions (no panic, no revoke).
        let _still_usable = v1.as_ref().unwrap().tool_count;
    }

    #[tokio::test]
    async fn degraded_entry_fails_loud_within_backoff_and_isolates_users() {
        let state = state_with_shared("[server]\nport = 8080\n");
        let bad = TempUser::new("degraded-bad");
        let good = TempUser::new("degraded-good");
        let ts = Arc::new(pep::MemoryTokenStore::new());

        // [mcp] present but not a table → McpConfig deserialize fails → Err.
        bad.write_overlay("[mcp]\nenabled = \"not-a-bool\"\n");
        let err = match state.get_or_build_mcp_loader(&bad.key, &ts).await {
            Ok(_) => panic!("invalid [mcp] must fail loud"),
            Err(e) => e,
        };
        assert!(
            err.contains("invalid [mcp]"),
            "surfaces the parse error: {err}"
        );

        let entry = state
            .mcp_loaders
            .get(&trustee_core::user_hash(&bad.key))
            .unwrap();
        assert!(entry.degraded.is_some(), "poison entry recorded");

        // Within backoff: fast-fail again (cached error).
        let err2 = match state.get_or_build_mcp_loader(&bad.key, &ts).await {
            Ok(_) => panic!("still within backoff"),
            Err(e) => e,
        };
        assert_eq!(err, err2, "same cached error");

        // Other users are completely unaffected.
        let good_loader = state
            .get_or_build_mcp_loader(&good.key, &ts)
            .await
            .expect("other user unaffected");
        assert!(
            good_loader.is_none(),
            "good user has no [mcp] → disabled marker"
        );
    }
}

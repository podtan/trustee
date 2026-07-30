//! Axum route handlers for the Trustee API.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::{IntoResponse, Json, Response},
    http::{header, StatusCode},
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::ServerState;
use crate::state::SessionError;

/// Attach a `Set-Cookie` header to a response if the cookie value is present.
fn with_rolling_cookie(mut response: Response, cookie: Option<String>) -> Response {
    if let Some(cookie_str) = cookie {
        if let Ok(value) = cookie_str.parse() {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub workflow_state: String,
    pub output_lines: Vec<String>,
    pub todo_lines: Vec<String>,
    pub mcp_servers: Vec<McpServerJson>,
    pub context_tokens: usize,
    pub input: String,
    pub resume_info_present: bool,
    pub session_name: Option<String>,
    pub project_name: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct McpServerJson {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub accepted: bool,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// Session discovery DTOs (checkpoint-based, legacy)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<trustee_core::sessions::SessionSummary>,
}

#[derive(Debug, Serialize)]
pub struct SessionDetailResponse {
    pub session: trustee_core::sessions::SessionSummary,
    pub checkpoints: Vec<trustee_core::sessions::CheckpointSummary>,
}

#[derive(Debug, Serialize)]
pub struct ResumeResponse {
    pub accepted: bool,
    pub session_id: String,
    pub checkpoint_id: String,
    pub iteration: u32,
}

#[derive(Debug, Deserialize)]
pub struct ResumeRequestBody {
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionHistoryResponse {
    pub session_id: String,
    pub checkpoint_id: String,
    pub task_description: String,
    pub iteration: u32,
    pub total_messages: usize,
    pub messages: Vec<trustee_core::sessions::HistoryMessage>,
}

// ---------------------------------------------------------------------------
// MSU new-session DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default)]
    pub resume_from: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub accepted: bool,
}

// ---------------------------------------------------------------------------
// Naming DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SetNameRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct NewSessionRequest {
    pub session_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetNameResponse {
    pub accepted: bool,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct NewSessionResponse {
    pub accepted: bool,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build a SessionResponse from a locked session.
fn session_to_response(session: &trustee_core::session::Session) -> SessionResponse {
    let workflow_state = match session.workflow_state {
        trustee_core::types::WorkflowState::Idle => "Idle",
        trustee_core::types::WorkflowState::Running => "Running",
        trustee_core::types::WorkflowState::Cancelling => "Cancelling",
    };

    let mcp_servers = session
        .mcp_servers
        .iter()
        .map(|s| McpServerJson {
            name: s.name.clone(),
            connected: s.status == trustee_core::types::McpServerStatus::Connected,
            tool_count: s.tool_count,
            error: s.error.clone(),
        })
        .collect();

    SessionResponse {
        workflow_state: workflow_state.to_string(),
        output_lines: session.output_lines.clone(),
        todo_lines: session.todo_lines.clone(),
        mcp_servers,
        context_tokens: session.current_context_tokens,
        input: session.input.clone(),
        resume_info_present: session.resume_info.is_some(),
        session_name: session.session_name.clone(),
        project_name: session.project_name.clone(),
        session_id: session.session_id.clone(),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/health
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /api/v1/session — return active session state (legacy).
pub async fn get_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
    let session = session_arc.lock().await;
    let resp = Json(session_to_response(&session));
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/command ��� submit a command to the active session (legacy).
pub async fn post_command(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CommandRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, ws_tx, token_store) = state.ensure_active_session(&user_key).await;

    execute_command_inner(&state, &headers, &session_arc, &ws_tx, &token_store, req).await?;

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/cancel — cancel the running workflow on active session (legacy).
pub async fn post_cancel(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, ws_tx, _token_store) = state.ensure_active_session(&user_key).await;

    let cancelled = {
        let session = session_arc.lock().await;
        let c = session.workflow_state == trustee_core::types::WorkflowState::Running;
        if c {
            session.cancel_token.cancel();
        }
        c
    };

    if cancelled {
        let state_msg = serde_json::json!({"type": "StateChanged", "state": "Cancelling"});
        let _ = ws_tx.send(state_msg.to_string());
    }

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/handoff — trigger handoff on active session (legacy).
pub async fn post_handoff(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
    let mut session = session_arc.lock().await;
    session.trigger_handoff(String::new());

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/session/stream — WebSocket for active session (legacy).
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let _cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, session_arc, ws_tx)))
}

async fn handle_ws(
    socket: WebSocket,
    session_arc: std::sync::Arc<tokio::sync::Mutex<trustee_core::session::Session>>,
    ws_tx: broadcast::Sender<String>,
) {
    use futures::{SinkExt, StreamExt};
    let (mut sender, mut receiver) = socket.split();
    let mut ws_rx = ws_tx.subscribe();

    // Send current session state as the first message
    {
        let session = session_arc.lock().await;
        let snapshot = session_to_response(&session);
        if let Ok(json) = serde_json::to_string(&snapshot) {
            let _ = sender.send(Message::Text(json.into())).await;
        }
    }

    loop {
        tokio::select! {
            msg = ws_rx.recv() => {
                match msg {
                    Ok(text) => {
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let warn = serde_json::json!({"type":"Warning","message":format!("Lagged {} messages", n)});
                        let _ = sender.send(Message::Text(warn.to_string().into())).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session discovery handlers (checkpoint-based — existing, migrated)
// ---------------------------------------------------------------------------

/// GET /api/v1/sessions — list all checkpoint sessions available for resume.
pub async fn list_sessions(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let (config_toml, home_dir) = {
        let user_key = state.resolve_user_key(&headers).await;
        let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        let config = match &session.config_toml {
            Some(c) => c.clone(),
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration not loaded".to_string(),
                ))
            }
        };
        (config, session.home_dir.clone())
    };

    let sessions = trustee_core::sessions::list_all_sessions(&config_toml, home_dir.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let resp = Json(SessionListResponse { sessions });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/sessions/{id} — get checkpoint session detail.
pub async fn get_session_detail(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let (config_toml, home_dir) = {
        let user_key = state.resolve_user_key(&headers).await;
        let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        match &session.config_toml {
            Some(c) => (c.clone(), session.home_dir.clone()),
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration not loaded".to_string(),
                ))
            }
        }
    };

    let detail = trustee_core::sessions::get_session_detail(&config_toml, &session_id, home_dir.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match detail {
        Some((session, checkpoints)) => {
            let resp = Json(SessionDetailResponse { session, checkpoints });
            Ok(with_rolling_cookie(resp.into_response(), cookie))
        }
        None => Err((StatusCode::NOT_FOUND, "Session not found".to_string())),
    }
}

/// POST /api/v1/sessions/{id}/resume — resume from checkpoint (legacy path).
pub async fn resume_session(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
    _body: Option<Json<ResumeRequestBody>>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let (config_toml, home_dir) = {
        let user_key = state.resolve_user_key(&headers).await;
        let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        if session.workflow_state != trustee_core::types::WorkflowState::Idle {
            return Err((
                StatusCode::CONFLICT,
                "Workflow is running or cancelling".to_string(),
            ));
        }
        match &session.config_toml {
            Some(c) => (c.clone(), session.home_dir.clone()),
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration not loaded".to_string(),
                ))
            }
        }
    };

    let resume_info = trustee_core::sessions::create_resume_info(&config_toml, &session_id, home_dir.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let resume_info = match resume_info {
        Some(info) => info,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                "Session or checkpoint not found".to_string(),
            ))
        }
    };

    let checkpoint_id = resume_info.checkpoint_id.clone();
    let iteration = resume_info.iteration;

    {
        let user_key = state.resolve_user_key(&headers).await;
        let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
        let mut session = session_arc.lock().await;
        session.resume_info = Some(resume_info);
        session.output_lines.clear();
    }

    let msg = serde_json::json!({
        "type": "SessionResumed",
        "session_id": session_id,
        "checkpoint_id": checkpoint_id,
    });
    let user_key = state.resolve_user_key(&headers).await;
    let (_, _sid, ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
    let _ = ws_tx.send(msg.to_string());

    let resp = Json(ResumeResponse {
        accepted: true,
        session_id: session_id.clone(),
        checkpoint_id,
        iteration,
    });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/sessions/{id}/history — load conversation history from checkpoint.
pub async fn get_session_history(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let home_dir = {
        let user_key = state.resolve_user_key(&headers).await;
        let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        session.home_dir.clone()
    };

    let history = trustee_core::sessions::load_session_history(&session_id, home_dir.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match history {
        Some(h) => {
            let resp = Json(SessionHistoryResponse {
                session_id: h.session_id,
                checkpoint_id: h.checkpoint_id,
                task_description: h.task_description,
                iteration: h.iteration,
                total_messages: h.total_messages,
                messages: h.messages,
            });
            Ok(with_rolling_cookie(resp.into_response(), cookie))
        }
        None => Err((StatusCode::NOT_FOUND, "Session not found".to_string())),
    }
}

// ---------------------------------------------------------------------------
// Session/project naming handlers (legacy, migrated)
// ---------------------------------------------------------------------------

/// POST /api/v1/session/name — set the display name for the active session.
pub async fn set_session_name(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetNameRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;

    {
        let mut session = session_arc.lock().await;
        session.session_name = Some(req.name.clone());
    }

    let resp = Json(SetNameResponse { accepted: true, name: req.name });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/project/name — set the display name for the current project.
pub async fn set_project_name(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetNameRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;
    let (_sid, session_arc, _ws_tx, _token_store) = state.ensure_active_session(&user_key).await;

    {
        let mut session = session_arc.lock().await;
        session.project_name = Some(req.name.clone());
    }

    let resp = Json(SetNameResponse { accepted: true, name: req.name });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/new — start a fresh session (legacy).
///
/// Creates a new MSU session via create_session() and makes it active.
pub async fn new_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<NewSessionRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let session_id = state
        .create_session(&user_key, req.session_name)
        .await
        .map_err(|e| match e {
            SessionError::MaxSessionsReached(n) => (
                StatusCode::TOO_MANY_REQUESTS,
                format!("Max {} sessions per user", n),
            ),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    // Broadcast so WebSocket clients know to reset their view
    let (_, _ws_tx, ws_tx, _token_store) = state.ensure_active_session(&user_key).await;
    let msg = serde_json::json!({ "type": "NewSession", "session_id": session_id });
    let _ = ws_tx.send(msg.to_string());

    let resp = Json(NewSessionResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

// ---------------------------------------------------------------------------
// MSU: New session-scoped route handlers (/api/v1/sessions/*)
// ---------------------------------------------------------------------------

/// POST /api/v1/sessions — create a new live session.
pub async fn create_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let session_id = state
        .create_session(&user_key, req.session_name)
        .await
        .map_err(|e| match e {
            SessionError::MaxSessionsReached(n) => (
                StatusCode::TOO_MANY_REQUESTS,
                format!("Max {} sessions per user", n),
            ),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    // If resume_from is provided, set resume_info on the new session
    if let Some(resume_from) = req.resume_from {
        if let Some((session_arc, _)) = state.get_session(&user_key, &session_id).await {
            let mut session = session_arc.lock().await;
            if let Some(ref config_toml) = session.config_toml.clone() {
                let home_dir = session.home_dir.as_deref();
                if let Ok(Some(info)) =
                    trustee_core::sessions::create_resume_info(config_toml, &resume_from, home_dir)
                        .await
                {
                    session.resume_info = Some(info);
                }
            }
        }
    }

    let resp = Json(CreateSessionResponse { session_id, accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/sessions/{id}/live — get live session state by ID.
///
/// NOTE: This uses /live suffix to avoid conflict with the existing
/// GET /api/v1/sessions/{id} checkpoint detail route.
pub async fn get_live_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let (session_arc, _ws_tx) = state
        .get_session(&user_key, &session_id)
        .await
        .ok_or((StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let session = session_arc.lock().await;
    let resp = Json(session_to_response(&session));
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/sessions/{id}/command — submit command to a specific session.
pub async fn post_command_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let (session_arc, ws_tx) = state
        .get_session(&user_key, &session_id)
        .await
        .ok_or((StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    // Get token_store from user's session collection
    let token_store = {
        let user_sessions = state.sessions.get(&user_key).unwrap();
        user_sessions.token_store.clone()
    };

    execute_command_inner(&state, &headers, &session_arc, &ws_tx, &token_store, req).await?;

    // Set this session as active (most recently used)
    state.set_active_session(&user_key, &session_id).await;

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/sessions/{id}/cancel — cancel workflow on a specific session.
pub async fn post_cancel_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let (session_arc, ws_tx) = state
        .get_session(&user_key, &session_id)
        .await
        .ok_or((StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let cancelled = {
        let session = session_arc.lock().await;
        let c = session.workflow_state == trustee_core::types::WorkflowState::Running;
        if c {
            session.cancel_token.cancel();
        }
        c
    };

    if cancelled {
        let state_msg = serde_json::json!({"type": "StateChanged", "state": "Cancelling"});
        let _ = ws_tx.send(state_msg.to_string());
    }

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/sessions/{id}/handoff — trigger handoff on a specific session.
pub async fn post_handoff_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let (session_arc, _ws_tx) = state
        .get_session(&user_key, &session_id)
        .await
        .ok_or((StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let mut session = session_arc.lock().await;
    session.trigger_handoff(String::new());

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/sessions/{id}/name — set name on a specific session.
pub async fn set_session_name_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
    Json(req): Json<SetNameRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    let (session_arc, _ws_tx) = state
        .get_session(&user_key, &session_id)
        .await
        .ok_or((StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    {
        let mut session = session_arc.lock().await;
        session.session_name = Some(req.name.clone());
    }

    let resp = Json(SetNameResponse { accepted: true, name: req.name });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// DELETE /api/v1/sessions/{id} — destroy a live session.
pub async fn destroy_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let user_key = state.resolve_user_key(&headers).await;

    state
        .destroy_session(&user_key, &session_id)
        .await
        .map_err(|e| match e {
            SessionError::NotFound(_) => (StatusCode::NOT_FOUND, e.to_string()),
            SessionError::NotIdle(_) => (StatusCode::CONFLICT, e.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    Ok(with_rolling_cookie(
        StatusCode::NO_CONTENT.into_response(),
        cookie,
    ))
}

/// GET /api/v1/sessions/{id}/stream — WebSocket for a specific session.
pub async fn ws_session_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, StatusCode> {
    let _cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let user_key = state.resolve_user_key(&headers).await;
    let (session_arc, ws_tx) = state
        .get_session(&user_key, &session_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, session_arc, ws_tx)))
}

// ---------------------------------------------------------------------------
// Static file serving
// ---------------------------------------------------------------------------

/// GET / — serve index.html
pub async fn serve_index() -> Response {
    match trustee_web::Asset::get("index.html") {
        Some(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content.data.to_vec(),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "Not found".to_string().into_bytes(),
        )
            .into_response(),
    }
}

/// GET /{file} — serve static files from trustee-web
pub async fn serve_static(Path(file): Path<String>) -> Response {
    match trustee_web::Asset::get(&file) {
        Some(content) => {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "Not found".to_string().into_bytes(),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Command execution helper (shared between legacy and session-scoped routes)
// ---------------------------------------------------------------------------

/// Execute a command on a session. Shared between `post_command` (legacy)
/// and `post_command_session` (session-scoped).
///
/// Handles token injection, workflow state check, session_id/resume_info
/// setup, concurrency permit, and execution trigger.
#[allow(clippy::too_many_arguments)]
async fn execute_command_inner(
    state: &ServerState,
    headers: &axum::http::HeaderMap,
    session_arc: &std::sync::Arc<tokio::sync::Mutex<trustee_core::session::Session>>,
    ws_tx: &broadcast::Sender<String>,
    token_store: &Arc<pep::MemoryTokenStore>,
    req: CommandRequest,
) -> Result<(), (StatusCode, String)> {
    let agent_name = {
        let session = session_arc.lock().await;
        session.agent_name.clone()
    };
    inject_session_token(&state.auth, headers, &agent_name, token_store).await;

    {
        let mut session = session_arc.lock().await;

        if session.workflow_state != trustee_core::types::WorkflowState::Idle {
            return Err((
                StatusCode::CONFLICT,
                "Workflow is running or cancelling".to_string(),
            ));
        }

        // Handle session_id / resume_info (same as original post_command)
        if let Some(ref client_session_id) = req.session_id {
            if session.session_id.is_none() {
                session.session_id = Some(client_session_id.clone());
            }

            if session.resume_info.is_none() {
                if let Some(ref config_toml) = session.config_toml.clone() {
                    let home_dir = session.home_dir.as_deref();
                    if let Ok(Some(info)) =
                        trustee_core::sessions::create_resume_info(
                            config_toml,
                            client_session_id,
                            home_dir,
                        )
                        .await
                    {
                        session.resume_info = Some(info);
                    }
                }
            }
        }

        let permit = state
            .workflow_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Server overloaded".to_string()))?;

        session.token_store = Some(token_store.clone());
        session.workflow_permit = Some(permit);
        session.input = req.command;
        session.execute_command();
    }

    let state_msg = serde_json::json!({"type": "StateChanged", "state": "Running"});
    let _ = ws_tx.send(state_msg.to_string());

    Ok(())
}

// ---------------------------------------------------------------------------
// MCP session token injection (C1 — web-session credentials)
// ---------------------------------------------------------------------------

const WEB_SESSION_CRED_NAME: &str = "__web_session";

async fn inject_session_token(
    auth: &Option<Arc<crate::auth::AuthState>>,
    headers: &axum::http::HeaderMap,
    _agent_name: &str,
    token_store: &pep::MemoryTokenStore,
) {
    use pep::{StoredToken, TokenStore};

    let Some(auth_state) = auth.as_ref() else {
        return;
    };

    let access_token = match resolve_access_token_for_mcp(auth_state, headers).await {
        Ok(token) => token,
        Err(e) => {
            tracing::debug!("Skipping MCP session token injection: {}", e);
            return;
        }
    };

    let expires_at = jwt_expiry(&access_token).unwrap_or_else(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        compute_rfc3339(now + 900)
    });

    let stored = StoredToken::new(&access_token, None, "Bearer", &expires_at, None);

    if let Err(e) = token_store.save(WEB_SESSION_CRED_NAME, &stored) {
        tracing::warn!("Failed to write session token to MemoryTokenStore: {}", e);
    } else {
        tracing::debug!(
            "Injected session token for web-session MCP credentials (expires {})",
            expires_at
        );
    }
}

async fn resolve_access_token_for_mcp(
    auth: &crate::auth::AuthState,
    headers: &axum::http::HeaderMap,
) -> Result<String, String> {
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        if token.starts_with("dev:") {
            return Err("dev tokens not supported for MCP".to_string());
        }
        return Ok(token);
    }

    let session_id = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .map(|c| c.trim())
                .find_map(|c| c.strip_prefix(&format!("{}=", auth.config.cookie_name)).map(|s| s.to_string()))
        })
        .ok_or("no session cookie")?;

    if session_id.starts_with("dev:") {
        return Err("dev tokens not supported for MCP".to_string());
    }

    auth.session_manager
        .get_token(&session_id)
        .await
        .map_err(|e| format!("session lookup: {e}"))
}

fn jwt_expiry(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(parts[1]))
        .ok()?;

    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    let exp = json.get("exp")?.as_u64()?;

    Some(compute_rfc3339(exp))
}

fn compute_rfc3339(epoch_secs: u64) -> String {
    let days = epoch_secs / 86400;
    let rem = epoch_secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mon = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mon <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yr, mon, d, h, m, s)
}

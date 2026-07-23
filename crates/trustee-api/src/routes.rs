//! Axum route handlers for the Trustee API.

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

/// Attach a `Set-Cookie` header to a response if the cookie value is present.
/// Used for rolling session cookies from `check_auth`.
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
// Session discovery DTOs
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
    /// Optional specific checkpoint ID to resume from.
    /// If omitted, resumes from the latest checkpoint.
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
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/health
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /api/v1/session — return current session state.
pub async fn get_session(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let session = state.session.lock().await;

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

    let resp = Json(SessionResponse {
        workflow_state: workflow_state.to_string(),
        output_lines: session.output_lines.clone(),
        todo_lines: session.todo_lines.clone(),
        mcp_servers,
        context_tokens: session.current_context_tokens,
        input: session.input.clone(),
        resume_info_present: session.resume_info.is_some(),
    });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/command — submit a command for execution.
pub async fn post_command(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CommandRequest>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;
    {
        let mut session = state.session.lock().await;

        if session.workflow_state != trustee_core::types::WorkflowState::Idle {
            return Err((
                StatusCode::CONFLICT,
                "Workflow is running or cancelling".to_string(),
            ));
        }

        session.input = req.command;
        session.execute_command();
    }

    // Broadcast state change so all WebSocket clients know the workflow started.
    let state_msg = serde_json::json!({"type": "StateChanged", "state": "Running"});
    let _ = state.ws_tx.send(state_msg.to_string());

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/cancel — cancel the running workflow.
pub async fn post_cancel(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let cancelled;
    {
        let session = state.session.lock().await;

        cancelled = session.workflow_state == trustee_core::types::WorkflowState::Running;
        if cancelled {
            session.cancel_token.cancel();
        }
    }

    // Broadcast state change so all WebSocket clients know the workflow is cancelling.
    if cancelled {
        let state_msg = serde_json::json!({"type": "StateChanged", "state": "Cancelling"});
        let _ = state.ws_tx.send(state_msg.to_string());
    }

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// POST /api/v1/session/handoff — trigger session handoff.
pub async fn post_handoff(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    let mut session = state.session.lock().await;
    session.trigger_handoff(String::new());

    let resp = Json(CommandResponse { accepted: true });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/session/stream — WebSocket for live message streaming.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, StatusCode> {
    let _cookie = crate::auth::check_auth(&state.auth, &headers).await?;
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state)))
}

async fn handle_ws(socket: WebSocket, state: ServerState) {
    use futures::{SinkExt, StreamExt};
    let (mut sender, mut receiver) = socket.split();
    let mut ws_rx = state.ws_tx.subscribe();

    // Send current session state as the first message
    {
        let session = state.session.lock().await;
        let snapshot = SessionResponse {
            workflow_state: format!("{:?}", session.workflow_state),
            output_lines: session.output_lines.clone(),
            todo_lines: session.todo_lines.clone(),
            mcp_servers: session
                .mcp_servers
                .iter()
                .map(|s| McpServerJson {
                    name: s.name.clone(),
                    connected: s.status == trustee_core::types::McpServerStatus::Connected,
                    tool_count: s.tool_count,
                    error: s.error.clone(),
                })
                .collect(),
            context_tokens: session.current_context_tokens,
            input: session.input.clone(),
            resume_info_present: session.resume_info.is_some(),
        };
        if let Ok(json) = serde_json::to_string(&snapshot) {
            let _ = sender.send(Message::Text(json.into())).await;
        }
    }

    // Fan-out loop: broadcast messages to this client
    loop {
        tokio::select! {
            // Receive broadcast messages and forward to client
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
            // Receive messages from client (we mostly ignore, but need to detect close)
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
// Session discovery handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/sessions — list all sessions with checkpoints available for resume.
pub async fn list_sessions(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let config_toml = {
        let session = state.session.lock().await;
        match &session.config_toml {
            Some(c) => c.clone(),
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration not loaded".to_string(),
                ))
            }
        }
    };

    let sessions = trustee_core::sessions::list_all_sessions(&config_toml)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let resp = Json(SessionListResponse { sessions });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/sessions/{id} — get session detail with checkpoints.
pub async fn get_session_detail(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let config_toml = {
        let session = state.session.lock().await;
        match &session.config_toml {
            Some(c) => c.clone(),
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration not loaded".to_string(),
                ))
            }
        }
    };

    let detail = trustee_core::sessions::get_session_detail(&config_toml, &session_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match detail {
        Some((session, checkpoints)) => {
            let resp = Json(SessionDetailResponse {
                session,
                checkpoints,
            });
            Ok(with_rolling_cookie(resp.into_response(), cookie))
        }
        None => Err((StatusCode::NOT_FOUND, "Session not found".to_string())),
    }
}

/// POST /api/v1/sessions/{id}/resume — resume from the latest checkpoint.
///
/// Sets `session.resume_info` so the next `/session/command` continues
/// from the restored checkpoint.
pub async fn resume_session(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
    _body: Option<Json<ResumeRequestBody>>,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let config_toml = {
        let session = state.session.lock().await;
        // Reject if workflow is running
        if session.workflow_state != trustee_core::types::WorkflowState::Idle {
            return Err((
                StatusCode::CONFLICT,
                "Workflow is running or cancelling".to_string(),
            ));
        }
        match &session.config_toml {
            Some(c) => c.clone(),
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration not loaded".to_string(),
                ))
            }
        }
    };

    let resume_info = trustee_core::sessions::create_resume_info(&config_toml, &session_id)
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

    // If the caller specified a specific checkpoint_id, validate it belongs to the session
    // For now we always use the latest checkpoint from create_resume_info.
    // Future: accept optional checkpoint_id in the body to resume from a specific one.
    let checkpoint_id = resume_info.checkpoint_id.clone();
    let iteration = resume_info.iteration;

    {
        let mut session = state.session.lock().await;
        session.resume_info = Some(resume_info);
        // Clear output so the user sees a fresh context when they resume
        session.output_lines.clear();
    }

    // Broadcast state so clients know resume info is loaded
    let msg = serde_json::json!({
        "type": "SessionResumed",
        "session_id": session_id,
        "checkpoint_id": checkpoint_id,
    });
    let _ = state.ws_tx.send(msg.to_string());

    let resp = Json(ResumeResponse {
        accepted: true,
        session_id: session_id.clone(),
        checkpoint_id,
        iteration,
    });
    Ok(with_rolling_cookie(resp.into_response(), cookie))
}

/// GET /api/v1/sessions/{id}/history — load conversation history from
/// the latest checkpoint for display in the Web UI.
pub async fn get_session_history(
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let cookie = crate::auth::check_auth(&state.auth, &headers)
        .await
        .map_err(|s| (s, "Unauthorized".to_string()))?;

    let history = trustee_core::sessions::load_session_history(&session_id)
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

//! Session discovery for the API/Web layer.
//!
//! Wraps `abk::checkpoint` to provide serializable session listing, detail,
//! and resume-info creation — used by the Trustee REST API so users can
//! browse and resume checkpoint sessions from the Web UI.

use serde::{Deserialize, Serialize};

use abk::checkpoint::{
    get_storage_manager,
    models::{CheckpointMetadata, ChatMessage, SessionMetadata as AbkSessionMetadata},
    storage::ProjectMetadata as AbkProjectMetadata,
};
use abk::cli::ResumeInfo;

/// Compact session info suitable for JSON API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub project_name: String,
    pub project_path: String,
    pub checkpoint_count: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub description: Option<String>,
    pub is_current_project: bool,
}

/// Compact checkpoint info for the session detail endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointSummary {
    pub checkpoint_id: String,
    pub session_id: String,
    pub iteration: u32,
    pub workflow_step: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// -----------------------------------------------------------------------
// Conversion helpers
// -----------------------------------------------------------------------

fn session_to_summary(
    session: &AbkSessionMetadata,
    project: &AbkProjectMetadata,
    is_current: bool,
) -> SessionSummary {
    SessionSummary {
        session_id: session.session_id.clone(),
        project_name: project.name.clone(),
        project_path: project.project_path.to_string_lossy().to_string(),
        checkpoint_count: session.checkpoint_count as usize,
        created_at: session.created_at,
        last_accessed: session.last_accessed,
        description: session.description.clone(),
        is_current_project: is_current,
    }
}

fn checkpoint_to_summary(cp: &CheckpointMetadata) -> CheckpointSummary {
    CheckpointSummary {
        checkpoint_id: cp.checkpoint_id.clone(),
        session_id: cp.session_id.clone(),
        iteration: cp.iteration,
        workflow_step: format!("{:?}", cp.workflow_step),
        created_at: cp.created_at,
    }
}

/// Derive the working directory from a trustee config TOML string.
///
/// Looks for `[agent] working_dir`. Falls back to the current directory
/// of the process if not specified or unparseable.
fn config_working_dir(config_toml: &str) -> std::path::PathBuf {
    if let Ok(value) = toml::from_str::<toml::Value>(config_toml) {
        if let Some(agent) = value.get("agent") {
            if let Some(wd) = agent.get("working_dir").and_then(|v| v.as_str()) {
                return std::path::PathBuf::from(wd);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Check whether two paths refer to the same project by canonicalising.
fn paths_match(a: &std::path::Path, b: &std::path::Path) -> bool {
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

// -----------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------

/// List all sessions across all projects that have at least one checkpoint.
///
/// Sessions from the current project (derived from `config_toml`) are listed
/// first, then everything else sorted by `last_accessed` descending.
pub async fn list_all_sessions(config_toml: &str) -> anyhow::Result<Vec<SessionSummary>> {
    let current_dir = config_working_dir(config_toml);
    let manager = get_storage_manager()
        .map_err(|e| anyhow::anyhow!("Failed to get storage manager: {}", e))?;

    let projects = manager
        .list_projects()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list projects: {}", e))?;

    let mut summaries = Vec::new();

    for project in &projects {
        // Skip projects whose path can't be resolved (e.g. deleted dirs,
        // permission denied) — don't let one bad project kill the whole list.
        let project_storage = match manager.get_project_storage(&project.project_path).await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::debug!("Skipping project {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        let sessions = match project_storage.list_sessions().await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("Failed to list sessions for {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        let is_current = paths_match(&project.project_path, &current_dir);

        for session in sessions {
            // Only include sessions that have checkpoints (resumable)
            if session.checkpoint_count > 0 {
                summaries.push(session_to_summary(&session, project, is_current));
            }
        }
    }

    // Sort: current project first, then by last_accessed descending
    summaries.sort_by(|a, b| match (a.is_current_project, b.is_current_project) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => b.last_accessed.cmp(&a.last_accessed),
    });

    Ok(summaries)
}

/// Get detailed information about a specific session, including its checkpoints.
///
/// Searches all projects for the given `session_id`.
/// Returns `None` if the session is not found.
pub async fn get_session_detail(
    _config_toml: &str,
    session_id: &str,
) -> anyhow::Result<Option<(SessionSummary, Vec<CheckpointSummary>)>> {
    let manager = get_storage_manager()
        .map_err(|e| anyhow::anyhow!("Failed to get storage manager: {}", e))?;

    let projects = manager
        .list_projects()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list projects: {}", e))?;

    for project in &projects {
        let project_storage = match manager.get_project_storage(&project.project_path).await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::debug!("Skipping project {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        let sessions = match project_storage.list_sessions().await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("Failed to list sessions for {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        if let Some(session) = sessions.iter().find(|s| s.session_id == session_id) {
            let session_storage = project_storage
                .create_session(session_id)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get session storage: {}", e))?;

            let checkpoints = session_storage
                .list_checkpoints()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to list checkpoints: {}", e))?;

            let summary = session_to_summary(session, project, false);
            let cp_summaries: Vec<CheckpointSummary> =
                checkpoints.iter().map(checkpoint_to_summary).collect();

            return Ok(Some((summary, cp_summaries)));
        }
    }

    Ok(None)
}

/// Create a `ResumeInfo` from the latest checkpoint of a given session.
///
/// Searches all projects for `session_id`, finds the most recent checkpoint,
/// and returns a `ResumeInfo` suitable for setting on `Session::resume_info`.
/// Returns `None` if the session or its checkpoints are not found.
pub async fn create_resume_info(
    _config_toml: &str,
    session_id: &str,
) -> anyhow::Result<Option<ResumeInfo>> {
    let manager = get_storage_manager()
        .map_err(|e| anyhow::anyhow!("Failed to get storage manager: {}", e))?;

    let projects = manager
        .list_projects()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list projects: {}", e))?;

    for project in &projects {
        let project_storage = match manager.get_project_storage(&project.project_path).await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::debug!("Skipping project {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        let sessions = match project_storage.list_sessions().await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("Failed to list sessions for {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        if sessions.iter().any(|s| s.session_id == session_id) {
            let session_storage = project_storage
                .create_session(session_id)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get session storage: {}", e))?;

            let checkpoints = session_storage
                .list_checkpoints()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to list checkpoints: {}", e))?;

            if let Some(latest) = checkpoints.iter().max_by_key(|cp| cp.created_at) {
                return Ok(Some(ResumeInfo {
                    session_id: session_id.to_string(),
                    checkpoint_id: latest.checkpoint_id.clone(),
                    iteration: latest.iteration,
                    project_path: Some(project.project_path.clone()),
                }));
            }

            // Session found but no checkpoints
            return Ok(None);
        }
    }

    Ok(None)
}

// -----------------------------------------------------------------------
// Conversation history loading
// -----------------------------------------------------------------------

/// A single chat message rendered in the Web UI conversation history.
///
/// Each message maps to how the TUI/Web already renders things:
/// - `user` messages → right-aligned chat bubble
/// - `assistant` messages → left-aligned agent bubble (markdown)
/// - `assistant` with `tool_calls` → tool-pending/tool-done lines
/// - `tool` messages → hidden (tool results are shown via tool_calls)
/// - `reasoning` → collapsible reasoning section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    /// "user", "assistant", or "tool"
    pub role: String,
    /// Main message text
    pub content: String,
    /// Reasoning/thinking content (if any)
    pub reasoning: Option<String>,
    /// Tool calls (assistant messages that invoke tools)
    pub tool_calls: Option<Vec<HistoryToolCall>>,
    /// Tool name (for tool-role messages)
    pub name: Option<String>,
}

/// A tool call within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryToolCall {
    pub name: String,
    /// Short description of what the tool was called with (for display)
    pub hint: String,
}

/// Metadata about the session/task for the history header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistory {
    pub session_id: String,
    pub checkpoint_id: String,
    pub task_description: String,
    pub iteration: u32,
    pub total_messages: usize,
    pub messages: Vec<HistoryMessage>,
}

/// Load conversation history from a session's latest checkpoint.
///
/// Returns messages suitable for rendering in the Web UI. System messages
/// are filtered out, tool results are summarized, and reasoning is
/// preserved separately.
pub async fn load_session_history(
    session_id: &str,
) -> anyhow::Result<Option<SessionHistory>> {
    let manager = get_storage_manager()
        .map_err(|e| anyhow::anyhow!("Failed to get storage manager: {}", e))?;

    let projects = manager
        .list_projects()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list projects: {}", e))?;

    for project in &projects {
        let project_storage = match manager.get_project_storage(&project.project_path).await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::debug!("Skipping project {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        let sessions = match project_storage.list_sessions().await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("Failed to list sessions for {}: {}", project.project_path.display(), e);
                continue;
            }
        };

        if !sessions.iter().any(|s| s.session_id == session_id) {
            continue;
        }

        let session_storage = project_storage
            .create_session(session_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get session storage: {}", e))?;

        let checkpoints = session_storage
            .list_checkpoints()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list checkpoints: {}", e))?;

        let latest = match checkpoints.iter().max_by_key(|cp| cp.created_at) {
            Some(cp) => cp,
            None => return Ok(None),
        };

        let checkpoint_id = latest.checkpoint_id.clone();
        let iteration = latest.iteration;

        let checkpoint = session_storage
            .load_checkpoint(&checkpoint_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to load checkpoint: {}", e))?;

        let task_description = checkpoint.agent_state.task_description.clone();
        let total_messages = checkpoint.conversation_state.messages.len();

        let messages = convert_messages(&checkpoint.conversation_state.messages);

        return Ok(Some(SessionHistory {
            session_id: session_id.to_string(),
            checkpoint_id,
            task_description,
            iteration,
            total_messages,
            messages,
        }));
    }

    Ok(None)
}

/// Convert ABK `ChatMessage`s to Web UI-friendly `HistoryMessage`s.
///
/// - System messages are dropped (not useful in UI)
/// - Tool-role messages are dropped (tool results shown via assistant's tool_calls)
/// - Very long content is truncated to avoid massive payloads
fn convert_messages(messages: &[ChatMessage]) -> Vec<HistoryMessage> {
    const MAX_CONTENT_LEN: usize = 10_000;

    let mut result = Vec::new();

    for msg in messages {
        // Skip system messages — not useful in the conversation view
        if msg.role == "system" {
            continue;
        }

        // Skip tool-role messages — tool results are shown via the
        // assistant's tool_calls and a compact tool-done line
        if msg.role == "tool" {
            continue;
        }

        let content = if msg.content.len() > MAX_CONTENT_LEN {
            format!("{}...\n[truncated]", &msg.content[..MAX_CONTENT_LEN])
        } else {
            msg.content.clone()
        };

        // Convert tool calls if present
        let tool_calls = msg.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|tc| {
                    let hint = summarize_tool_args(&tc.function.name, &tc.function.arguments);
                    HistoryToolCall {
                        name: tc.function.name.clone(),
                        hint,
                    }
                })
                .collect::<Vec<_>>()
        });

        // For assistant messages that only contain tool calls (empty content),
        // we still emit them so tool call lines render
        if msg.role == "assistant" && content.is_empty() && tool_calls.is_some() {
            result.push(HistoryMessage {
                role: "assistant".to_string(),
                content: String::new(),
                reasoning: msg.reasoning.clone(),
                tool_calls,
                name: None,
            });
            continue;
        }

        result.push(HistoryMessage {
            role: msg.role.clone(),
            content,
            reasoning: msg.reasoning.clone(),
            tool_calls,
            name: msg.name.clone(),
        });
    }

    result
}

/// Create a short human-readable hint from tool call arguments.
///
/// e.g. `{"command": "ls -la /tmp"}` → `ls -la /tmp`
///      `{"file_path": "/foo/bar.rs"}` → `/foo/bar.rs`
fn summarize_tool_args(name: &str, args: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(_) => return args.chars().take(200).collect(),
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return args.chars().take(200).collect(),
    };

    match name {
        "bash" | "execute_command" => {
            obj.get("command").and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        "read" | "read_file" => {
            obj.get("file_path").and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        "write" | "write_file" | "edit" => {
            obj.get("file_path").and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        "grep" | "search" => {
            obj.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        "glob" => {
            obj.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        "todowrite" => {
            let count = obj.get("todos").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{} items", count))
        }
        "websearch" | "webfetch" => {
            obj.get("query").or_else(|| obj.get("url")).and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        _ => {
            obj.values().next().and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                _ => Some(v.to_string()),
            })
        }
    }
    .unwrap_or_default()
    .chars()
    .take(200)
    .collect()
}

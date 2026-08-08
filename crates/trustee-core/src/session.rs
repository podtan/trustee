//! Session state — core agent session without any UI concerns.
//!
//! This struct holds all the state shared between frontends (TUI, API, Web):
//! output lines, input, workflow state, config, resume info, MCP servers, etc.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use abk::cli::ResumeInfo;
use abk::context::RunContext;

use std::sync::atomic::{AtomicU8, Ordering};

use crate::types::{
    AutoHandoffConfig, BuildInfo, CapturedText, HandoffCaptureSink, McpServerInfo, McpServerStatus,
    TuiMessage, WorkflowState,
};

/// Core session state for the Trustee agent.
///
/// Holds all state that is independent of the presentation layer (TUI, API, Web).
/// Frontend crates compose this struct and add their own UI-specific fields.
pub struct Session {
    /// Input buffer for user commands
    pub input: String,
    /// Output log lines
    pub output_lines: Vec<String>,
    /// Sender for messages from async workflows (clone and pass to workflow runners)
    pub workflow_tx: mpsc::UnboundedSender<TuiMessage>,
    /// Current workflow lifecycle state
    pub workflow_state: WorkflowState,
    /// Configuration TOML for ABK workflows
    pub config_toml: Option<String>,
    /// Secrets for ABK workflows
    pub secrets: Option<HashMap<String, String>>,
    /// Build info for ABK workflows
    pub build_info: Option<BuildInfo>,
    /// Resume info from the last completed task for session continuity
    pub resume_info: Option<ResumeInfo>,
    /// Saved resume_info before execute_command consumes it; restored if task
    /// is cancelled before producing a real checkpoint (mistake-ENTER recovery).
    pub backup_resume_info: Option<ResumeInfo>,
    /// Latest todo list from LLM todowrite tool
    pub todo_lines: Vec<String>,
    /// Cancellation token for aborting the current workflow
    pub cancel_token: CancellationToken,
    /// Command buffered by user during cancellation wind-down.
    pub pending_command: Option<String>,
    /// Whether a session handoff (Ctrl+H) should fire once the current workflow cancels.
    pub handoff_pending: bool,
    /// In-flight spinner entries: (tool_name, output_lines_index, hint).
    pub pending_tool_lines: Vec<(String, usize, Option<String>)>,
    /// Current context token count (updated from ApiCallStarted events).
    pub current_context_tokens: usize,
    /// Auto-handoff configuration parsed from [tui.auto_handoff].
    pub auto_handoff: AutoHandoffConfig,
    /// MCP server statuses received from agent init
    pub mcp_servers: Vec<McpServerInfo>,
    /// Whether the session should quit
    pub should_quit: bool,
    /// Whether auto-scroll is enabled (follows new output)
    pub auto_scroll: bool,

    // --- TMU Phase 1: Stateless Core ---
    /// Agent name for checkpoint/token paths (replaces ABK_AGENT_NAME env var).
    /// Defaults to "trustee". Set from config at startup.
    pub agent_name: String,
    /// Per-session token store (None = FileTokenStore fallback).
    /// When set, MCP credential flows use this instead of file-based storage.
    pub token_store: Option<Arc<dyn pep::token_store::TokenStore>>,

    // --- Project/Session Identity (backward compatible, all None = old behavior) ---
    /// Storage partition key (replaces path hash). None = hash(working_dir)
    pub project_id: Option<String>,
    /// Human-readable project name. None = directory name
    pub project_name: Option<String>,
    /// Storage directory name (replaces timestamp slug). None = auto-generate
    pub session_id: Option<String>,
    /// Human-readable session name. None = no description
    pub session_name: Option<String>,
    /// Per-user home directory for checkpoint storage. None = default ~/.{agent_name}/
    /// Set to ~/.trustee/users/{user_hash}/ for per-user isolation in web mode.
    pub home_dir: Option<std::path::PathBuf>,

    /// Concurrency permit — held while a workflow is running.
    /// When the workflow completes (state → Idle), this is dropped,
    /// releasing the permit back to the global semaphore.
    /// None when no workflow is running or when concurrency limiting is disabled.
    pub workflow_permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl Session {
    /// Create a new Session with default state and a fresh message channel.
    ///
    /// Returns `(Session, Receiver)` so the caller can own the receiver
    /// without locking the session (prevents deadlock in async drain loops).
    pub fn new() -> (Self, mpsc::UnboundedReceiver<TuiMessage>) {
        let (workflow_tx, workflow_rx) = mpsc::unbounded_channel();
        let session = Self {
            input: String::new(),
            output_lines: Vec::new(),
            workflow_tx,
            workflow_state: WorkflowState::Idle,
            config_toml: None,
            secrets: None,
            build_info: None,
            resume_info: None,
            backup_resume_info: None,
            todo_lines: Vec::new(),
            cancel_token: CancellationToken::new(),
            pending_command: None,
            handoff_pending: false,
            pending_tool_lines: Vec::new(),
            current_context_tokens: 0,
            auto_handoff: AutoHandoffConfig::default(),
            mcp_servers: Vec::new(),
            should_quit: false,
            auto_scroll: true,
            agent_name: "trustee".to_string(),
            token_store: None,
            project_id: None,
            project_name: None,
            session_id: None,
            session_name: None,
            home_dir: None,
            workflow_permit: None,
        };
        (session, workflow_rx)
    }

    /// Parse auto-handoff configuration from the stored config TOML.
    pub fn parse_auto_handoff_config(&mut self) {
        if let Some(ref config_toml) = self.config_toml {
            self.auto_handoff = crate::config::parse_auto_handoff_config(config_toml);
        }
    }

    /// Handle messages from async workflows.
    ///
    /// This processes all workflow lifecycle events, output updates, and state transitions.
    /// Returns `true` if the caller should check for pending commands/handoffs after.
    pub fn handle_workflow_message(&mut self, msg: TuiMessage) {
        match msg {
            TuiMessage::WorkflowCancelled => {
                self.output_lines.push("⏹ Workflow cancelled".to_string());
                self.output_lines.push("".to_string());
                self.workflow_state = WorkflowState::Cancelling;
            }
            TuiMessage::OutputLine(line) => {
                self.output_lines.push(line);
            }
            TuiMessage::StreamDelta(delta) => {
                if let Some(last) = self.output_lines.last_mut() {
                    last.push_str(&delta);
                } else {
                    self.output_lines.push(delta);
                }
            }
            TuiMessage::ReasoningDelta(delta) => {
                if let Some(last) = self.output_lines.last_mut() {
                    if !last.starts_with('\x01') {
                        last.insert(0, '\x01');
                    }
                    last.push_str(&delta);
                } else {
                    self.output_lines.push(format!("\x01{}", delta));
                }
            }
            TuiMessage::WorkflowCompleted => {
                self.output_lines.push("✓ Workflow completed".to_string());
                self.output_lines.push("".to_string());
                if self.workflow_state == WorkflowState::Running {
                    self.workflow_state = WorkflowState::Cancelling;
                }
            }
            TuiMessage::WorkflowError(err) => {
                self.output_lines.push(format!("✗ Error: {}", err));
                self.output_lines.push("".to_string());
                if self.workflow_state == WorkflowState::Running {
                    self.workflow_state = WorkflowState::Cancelling;
                }
            }
            TuiMessage::TodoUpdate(content) => {
                self.todo_lines = content.lines().map(|l| l.to_string()).collect();
            }
            TuiMessage::ToolPending { tool_name, hint } => {
                let label = match &hint {
                    Some(h) => format!("⠋ {} {}", tool_name, h),
                    None => format!("⠋ {}", tool_name),
                };
                let idx = self.output_lines.len();
                self.output_lines.push(label);
                self.pending_tool_lines.push((tool_name, idx, hint));
            }
            TuiMessage::ToolDone { tool_name, success, hint } => {
                let status = if success { "✓" } else { "✗" };
                if let Some(pos) = self.pending_tool_lines.iter().position(|(n, _, _)| *n == tool_name) {
                    let (_, idx, pending_hint) = self.pending_tool_lines.remove(pos);
                    let h = hint.or(pending_hint);
                    let label = match &h {
                        Some(h) => format!("{} {} {}", status, tool_name, h),
                        None => format!("{} {}", status, tool_name),
                    };
                    if idx < self.output_lines.len() {
                        self.output_lines[idx] = label;
                        return;
                    }
                    self.output_lines.push(label);
                } else {
                    let label = match &hint {
                        Some(h) => format!("{} {} {}", status, tool_name, h),
                        None => format!("{} {}", status, tool_name),
                    };
                    self.output_lines.push(label);
                }
            }
            TuiMessage::ResumeInfo(info) => {
                if self.workflow_state == WorkflowState::Cancelling && info.is_none() {
                    self.resume_info = self.backup_resume_info.take();
                } else if info.is_some() {
                    // Only overwrite with a valid resume_info.
                    // Don't let ResumeInfo(None) clobber a valid Some that was
                    // set by an earlier incremental checkpoint message — this
                    // happens when the error/cancel path fails to create a
                    // final checkpoint but earlier checkpoints exist.
                    self.resume_info = info;
                    self.backup_resume_info = None;
                }
                // If info is None and we're not cancelling, keep existing resume_info.

                // Capture session_id from resume_info so it persists for the
                // lifetime of this conversation. This is immutable — once ABK
                // assigns it, we never change it.
                if let Some(ref ri) = self.resume_info {
                    if self.session_id.is_none() {
                        self.session_id = Some(ri.session_id.clone());
                    }
                }

                if self.workflow_state == WorkflowState::Cancelling {
                    self.workflow_state = WorkflowState::Idle;
                    // Release the concurrency permit when the workflow finishes.
                    self.workflow_permit = None;
                }
                if self.resume_info.is_some() {
                    if std::env::var("RUST_LOG")
                        .map(|v| v.to_lowercase().contains("debug"))
                        .unwrap_or(false)
                    {
                        self.output_lines.push("🔄 Session preserved — next command will continue this session".to_string());
                    }
                }
                if self.workflow_state == WorkflowState::Idle && self.handoff_pending {
                    self.handoff_pending = false;
                    self.trigger_handoff(String::new());
                } else if let Some(cmd) = self.pending_command.take() {
                    self.input = cmd;
                    self.execute_command();
                }
            }
            TuiMessage::ContextTokensUpdated(count) => {
                self.current_context_tokens = count;
                if self.auto_handoff.enabled
                    && count >= self.auto_handoff.context_threshold
                    && self.workflow_state == WorkflowState::Running
                    && !self.handoff_pending
                    && self.resume_info.is_some()
                {
                    self.handoff_pending = true;
                    self.cancel_token.cancel();
                    self.workflow_state = WorkflowState::Cancelling;
                    self.output_lines.push(format!(
                        "🔄 Auto-handoff: cancelling workflow, context tokens ({}) ≥ threshold ({})",
                        count, self.auto_handoff.context_threshold
                    ));
                }
            }
            TuiMessage::McpServerStatus { name, connected, tool_count, error } => {
                let status = if connected { McpServerStatus::Connected } else { McpServerStatus::Failed };
                if let Some(existing) = self.mcp_servers.iter_mut().find(|s| s.name == name) {
                    existing.status = status;
                    existing.tool_count = tool_count;
                    existing.error = error;
                } else {
                    self.mcp_servers.push(McpServerInfo { name, status, tool_count, error });
                }
            }
            TuiMessage::HandoffReady(briefing) => {
                self.workflow_state = WorkflowState::Idle;
                self.resume_info = None;
                // Clear session identity so execute_command generates fresh
                // session_id/session_name for the new post-handoff session.
                self.session_id = None;
                self.session_name = None;
                self.input = briefing;
                self.execute_command();
            }
            TuiMessage::SessionTitleUpdated(title) => {
                // LLM-generated title has arrived — update the session name.
                self.session_name = Some(title);
            }
        }
        if self.auto_scroll {
            // Signal to frontend that it should scroll to bottom.
            // Frontend reads auto_scroll flag directly.
        }
    }

    /// Execute the current command in the input buffer.
    ///
    /// Spawns an async ABK workflow task, clears the input buffer, and sets
    /// workflow_state to Running.
    pub fn execute_command(&mut self) {
        let command = self.input.trim().to_string();

        if self.workflow_state != WorkflowState::Idle {
            self.pending_command = Some(command);
            self.output_lines.push("⏳ Previous workflow finishing — command queued".to_string());
            self.input.clear();
            return;
        }

        let is_continuation = self.resume_info.is_some();

        if !is_continuation {
            self.output_lines.clear();
            // Auto-derive session_id and session_name from the first command
            // if not explicitly set. The session_id uses timestamp + UUID suffix
            // (session_YYYY_MM_DD_HH_MM_{uuid8}) for uniqueness across all
            // interfaces. The session_name is a human-readable display name
            // (truncated command text).
            if self.session_id.is_none() {
                let timestamp = chrono::Utc::now().format("%Y_%m_%d_%H_%M");
                let uuid_suffix = uuid::Uuid::new_v4().simple().to_string();
                let uuid8 = &uuid_suffix[..8];
                self.session_id = Some(format!("session_{}_{}", timestamp, uuid8));
            }
            if self.session_name.is_none() {
                let derived = if command.len() > 80 {
                    format!("{}...", &command[..77])
                } else {
                    command.clone()
                };
                self.session_name = Some(derived);
            }
        }

        self.output_lines.push(format!("> {}", command));

        let config_toml = match &self.config_toml {
            Some(c) => c.clone(),
            None => {
                self.output_lines.push("✗ Error: Configuration not loaded".to_string());
                self.output_lines.push("".to_string());
                return;
            }
        };

        let secrets = self.secrets.clone().unwrap_or_default();
        // Clone secrets for post-completion title generation (secrets are moved into run_task below)
        let title_secrets = secrets.clone();
        let build_info = self.build_info.clone();
        let tx = self.workflow_tx.clone();

        let agent_name = self.agent_name.clone();
        let token_store = self.token_store.clone();
        let project_id = self.project_id.clone();
        let project_name = self.project_name.clone();
        let session_id = self.session_id.clone();
        let session_name = self.session_name.clone();
        let home_dir = self.home_dir.clone();

        self.backup_resume_info = self.resume_info.clone();
        let resume_info = self.resume_info.take();

        self.workflow_state = WorkflowState::Running;
        self.auto_scroll = true;

        self.cancel_token = CancellationToken::new();
        let child_token = self.cancel_token.clone();

        let (resume_tx, mut resume_rx) = mpsc::unbounded_channel();

        let resume_forward_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(info) = resume_rx.recv().await {
                resume_forward_tx.send(TuiMessage::ResumeInfo(info)).ok();
            }
        });

        tokio::spawn(async move {
            let tui_sink: abk::orchestration::output::SharedSink =
                Arc::new(crate::session::TuiForwardSink::new(tx.clone()));

            // Build RunContext from session fields for stateless operation
            let mut run_ctx = RunContext::new()
                .with_agent_name(agent_name.clone());

            // Set home_dir for per-user isolation if provided
            if let Some(ref dir) = home_dir {
                run_ctx = run_ctx.with_home_dir(dir.clone());
            }

            // Set project identity if any field is provided
            if project_id.is_some() || project_name.is_some() {
                run_ctx = run_ctx.with_project(abk::context::ProjectIdentity {
                    id: project_id.unwrap_or_else(|| "default".to_string()),
                    name: project_name,
                });
            }

            // Set session identity if any field is provided
            if session_id.is_some() || session_name.is_some() {
                run_ctx = run_ctx.with_session(abk::context::SessionIdentity {
                    id: session_id.unwrap_or_else(|| "default".to_string()),
                    name: session_name,
                });
            }

            #[cfg(feature = "registry-mcp-token")]
            {
                if let Some(ref ts) = token_store {
                    run_ctx = run_ctx.with_token_store(ts.clone());
                }
            }

            // Run the entire workflow inside a TUI-mode scope and a
            // per-task logger scope. This replaces the old process-global
            // set_tui_mode()/init_global_logger() mutations with task-local
            // scopes, enabling safe concurrent multi-user operation.
            let scope_logger = {
                abk::observability::Logger::with_agent_name(
                    None::<&std::path::Path>,
                    Some("INFO"),
                    Some(&agent_name),
                ).unwrap_or_else(|_| abk::observability::Logger::new(None, Some("INFO")).unwrap())
            };

            let result = abk::observability::with_logger(scope_logger, async {
                abk::observability::with_tui_mode(true, async {
                    abk::cli::run_task_from_raw_config(
                        &config_toml,
                        secrets,
                        build_info,
                        &command,
                        Some(tui_sink),
                        resume_info,
                        Some(resume_tx),
                        Some(child_token),
                        Some(&run_ctx),
                    )
                    .await
                })
                .await
            })
            .await;

            let task_result = result.unwrap_or_else(|e| abk::cli::TaskResult {
                success: false,
                error: Some(e.to_string()),
                // resume_info will be None here, but the on_checkpoint channel
                // may have already delivered a valid ResumeInfo via TuiMessage.
                // The ResumeInfo handler now ignores None when a valid Some exists,
                // so this None won't clobber the earlier incremental checkpoint.
                resume_info: None,
            });

            let msg = if task_result.success {
                TuiMessage::WorkflowCompleted
            } else {
                TuiMessage::WorkflowError(task_result.error.unwrap_or_default())
            };
            tx.send(msg).ok();
            tx.send(TuiMessage::ResumeInfo(task_result.resume_info)).ok();

            // Solution B: After successful completion, spawn a lightweight LLM call
            // to generate a descriptive session title. The title is sent back via
            // SessionTitleUpdated message. This is fire-and-forget — errors are
            // logged but don't affect the session.
            if task_result.success {
                let title_tx = tx.clone();
                let title_config = config_toml.clone();
                let title_command = command.clone();

                tokio::spawn(async move {
                    match abk::cli::generate_session_title(
                        &title_config,
                        title_secrets,
                        &title_command,
                    )
                    .await
                    {
                        Ok(Some(title)) => {
                            title_tx.send(TuiMessage::SessionTitleUpdated(title)).ok();
                        }
                        Ok(None) => {
                            // Empty title — keep the default truncated-command title
                        }
                        Err(e) => {
                            // Non-fatal: keep the default truncated-command title
                            let _ = e;
                        }
                    }
                });
            }
        });

        self.input.clear();
    }

    /// Trigger a session handoff.
    ///
    /// Runs a single LLM call using the current session's resume_info to generate
    /// a briefing. On completion, sends `TuiMessage::HandoffReady(briefing)`.
    pub fn trigger_handoff(&mut self, hint: String) {
        if self.resume_info.is_none() {
            self.output_lines.push("ℹ Nothing to hand off — run a task first".to_string());
            return;
        }

        let config_toml = match &self.config_toml {
            Some(c) => c.clone(),
            None => {
                self.output_lines.push("✗ Error: Configuration not loaded".to_string());
                return;
            }
        };

        let secrets = self.secrets.clone().unwrap_or_default();
        let build_info = self.build_info.clone();
        let tx = self.workflow_tx.clone();

        let agent_name = self.agent_name.clone();
        let token_store = self.token_store.clone();
        let project_id = self.project_id.clone();
        let project_name = self.project_name.clone();
        let session_id = self.session_id.clone();
        let session_name = self.session_name.clone();
        let home_dir = self.home_dir.clone();

        let resume_info = self.resume_info.take();

        self.workflow_state = WorkflowState::Running;
        self.auto_scroll = true;
        self.cancel_token = CancellationToken::new();
        let child_token = self.cancel_token.clone();

        self.output_lines.push("🔀 Generating session handoff briefing...".to_string());

        tokio::spawn(async move {
            let (cap_tx, mut cap_rx) = mpsc::unbounded_channel::<CapturedText>();
            let cap_sink: abk::orchestration::output::SharedSink =
                Arc::new(HandoffCaptureSink::new(cap_tx, child_token.clone()));

            let base = "Output a session handoff briefing in at most 300 lines. \
                 Do NOT use any tools. Include: the FULL ABSOLUTE PATH of every \
                 project/repository being worked on (e.g. /Projects/Foo/bar — never \
                 omit the leading path), all project/task/workstream UUIDs referenced, \
                 every file created or modified with its full absolute path, all \
                 commands run and their outcomes, the current state of the work, any \
                 blockers, and the exact next action to take. \
                 Output ONLY the briefing text — no preamble, headers, or closing remarks.";
            let prompt = if hint.is_empty() {
                base.to_string()
            } else {
                format!("{base}\n\nIn the briefing also consider: {hint}")
            };

            let (dummy_tx, _dummy_rx) = mpsc::unbounded_channel();

            // Build RunContext for stateless operation — same fields as execute_command
            let mut run_ctx = RunContext::new()
                .with_agent_name(agent_name.clone());

            // Set home_dir for per-user isolation
            if let Some(ref dir) = home_dir {
                run_ctx = run_ctx.with_home_dir(dir.clone());
            }

            // Set project identity if any field is provided
            if project_id.is_some() || project_name.is_some() {
                run_ctx = run_ctx.with_project(abk::context::ProjectIdentity {
                    id: project_id.unwrap_or_else(|| "default".to_string()),
                    name: project_name,
                });
            }

            // Set session identity if any field is provided
            if session_id.is_some() || session_name.is_some() {
                run_ctx = run_ctx.with_session(abk::context::SessionIdentity {
                    id: session_id.unwrap_or_else(|| "default".to_string()),
                    name: session_name,
                });
            }
            #[cfg(feature = "registry-mcp-token")]
            {
                // Note: token_store not available in handoff — handoffs don't
                // need MCP credentials, so we skip it here.
            }

            // Run inside per-task logger + TUI-mode scope (task-local, not process-global)
            let scope_logger = abk::observability::Logger::with_agent_name(
                None,
                Some("INFO"),
                Some(&agent_name),
            ).unwrap_or_else(|_| abk::observability::Logger::new(None, Some("INFO")).unwrap());

            let _res = abk::observability::with_logger(scope_logger, async {
                abk::observability::with_tui_mode(true, async {
                    abk::cli::run_task_from_raw_config(
                        &config_toml,
                        secrets,
                        build_info,
                        &prompt,
                        Some(cap_sink),
                        resume_info,
                        Some(dummy_tx),
                        Some(child_token),
                        Some(&run_ctx),
                    )
                    .await
                })
                .await
            })
            .await;

            let mut text_parts = String::new();
            let mut reasoning_parts = String::new();
            while let Ok(captured) = cap_rx.try_recv() {
                match captured {
                    CapturedText::Text(s) => text_parts.push_str(&s),
                    CapturedText::Reasoning(s) => reasoning_parts.push_str(&s),
                }
            }

            let briefing = if !text_parts.trim().is_empty() {
                text_parts.trim().to_string()
            } else if !reasoning_parts.trim().is_empty() {
                reasoning_parts.trim().to_string()
            } else {
                "Session handoff: briefing unavailable — continue from previous context.".to_string()
            };

            tx.send(TuiMessage::HandoffReady(briefing)).ok();
        });
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new().0
    }
}

/// A sink that forwards ABK `OutputEvent`s to the message channel.
///
/// Includes a 3-state atomic state machine (IDLE/REASONING/CONTENT) that
/// inserts blank separator lines when transitioning between reasoning and
/// content streams, so the frontend can distinguish them visually.
pub struct TuiForwardSink {
    tx: mpsc::UnboundedSender<TuiMessage>,
    stream_state: AtomicU8,
}

/// Stream state machine constants.
const STREAM_IDLE: u8 = 0;
const STREAM_REASONING: u8 = 1;
const STREAM_CONTENT: u8 = 2;

impl TuiForwardSink {
    pub fn new(tx: mpsc::UnboundedSender<TuiMessage>) -> Self {
        Self {
            tx,
            stream_state: AtomicU8::new(STREAM_IDLE),
        }
    }
}

impl abk::orchestration::output::OutputSink for TuiForwardSink {
    fn emit(&self, event: abk::orchestration::output::OutputEvent) {
        use abk::orchestration::output::OutputEvent;

        let msg = match event {
            OutputEvent::StreamingChunk { delta } => {
                if delta.is_empty() {
                    return;
                }
                let prev = self.stream_state.swap(STREAM_CONTENT, Ordering::Relaxed);
                if prev != STREAM_CONTENT {
                    let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                }
                let _ = self.tx.send(TuiMessage::StreamDelta(delta));
                return;
            }

            OutputEvent::LlmResponse { text, model } => {
                TuiMessage::OutputLine(format!("[{}] {}", model, text))
            }

            OutputEvent::Info { message } => {
                // Suppress noisy/no-value messages from ABK
                if message.contains("API call completed successfully") {
                    return;
                }
                TuiMessage::OutputLine(message)
            }

            OutputEvent::WorkflowStarted { task_description } => {
                TuiMessage::OutputLine(format!("🚀 Workflow started: {}", task_description))
            }

            OutputEvent::WorkflowCompleted { reason, iterations } => {
                TuiMessage::OutputLine(format!(
                    "✅ Workflow completed after {} iterations: {}",
                    iterations, reason
                ))
            }

            OutputEvent::IterationStarted { iteration, context_tokens } => {
                let _ = self.tx.send(TuiMessage::ContextTokensUpdated(context_tokens));
                TuiMessage::OutputLine(format!(
                    "📡 Iteration {} | Context = {} tokens",
                    iteration, context_tokens
                ))
            }

            OutputEvent::ApiCallStarted {
                call_number,
                model,
                tool_count,
                streaming,
                context_tokens,
                tool_tokens,
            } => {
                let mode = if streaming { "Streaming" } else { "Non-streaming" };
                let total = context_tokens + tool_tokens;
                let _ = self.tx.send(TuiMessage::ContextTokensUpdated(total));
                // Blank line separator before each API call for readability
                let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                TuiMessage::OutputLine(format!(
                    "🔥 API Call {} | Ctx={}({}+{}) | {} | Model: {} | Tools: {}",
                    call_number, total, context_tokens, tool_tokens, mode, model, tool_count
                ))
            }

            OutputEvent::ToolsExecuting { tool_names, hints } => {
                for (name, hint) in tool_names.into_iter().zip(hints.into_iter()) {
                    let _ = self.tx.send(TuiMessage::ToolPending { tool_name: name, hint });
                }
                self.stream_state.store(STREAM_IDLE, Ordering::Relaxed);
                return;
            }

            OutputEvent::ToolCompleted {
                tool_name,
                success,
                content,
                description,
            } => {
                if tool_name == "todowrite" && success {
                    let _ = self.tx.send(TuiMessage::TodoUpdate(content.clone()));
                }
                let hint = description;
                let _ = self.tx.send(TuiMessage::ToolDone { tool_name, success, hint });
                self.stream_state.store(STREAM_IDLE, Ordering::Relaxed);
                return;
            }

            OutputEvent::Error { message, context } => {
                if let Some(ctx) = context {
                    TuiMessage::OutputLine(format!("❌ Error: {} — {}", message, ctx))
                } else {
                    TuiMessage::OutputLine(format!("❌ Error: {}", message))
                }
            }

            OutputEvent::ReasoningChunk { delta } => {
                if delta.is_empty() {
                    return;
                }
                let prev = self.stream_state.swap(STREAM_REASONING, Ordering::Relaxed);
                if prev != STREAM_REASONING {
                    let _ = self.tx.send(TuiMessage::OutputLine(String::new()));
                }
                let _ = self.tx.send(TuiMessage::ReasoningDelta(delta));
                return;
            }

            OutputEvent::McpServerStatus { name, connected, tool_count, error } => {
                let _ = self.tx.send(TuiMessage::McpServerStatus {
                    name,
                    connected,
                    tool_count,
                    error,
                });
                return;
            }
        };

        self.stream_state.store(STREAM_IDLE, Ordering::Relaxed);
        let _ = self.tx.send(msg);
    }
}

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
    AutoHandoffConfig, BuildInfo, McpServerInfo, McpServerStatus,
    TuiMessage, WorkflowState,
};

/// Truncate a first-command string into a display session name.
///
/// Never slices by raw byte index: non-ASCII scripts (Persian, Arabic, CJK,
/// emoji, …) are multi-byte in UTF-8 and a fixed byte cut can land mid-
/// character, which PANICS ("byte index is not a char boundary") inside the
/// tokio worker and poisons the session mutex (nghr 811ed903).
///
/// Semantics: ≤ 80 CHARS → returned unchanged (pure-ASCII behavior is
/// byte-for-byte identical to the original implementation); > 80 chars →
/// truncated at the last char boundary at or before byte 77, plus "...".
pub fn truncate_session_name(command: &str) -> String {
    if command.chars().count() <= 80 {
        command.to_string()
    } else {
        let end = command
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= 77)
            .last()
            .unwrap_or(0);
        format!("{}...", &command[..end])
    }
}

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

    /// Optional agent identity content (e.g. fetched from Fame).
    /// When set, prepended to `lifecycle.system_template` in the config
    /// clone inside `execute_command()` and `trigger_handoff()`.
    /// None = use the config's default system template.
    pub identity: Option<String>,

    /// Optional model override for the next command.
    /// When set, `[llm.providers.{model}]` from the config TOML is injected
    /// into `[llm.provider]` before creating the agent, switching the LLM
    /// for that command. Consumed per-command (cleared after injection).
    /// None = use the default `[llm.provider]`.
    pub model: Option<String>,

    // --- Handoff rotation (post 0.9.16 bugs) ---
    /// Set to true immediately before the post-handoff briefing execute, so
    /// `execute_command` treats that run as a rotation rather than a fresh
    /// session: it derives a new chain identity but does NOT clear the
    /// visible transcript (Bug 2). Cleared once the rotation is handled.
    pub rotating_from_handoff: bool,
    /// True while the current chain identity was born from a handoff briefing
    /// and no user command has run in it yet (Bug 4). While true, handoff is
    /// blocked entirely (manual + auto) to prevent briefing-of-briefing loops.
    /// Cleared on the first user-initiated command.
    pub briefing_born: bool,
    /// Monotonic count of handoff rotations this session has performed.
    /// Surfaced in live-session listings so the UI can show lineage
    /// (Bug 5: "name rotated silently" → "name · ↻2").
    pub handoff_count: u32,
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
            identity: None,
            model: None,
            rotating_from_handoff: false,
            briefing_born: false,
            handoff_count: 0,
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
                    // Bug 4: never auto-handoff a briefing-born chain — a large
                    // briefing inflating the context would chain handoffs into a
                    // briefing-of-briefing loop. Auto-handoff only re-arms after
                    // a real user command has run in this chain.
                    && !self.briefing_born
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
                // Capture the pre-rotation chain id so the rotation event can
                // tell clients exactly which chain was replaced (Bug 1).
                let old_session_id = self.session_id.clone();
                // Clear session identity so execute_command generates fresh
                // session_id/session_name for the new post-handoff session.
                self.session_id = None;
                self.session_name = None;
                // Rotation markers (Bugs 2/4): preserve the transcript across
                // the identity switch, and mark the new chain as briefing-born
                // so it can't immediately hand off again (briefing-of-briefing
                // loop) until a real user command runs in it.
                self.rotating_from_handoff = true;
                self.briefing_born = true;
                self.handoff_count += 1;
                self.input = briefing;
                self.execute_command();
                // execute_command derives the new chain id synchronously —
                // broadcast the explicit rotation event so clients adopt the
                // new identity without inferring it from ResumeInfo (Bug 1).
                let _ = self.workflow_tx.send(TuiMessage::SessionRotated {
                    old: old_session_id,
                    new: self.session_id.clone(),
                });
            }
            TuiMessage::SessionRotated { .. } => {
                // Identity rotation already applied in-place by the
                // HandoffReady handler. This event is for external clients
                // (web/torpi consoles) — nothing to do in session state.
            }
            TuiMessage::HandoffFailed => {
                // Briefing unavailable — session continuity was already restored
                // by the ResumeInfo(Some) that precedes this message. Return to
                // Idle so the user can retry or continue manually (Bug 5/8).
                self.workflow_state = WorkflowState::Idle;
                self.output_lines.push(
                    "✗ Handoff briefing unavailable — session preserved, try again.".to_string(),
                );
                self.output_lines.push("".to_string());
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

        // Rotation gate (Bug 2): the post-handoff briefing execute is a
        // "fresh chain" run (resume_info == None) but NOT a fresh session —
        // the user's visible transcript must survive the identity switch.
        // Take the flag so it applies exactly once, to this run.
        let rotating_from_handoff = self.rotating_from_handoff;
        self.rotating_from_handoff = false;

        // Bug 4: the first command that is NOT the briefing run itself is a
        // real user command — the briefing-born lock lifts and handoff
        // (manual and auto) re-arms for this chain.
        if !rotating_from_handoff {
            self.briefing_born = false;
        }

// Transcript gate (Bug 2): never wipe the visible transcript for a
        // session that has rotated its chain identity. This covers the
        // briefing run itself (rotating_from_handoff) AND any later command
        // that lands on the non-continuation path (e.g. after a failed
        // checkpoint) — a rotated session is one logical conversation, not a
        // fresh one.
        if !is_continuation && !rotating_from_handoff && self.handoff_count == 0 {
            self.output_lines.clear();
        }
        // Identity derivation: applies to EVERY non-continuation run (fresh
        // session OR handoff rotation) — a rotated session must get its new
        // chain id immediately so the SessionRotated event can carry it.
        if !is_continuation {
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
                self.session_name = Some(truncate_session_name(&command));
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

        // Inject identity into the config clone if present.
        let config_toml = inject_identity(config_toml, &self.identity);

        // Inject model override (select from [llm.providers.{model}]) if present.
        let model_override = self.model.take();
        let config_toml = inject_model(config_toml, model_override);

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

            // Capture session_id from resume_info before it's moved into the channel
            let title_session_id = task_result.resume_info
                .as_ref()
                .map(|ri| ri.session_id.clone());

            tx.send(msg).ok();
            tx.send(TuiMessage::ResumeInfo(task_result.resume_info)).ok();

            // Solution B: After successful completion, spawn a lightweight LLM call
            // to generate a descriptive session title. The title is:
            // 1. Persisted to session_metadata.json on disk via persist_session_title
            // 2. Sent via SessionTitleUpdated message (updates in-memory session_name)
            // Only fires if the title hasn't been LLM-set yet.
            // Fire-and-forget — errors don't affect the session.
            if task_result.success {
                let title_tx = tx.clone();
                let title_config = config_toml.clone();
                let title_command = command.clone();
                let title_ctx = run_ctx.clone();

                tokio::spawn(async move {
                    // Only generate titles for truly fresh sessions.
                    // Check the disk: if session already has checkpoints from prior
                    // runs, or description is already set, skip.
                    if let Some(ref sid) = title_session_id {
                        if !abk::cli::should_generate_title(&title_ctx, sid, &title_command).await {
                            return;
                        }
                    } else {
                        return; // No session ID — can't safely persist
                    }

                    // Small delay to ensure checkpoint metadata writes complete first
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                    // Re-check after delay (checkpoint may have written metadata)
                    if let Some(ref sid) = title_session_id {
                        if !abk::cli::should_generate_title(&title_ctx, sid, &title_command).await {
                            return;
                        }
                    }

                    match abk::cli::generate_session_title(
                        &title_config,
                        title_secrets,
                        &title_command,
                    )
                    .await
                    {
                        Ok(Some(title)) => {
                            // Persist to disk + remote
                            if let Some(ref sid) = title_session_id {
                                if let Err(e) = abk::cli::persist_session_title(
                                    &title_ctx,
                                    &title_config,
                                    sid,
                                    &title,
                                ).await {
                                    let _ = e; // non-fatal
                                }
                            }
                            // Update in-memory session name for live WS clients
                            title_tx.send(TuiMessage::SessionTitleUpdated(title)).ok();
                        }
                        Ok(None) => {}
                        Err(e) => { let _ = e; }
                    }
                });
            }
        });

        self.input.clear();
    }

    /// Request a session handoff through the safe state-machine entry point.
    ///
    /// Mirrors the TUI Ctrl+H behavior:
    /// - Idle → trigger the handoff immediately
    /// - Running → cancel the workflow, fire handoff once it stops
    /// - Cancelling → queue handoff for when it stops
    ///
    /// This prevents any caller (web button, API, future MCP tool) from
    /// spawning a briefing task concurrently with a running workflow
    /// (Bugs 1/2/5).
    pub fn request_handoff(&mut self, hint: String) {
        // Bug 4: a session born from a handoff briefing cannot hand off again
        // before a user command has run in it. Manual double-triggers during
        // handoff investigation spawned two extra chains in 3 minutes; the
        // second briefing was literally about the first briefing. Explicit
        // user action may re-arm it only after real work has happened.
        if self.briefing_born {
            self.workflow_tx
                .send(TuiMessage::OutputLine(
                    "ℹ Handoff unavailable — this session started from a handoff briefing. Run a command first.".to_string(),
                ))
                .ok();
            return;
        }
        match self.workflow_state {
            WorkflowState::Idle => self.trigger_handoff(hint),
            WorkflowState::Running => {
                self.cancel_token.cancel();
                self.workflow_state = WorkflowState::Cancelling;
                self.handoff_pending = true;
                // Broadcast via the message channel so live UI (web/TUI) sees
                // the status immediately. A direct output_lines.push() would
                // only appear after a page refresh (output_lines is served via
                // the /api/v1/session poll, not pushed over WebSocket).
                self.workflow_tx
                    .send(TuiMessage::OutputLine("⏹ Cancelling before handoff...".to_string()))
                    .ok();
            }
            WorkflowState::Cancelling => {
                self.handoff_pending = true;
            }
        }
    }

    /// Trigger a session handoff.
    ///
    /// Runs a single LLM call using the current session's resume_info to generate
    /// a briefing. On completion, sends `TuiMessage::HandoffReady(briefing)`.
    ///
    /// This is the internal Idle-only entry point. Callers that may be invoked
    /// while a workflow is running should use [`Session::request_handoff`].
    pub fn trigger_handoff(&mut self, hint: String) {
        // Belt-and-braces guard: never run a briefing concurrently with a
        // workflow (Bug 1). request_handoff handles the Running/Cancelling
        // states; direct callers must be Idle.
        if self.workflow_state != WorkflowState::Idle {
            self.output_lines
                .push("⏳ Workflow still running — handoff will fire after it stops".to_string());
            return;
        }
        // Bug 4 (defense in depth): request_handoff already rejects this, but
        // trigger_handoff is also reachable from the handoff_pending path —
        // never hand off a chain that hasn't seen a user command yet.
        if self.briefing_born {
            self.output_lines
                .push("ℹ Handoff unavailable — this session started from a handoff briefing. Run a command first.".to_string());
            return;
        }
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

        // Inject identity into the config clone if present.
        let config_toml = inject_identity(config_toml, &self.identity);

        // Inject model override for handoff briefing (use same model as the session).
        let model_override = self.model.clone();
        let config_toml = inject_model(config_toml, model_override);

        let tx = self.workflow_tx.clone();
        let agent_name = self.agent_name.clone();
        let project_id = self.project_id.clone();
        let project_name = self.project_name.clone();
        let session_id = self.session_id.clone();
        let session_name = self.session_name.clone();
        let home_dir = self.home_dir.clone();

        // Preserve resume_info so a failed briefing can restore the session
        // instead of losing all continuity (Bug 4/8).
        let backup_resume_info = self.resume_info.clone();
        let resume_info = match self.resume_info.take() {
            Some(ri) => ri,
            None => {
                // Already checked is_none() above — defensive only.
                self.output_lines.push("ℹ Nothing to hand off — run a task first".to_string());
                return;
            }
        };

        self.workflow_state = WorkflowState::Running;
        self.auto_scroll = true;
        self.cancel_token = CancellationToken::new();

        // Broadcast via the message channel so live UI (web/TUI) sees the
        // status line immediately. A direct output_lines.push() would only
        // appear after a page refresh (output_lines is served via the
        // /api/v1/session poll, not pushed over WebSocket). The drain task
        // pushes this line to output_lines (via handle_workflow_message) AND
        // broadcasts StateChanged(Running) + OutputLine to WS clients.
        self.workflow_tx
            .send(TuiMessage::OutputLine("🔀 Generating session handoff briefing...".to_string()))
            .ok();

        tokio::spawn(async move {
            let base = "Output a session handoff briefing in at most 300 lines. \
                 Do NOT use any tools. The FIRST line must be exactly: \
                 \"Session Title: <a concise descriptive title of this work, maximum 50 characters>\" \
                 — nothing else on that line. The rest of the briefing must include: \
                 the FULL ABSOLUTE PATH of every \
                 project/repository being worked on (e.g. /Projects/Foo/bar — never \
                 omit the leading path), all project/task/workstream UUIDs referenced, \
                 every file created or modified with its full absolute path, all \
                 commands run and their outcomes, the current state of the work, any \
                 blockers, and the exact next action to take. \
                 Output ONLY the briefing text — the title line first, then the body, \
                 with no other headers, preamble, or closing remarks.";
            let prompt = if hint.is_empty() {
                base.to_string()
            } else {
                format!("{base}\n\nIn the briefing also consider: {hint}")
            };

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

            // Run inside per-task logger + TUI-mode scope (task-local, not process-global)
            let scope_logger = abk::observability::Logger::with_agent_name(
                None,
                Some("INFO"),
                Some(&agent_name),
            ).unwrap_or_else(|_| abk::observability::Logger::new(None, Some("INFO")).unwrap());

            // Single direct LLM call — no agentic workflow loop. This is the fix
            // for the session-bricking bug: the full workflow ran tools, looped
            // on the large history, and never completed.
            let result = abk::observability::with_logger(scope_logger, async {
                abk::observability::with_tui_mode(true, async {
                    abk::cli::generate_handoff_briefing(
                        &config_toml,
                        &run_ctx,
                        &resume_info,
                        &prompt,
                    )
                    .await
                })
                .await
            })
            .await;

            match result {
                Ok(briefing) if !briefing.trim().is_empty() => {
                    tx.send(TuiMessage::HandoffReady(briefing)).ok();
                }
                _ => {
                    // Briefing unavailable (LLM failure, truncation, cancel).
                    // Restore session continuity and surface the failure —
                    // NEVER auto-execute a garbage fallback string (Bug 8).
                    tx.send(TuiMessage::ResumeInfo(backup_resume_info)).ok();
                    tx.send(TuiMessage::HandoffFailed).ok();
                }
            }
        });
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new().0
    }
}

/// Prepend agent identity to `lifecycle.system_template` in the config TOML.
///
/// If `identity` is `None` or empty, returns the config unchanged.
/// On parse/serialize failure, returns the config unchanged (best-effort).
fn inject_identity(config_toml: String, identity: &Option<String>) -> String {
    let Some(identity) = identity.as_ref().filter(|s| !s.is_empty()) else {
        return config_toml;
    };

    let Ok(mut table) = config_toml.parse::<toml::Value>() else {
        return config_toml;
    };

    let lifecycle = table
        .get_mut("lifecycle")
        .and_then(|v| v.as_table_mut());

    if let Some(lifecycle) = lifecycle {
        let existing = lifecycle
            .get("system_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let combined = format!("{}\n\n{}", identity, existing);
        lifecycle.insert(
            "system_template".to_string(),
            toml::Value::String(combined),
        );
    } else {
        // No [lifecycle] section — create one with just the identity as template.
        let mut ltable = toml::value::Table::new();
        ltable.insert(
            "system_template".to_string(),
            toml::Value::String(identity.clone()),
        );
        if let Some(table) = table.as_table_mut() {
            table.insert("lifecycle".to_string(), toml::Value::Table(ltable));
        }
    }

    toml::to_string(&table).unwrap_or(config_toml)
}

/// Override the model for the next command.
///
/// `model` is a LITERAL model string (e.g. "GLM-4.7-Flash", "gpt-4o"),
/// not necessarily a provider key. Resolution order:
///
/// 1. If it matches a KEY in `[llm.providers.*]` → swap to that provider,
///    use its default model.
/// 2. Else if it appears in some named provider's `models` list (or equals
///    its `model`) → swap to that provider AND set that model on it.
/// 3. Else — set it as the `model` on the current provider. This lets users
///    pass any model name their endpoint accepts even if unlisted.
///
/// If `model` is `None`, the config is returned unchanged.
fn inject_model(config_toml: String, model: Option<String>) -> String {
    let Some(model_name) = model.as_ref().filter(|s| !s.is_empty()) else {
        return config_toml;
    };

    let Ok(mut table) = config_toml.parse::<toml::Value>() else {
        return config_toml;
    };

    let llm = match table.get_mut("llm").and_then(|v| v.as_table_mut()) {
        Some(l) => l,
        None => return config_toml,
    };

    // Helper: is this named-provider entry a real provider (vs ambiguous)?
    let looks_like_provider = |v: &toml::Value| -> bool {
        v.as_table()
            .map(|t| {
                t.contains_key("base_url")
                    || t.contains_key("api_key")
                    || t.contains_key("models")
            })
            .unwrap_or(false)
    };
    // Helper: does this entry offer this model?
    let offers_model = |v: &toml::Value| -> bool {
        v.get("models")
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().any(|m| m.as_str() == Some(model_name)))
            .unwrap_or(false)
            || v.get("model").and_then(|m| m.as_str()) == Some(model_name.as_str())
    };

    let providers = llm.get("providers").cloned();

    // Case 1: model_name is a provider KEY → swap, keep provider's default model.
    if let Some(ref ps) = providers {
        if let Some(selected) = ps.get(model_name.as_str()) {
            if looks_like_provider(selected) {
                let mut selected = selected.clone();
                if let Some(st) = selected.as_table_mut() {
                    st.entry("provider_type".to_string())
                        .or_insert_with(|| toml::Value::String("openai".to_string()));
                }
                llm.insert("provider".to_string(), selected);
                return toml::to_string(&table).unwrap_or(config_toml);
            }
        }
    }

    // Case 2: model_name is OFFERED by some named provider → swap + set model.
    if let Some(ref ps) = providers {
        if let Some(ptable) = ps.as_table() {
            for (_key, entry) in ptable {
                if looks_like_provider(entry) && offers_model(entry) {
                    let mut selected = entry.clone();
                    if let Some(st) = selected.as_table_mut() {
                        st.insert(
                            "model".to_string(),
                            toml::Value::String(model_name.clone()),
                        );
                        st.entry("provider_type".to_string())
                            .or_insert_with(|| toml::Value::String("openai".to_string()));
                    }
                    llm.insert("provider".to_string(), selected);
                    return toml::to_string(&table).unwrap_or(config_toml);
                }
            }
        }
    }

    // Case 3: literal model string — set it on the current provider.
    if let Some(provider) = llm.get_mut("provider").and_then(|p| p.as_table_mut()) {
        provider.insert(
            "model".to_string(),
            toml::Value::String(model_name.clone()),
        );
    }

    toml::to_string(&table).unwrap_or(config_toml)
}
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

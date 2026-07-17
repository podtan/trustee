//! Workflow execution and message handling.
//!
//! Contains `handle_workflow_message()`, `execute_command()`, `trigger_handoff()`,
//! and clipboard helpers. These process messages from the async workflow channel
//! and mutate `App` state (output_lines, workflow_state, etc.).

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::App;
use crate::types::{
    CapturedText, HandoffCaptureSink, McpServerInfo, McpServerStatus, TuiMessage, WorkflowState,
};

impl App {
    /// Handle messages from async workflows
    pub(crate) fn handle_workflow_message(&mut self, msg: TuiMessage) {
        match msg {
            TuiMessage::WorkflowCancelled => {
                self.output_lines.push("⏹ Workflow cancelled".to_string());
                self.output_lines.push("".to_string());
                self.workflow_state = WorkflowState::Cancelling;
                // Stay in Cancelling — ResumeInfo will transition to Idle
            }
            TuiMessage::OutputLine(line) => {
                self.output_lines.push(line);
            }
            TuiMessage::StreamDelta(delta) => {
                // Append streaming delta to the last line (print-style)
                // instead of creating a new line (println-style).
                if let Some(last) = self.output_lines.last_mut() {
                    last.push_str(&delta);
                } else {
                    self.output_lines.push(delta);
                }
            }
            TuiMessage::ReasoningDelta(delta) => {
                // Same as StreamDelta but prefix with \x01 marker for grey rendering.
                // The marker is stripped during render and the line is styled grey.
                if let Some(last) = self.output_lines.last_mut() {
                    if !last.starts_with('\x01') {
                        // First reasoning on this line — mark it
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
                // Transition to Cancelling while waiting for ResumeInfo.
                // This blocks input until the old task is fully wound down.
                if self.workflow_state == WorkflowState::Running {
                    self.workflow_state = WorkflowState::Cancelling;
                }
            }
            TuiMessage::WorkflowError(err) => {
                self.output_lines.push(format!("✗ Error: {}", err));
                self.output_lines.push("".to_string());
                // Same as WorkflowCompleted: transition to Cancelling
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
                    None    => format!("⠋ {}", tool_name),
                };
                let idx = self.output_lines.len();
                self.output_lines.push(label);
                // Push new entry — same tool can appear multiple times in parallel.
                self.pending_tool_lines.push((tool_name, idx, hint));
            }
            TuiMessage::ToolDone { tool_name, success, hint } => {
                let status = if success { "✓" } else { "✗" };
                // Find the first pending entry for this tool name.
                if let Some(pos) = self.pending_tool_lines.iter().position(|(n, _, _)| *n == tool_name) {
                    let (_, idx, pending_hint) = self.pending_tool_lines.remove(pos);
                    // Prefer hint from ToolDone (bash description); fall back to the
                    // hint we already captured at ToolPending time (file path).
                    let h = hint.or(pending_hint);
                    let label = match &h {
                        Some(h) => format!("{} {} {}", status, tool_name, h),
                        None    => format!("{} {}", status, tool_name),
                    };
                    if idx < self.output_lines.len() {
                        self.output_lines[idx] = label;
                        return; // skip the auto-scroll push below
                    }
                    // idx out of range — fall through to append
                    self.output_lines.push(label);
                } else {
                    // No pending entry — append directly.
                    let label = match &hint {
                        Some(h) => format!("{} {} {}", status, tool_name, h),
                        None    => format!("{} {}", status, tool_name),
                    };
                    self.output_lines.push(label);
                }
            }
            TuiMessage::ResumeInfo(info) => {
                // If the task was cancelled (Cancelling state) and returned no
                // real checkpoint (None), restore the pre-command backup so the
                // original session survives a mistake-ENTER+ESC sequence.
                if self.workflow_state == WorkflowState::Cancelling && info.is_none() {
                    self.resume_info = self.backup_resume_info.take();
                } else {
                    self.resume_info = info;
                    self.backup_resume_info = None; // real checkpoint — discard backup
                }
                // Only transition Cancelling → Idle (old task fully wound down).
                // During Running, ResumeInfo is just a checkpoint snapshot —
                // don't touch the state. ABK sends ResumeInfo after session init
                // and after each iteration checkpoint, which must not reset the UI.
                if self.workflow_state == WorkflowState::Cancelling {
                    self.workflow_state = WorkflowState::Idle;
                }
                if self.resume_info.is_some() {
                    if std::env::var("RUST_LOG")
                        .map(|v| v.to_lowercase().contains("debug"))
                        .unwrap_or(false)
                    {
                        self.output_lines.push("🔄 Session preserved — next command will continue this session".to_string());
                    }
                }
                // If a handoff was queued (Ctrl+H while running), fire it now
                // instead of auto-executing any pending command.
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
                // Auto-handoff: when context exceeds threshold during a running
                // workflow, cancel immediately (same as Ctrl+H while running).
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
                // Briefing generation complete — transition out of Running state,
                // clear resume_info (fresh session), then fire the briefing as a
                // new task. ABK will create a new session checkpoint automatically
                // with the briefing as the first user message.
                self.workflow_state = WorkflowState::Idle;
                self.resume_info = None;
                self.input = briefing;
                self.cursor_position = self.input.chars().count();
                self.execute_command();
            }
        }
        // Auto-scroll to bottom when enabled
        if self.auto_scroll {
            self.scroll = u16::MAX;
        }
    }

    /// Trigger a session handoff (Ctrl+H):
    /// 1. Guard — requires a prior session (`resume_info` must be set)
    /// 2. Run a single LLM call via `run_task_from_raw_config` using the current
    ///    session's `resume_info`; a `HandoffCaptureSink` captures only the text
    ///    response (invisible in the main output panel)
    /// 3. On completion, sends `TuiMessage::HandoffReady(briefing)` which starts
    ///    a brand-new session with the briefing as the first user message
    pub(crate) fn trigger_handoff(&mut self, hint: String) {
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

            abk::observability::set_tui_mode(true);

            // Instruction comes first so the LLM sees the constraint before any user text.
            // User hint (if non-empty) is appended as additional briefing focus — never prepended.
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
            let _res = abk::cli::run_task_from_raw_config(
                &config_toml,
                secrets,
                build_info,
                &prompt,
                Some(cap_sink),
                resume_info,
                Some(dummy_tx),
                Some(child_token),
            )
            .await;

            abk::observability::set_tui_mode(false);

            // Drain the capture channel. Separate text chunks from reasoning chunks.
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

    /// Execute the current command in the input buffer
    ///
    /// Task 50: Wired to ABK's run_task_from_raw_config
    /// Task 55: Creates TuiSink to bridge OutputEvent → TuiMessage channel
    pub(crate) fn execute_command(&mut self) {
        let command = self.input.trim().to_string();

        // If previous workflow still winding down, buffer the command
        if self.workflow_state != WorkflowState::Idle {
            self.pending_command = Some(command);
            self.output_lines.push("⏳ Previous workflow finishing — command queued".to_string());
            self.input.clear();
            self.cursor_position = 0;
            return;
        }

        let is_continuation = self.resume_info.is_some();

        // Only clear output for truly new sessions (Bug #3 fix)
        if !is_continuation {
            self.output_lines.clear();
            self.scroll = 0;
        }

        // Add command to output
        self.output_lines.push(format!("> {}", command));

        // Check if config is available
        let config_toml = match &self.config_toml {
            Some(c) => c.clone(),
            None => {
                self.output_lines.push("✗ Error: Configuration not loaded".to_string());
                self.output_lines.push("".to_string());
                return;
            }
        };

        let secrets = self.secrets.clone().unwrap_or_default();
        let build_info = self.build_info.clone();
        let tx = self.workflow_tx.clone();

        // Snapshot resume_info before consuming it, so we can restore it if
        // the task is cancelled before producing a real checkpoint (e.g. user
        // pressed ENTER by mistake then ESC before the first iteration saved).
        self.backup_resume_info = self.resume_info.clone();

        // Take resume_info (one-time use — consumed on next command)
        let resume_info = self.resume_info.take();

        // Mark workflow as running, re-enable auto-scroll
        self.workflow_state = WorkflowState::Running;
        self.auto_scroll = true;

        // Create a fresh cancellation token for this workflow run.
        self.cancel_token = CancellationToken::new();
        let child_token = self.cancel_token.clone();

        // Create channel for incremental resume_info from ABK checkpoints.
        let (resume_tx, mut resume_rx) = mpsc::unbounded_channel();

        // Spawn a forwarder task that relays incremental resume_info
        let resume_forward_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(info) = resume_rx.recv().await {
                resume_forward_tx.send(TuiMessage::ResumeInfo(info)).ok();
            }
        });

        // Spawn the workflow with TuiSink-based output
        tokio::spawn(async move {
            let tui_sink: abk::orchestration::output::SharedSink =
                Arc::new(crate::tui_sink::TuiSink::new(tx.clone()));

            abk::observability::set_tui_mode(true);

            let result = abk::cli::run_task_from_raw_config(
                &config_toml,
                secrets,
                build_info,
                &command,
                Some(tui_sink),
                resume_info,
                Some(resume_tx),
                Some(child_token),
            )
            .await;

            abk::observability::set_tui_mode(false);

            let task_result = result.unwrap_or_else(|e| abk::cli::TaskResult {
                success: false,
                error: Some(e.to_string()),
                resume_info: None,
            });

            // Send completion message
            let msg = if task_result.success {
                TuiMessage::WorkflowCompleted
            } else {
                TuiMessage::WorkflowError(task_result.error.unwrap_or_default())
            };
            tx.send(msg).ok();

            // Send final resume info back for storage in App
            tx.send(TuiMessage::ResumeInfo(task_result.resume_info)).ok();
        });

        // Clear input buffer and reset cursor
        self.input.clear();
        self.cursor_position = 0;

        // Auto-scroll to bottom
        self.scroll = u16::MAX;
    }

    /// Copy output panel text to the system clipboard.
    pub(crate) fn copy_output_to_clipboard(&mut self) {
        // Strip the \x01 reasoning marker from each line before copying
        let clean: String = self.output_lines.iter()
            .map(|l| l.strip_prefix('\x01').unwrap_or(l).to_owned())
            .collect::<Vec<String>>()
            .join("\n");
        self.copy_to_clipboard(clean);
    }

    /// Copy a string to the system clipboard and show brief feedback.
    pub(crate) fn copy_to_clipboard(&mut self, text: String) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => match clipboard.set_text(&text) {
                Ok(()) => {
                    self.output_lines.push("📋 Copied to clipboard".to_string());
                }
                Err(e) => {
                    self.output_lines.push(format!("✗ Clipboard error: {}", e));
                }
            },
            Err(e) => {
                self.output_lines.push(format!("✗ Clipboard unavailable: {}", e));
            }
        }
    }
}

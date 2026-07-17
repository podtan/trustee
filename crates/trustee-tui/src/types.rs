//! Type definitions for the TUI application.
//!
//! All enums, structs, and type aliases used across TUI modules.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use abk::cli::ResumeInfo;
use abk::orchestration::output::{OutputEvent, OutputSink};

/// Status of an MCP server connection (for the MCP status panel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatus {
    /// Server connected and tools loaded successfully
    Connected,
    /// Server failed to connect (timeout, DNS error, auth failure, etc.)
    Failed,
}

/// Information about a single MCP server shown in the status panel.
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub status: McpServerStatus,
    pub tool_count: usize,
    pub error: Option<String>,
}

/// Which panel currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Output,
    Todo,
    Mcp,
    Input,
}

/// Workflow lifecycle state machine.
///
/// Replaces the previous two-flag approach (`workflow_running` + `workflow_busy`)
/// with a proper 3-state enum that prevents the ESC-cancel race condition where
/// the input box accepted commands during the cancellation wind-down window.
///
/// | State      | Input Title                          | Typing | Enter |
/// |------------|--------------------------------------|--------|-------|
/// | `Idle`     | "Input (Ready)"                     | ✅     | ✅    |
/// | `Running`  | "Input (Running... Esc to cancel)"  | ✅     | ❌    |
/// | `Cancelling`| "Input (Cancelling...)"            | ❌     | ❌    |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    /// No workflow is active — input accepts commands.
    Idle,
    /// A workflow is running — input is read-only, ESC will cancel.
    Running,
    /// ESC was pressed, cancel token fired, waiting for old task to finish.
    /// Input is fully locked (no typing, no Enter) until ResumeInfo arrives.
    Cancelling,
}

/// Messages that can be sent to the TUI from async workflows
#[derive(Debug, Clone)]
pub enum TuiMessage {
    /// A line of output to display
    OutputLine(String),
    /// A streaming delta to append to the last line (print-style, not println)
    StreamDelta(String),
    /// A reasoning delta to append to the last line (displayed in grey)
    ReasoningDelta(String),
    /// Workflow completed
    WorkflowCompleted,
    /// Workflow error
    WorkflowError(String),
    /// Resume info from the completed workflow for session continuity
    ResumeInfo(Option<ResumeInfo>),
    /// Todo list update from LLM todowrite tool
    TodoUpdate(String),
    /// Workflow was cancelled by user (ESC pressed during execution)
    WorkflowCancelled,
    /// LLM-generated handoff briefing ready — start a fresh session with it
    HandoffReady(String),
    /// A native tool call has started (shows spinner)
    ToolPending { tool_name: String, hint: Option<String> },
    /// A native tool call has finished (replaces spinner with ✓/✗)
    ToolDone { tool_name: String, success: bool, hint: Option<String> },
    /// Context token count updated (for auto-handoff threshold checking)
    ContextTokensUpdated(usize),
    /// MCP server status update from agent initialization
    McpServerStatus {
        name: String,
        connected: bool,
        tool_count: usize,
        error: Option<String>,
    },
}

/// Build information for ABK (forward declaration)
pub type BuildInfo = abk::cli::BuildInfo;

/// Auto-handoff configuration parsed from `[tui.auto_handoff]` in trustee.toml.
///
/// When enabled, the TUI monitors context token counts reported by ABK and
/// automatically triggers a session handoff once the threshold is exceeded.
#[derive(Debug, Clone)]
pub struct AutoHandoffConfig {
    /// Whether automatic handoff is enabled.
    pub enabled: bool,
    /// Context token count threshold that triggers auto-handoff.
    pub context_threshold: usize,
}

impl Default for AutoHandoffConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            context_threshold: 170_000,
        }
    }
}

/// Tagged chunk type so we can distinguish reasoning (thinking) content
/// from regular text content after draining the capture channel.
pub(crate) enum CapturedText {
    Text(String),
    Reasoning(String),
}

/// Sink used during handoff briefing — captures LLM response text and
/// cancels the loop immediately if the LLM makes a tool call.
///
/// Also captures ReasoningChunk events as a fallback: some thinking-capable
/// models deliver their entire output through reasoning tokens, so without
/// this the briefing channel would remain empty (issue #63ad71c8).
pub(crate) struct HandoffCaptureSink {
    tx: mpsc::UnboundedSender<CapturedText>,
    cancel: CancellationToken,
}

impl HandoffCaptureSink {
    pub fn new(tx: mpsc::UnboundedSender<CapturedText>, cancel: CancellationToken) -> Self {
        Self { tx, cancel }
    }
}

impl OutputSink for HandoffCaptureSink {
    fn emit(&self, event: OutputEvent) {
        match event {
            OutputEvent::StreamingChunk { delta } if !delta.is_empty() => {
                let _ = self.tx.send(CapturedText::Text(delta));
            }
            OutputEvent::LlmResponse { text, .. } if !text.is_empty() => {
                let _ = self.tx.send(CapturedText::Text(text));
            }
            // Reasoning/thinking tokens — capture as fallback in case the model
            // delivers its entire briefing through reasoning instead of text.
            OutputEvent::ReasoningChunk { delta } if !delta.is_empty() => {
                let _ = self.tx.send(CapturedText::Reasoning(delta));
            }
            // LLM disobeyed "Do NOT use any tools" — cancel immediately.
            // Whatever text was captured before this point becomes the briefing;
            // the empty-briefing fallback handles the case where nothing was captured.
            OutputEvent::ToolsExecuting { .. } => {
                self.cancel.cancel();
            }
            _ => {}
        }
    }
}

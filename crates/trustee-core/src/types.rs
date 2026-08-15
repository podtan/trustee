//! Type definitions shared across trustee frontends (TUI, API, Web).
//!
//! All enums, structs, and type aliases used by trustee-core.

use abk::cli::ResumeInfo;

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

/// Status of an MCP server connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatus {
    /// Server connected and tools loaded successfully
    Connected,
    /// Server failed to connect (timeout, DNS error, auth failure, etc.)
    Failed,
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
    Cancelling,
}

/// Information about a single MCP server.
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub status: McpServerStatus,
    pub tool_count: usize,
    pub error: Option<String>,
}

/// Build information for ABK.
pub type BuildInfo = abk::cli::BuildInfo;

/// Messages exchanged between async workflows and frontends (TUI, API, Web).
///
/// These are the events that flow through the message channel from ABK
/// workflow execution to the presentation layer.
#[derive(Debug, Clone)]
pub enum TuiMessage {
    /// A line of output to display
    OutputLine(String),
    /// A streaming delta to append to the last line (print-style, not println)
    StreamDelta(String),
    /// A reasoning delta to append to the last line (displayed differently)
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
    /// Session chain identity rotated (post-handoff). Carries the old and the
    /// new checkpoint-chain session ids so clients can adopt the new identity
    /// (tab title, history-load target) without inferring it from ResumeInfo.
    /// The live/in-memory registry key does NOT change — only the chain id.
    SessionRotated { old: Option<String>, new: Option<String> },
    /// Handoff briefing failed/unavailable — session preserved, NO auto-execute.
    HandoffFailed,
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
    /// LLM-generated session title ready to replace the truncated command title
    SessionTitleUpdated(String),
}

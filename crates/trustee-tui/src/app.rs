//! TUI Application structure and main loop.
//!
//! This module contains the `App` struct definition, its constructor, the
//! async main event loop, and configuration parsing. All event handling,
//! rendering, and workflow execution logic is split into dedicated modules:
//!
//! - [`event`]    — keyboard, mouse, paste, and resize event handling
//! - [`render`]   — TUI rendering with manual line wrapping (orphan fix)
//! - [`workflow`] — workflow message processing and command execution
//! - [`helpers`]  — text wrapping, visual line computation, color parsing
//! - [`types`]    — all type definitions (enums, structs, sinks)

use std::io;

use crossterm::{
    event::{self, Event, EnableBracketedPaste, DisableBracketedPaste, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    Terminal,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use anyhow::Result;

use crate::helpers::parse_color;
use crate::types::{
    AutoHandoffConfig, BuildInfo, FocusPanel, McpServerInfo, TuiMessage, WorkflowState,
};
use abk::cli::ResumeInfo;

/// Main application state for the TUI
pub struct App {
    /// Input buffer for user commands
    pub input: String,
    /// Cursor position in input buffer (char index, not byte offset)
    pub cursor_position: usize,
    /// Output log lines
    pub output_lines: Vec<String>,
    /// Scroll position in output (vertical). u16::MAX = auto-follow bottom.
    pub scroll: u16,
    /// Whether auto-scroll is enabled (follows new output)
    pub auto_scroll: bool,
    /// Cached max scroll value from last render (for keyboard navigation)
    pub(crate) max_scroll_cache: u16,
    /// Which panel has keyboard focus (Tab cycles)
    pub focus: FocusPanel,
    /// Scroll position in todo panel
    pub todo_scroll: u16,
    /// Cached max scroll for todo panel
    pub(crate) todo_max_scroll_cache: u16,
    /// Manual scroll offset for input box (user-driven)
    pub input_scroll: u16,
    /// Cached max scroll for input box
    pub(crate) input_max_scroll_cache: u16,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Receiver for messages from async workflows
    pub workflow_rx: mpsc::UnboundedReceiver<TuiMessage>,
    /// Sender for messages from async workflows (clone and pass to workflow runners)
    pub workflow_tx: mpsc::UnboundedSender<TuiMessage>,
    /// Current workflow lifecycle state (replaces workflow_running + workflow_busy).
    pub workflow_state: WorkflowState,
    /// Configuration TOML for ABK workflows (Task 50)
    pub config_toml: Option<String>,
    /// Secrets for ABK workflows (Task 50)
    pub secrets: Option<std::collections::HashMap<String, String>>,
    /// Build info for ABK workflows (Task 50)
    pub build_info: Option<BuildInfo>,
    /// Resume info from the last completed task for session continuity
    pub resume_info: Option<ResumeInfo>,
    /// Saved resume_info before execute_command consumes it; restored if task
    /// is cancelled before producing a real checkpoint (mistake-ENTER recovery).
    pub(crate) backup_resume_info: Option<ResumeInfo>,
    /// Latest todo list from LLM todowrite tool
    pub todo_lines: Vec<String>,
    /// Cached inner width of input box (characters per visual line)
    pub(crate) input_inner_width_cache: usize,
    /// Cached panel rectangles for mouse hit-testing (set during render)
    pub(crate) output_rect: Rect,
    pub(crate) todo_rect: Rect,
    pub(crate) mcp_rect: Rect,
    pub(crate) input_rect: Rect,
    /// Scroll position in MCP status panel
    pub mcp_scroll: u16,
    /// Cached max scroll for MCP status panel
    pub(crate) mcp_max_scroll_cache: u16,
    /// When Some, the focused panel is zoomed fullscreen (no borders) for clean
    /// text selection. Mouse capture is disabled while zoomed.
    pub(crate) zoomed_panel: Option<FocusPanel>,
    /// Cancellation token for aborting the current workflow
    pub(crate) cancel_token: CancellationToken,
    /// Command buffered by user during cancellation wind-down.
    pub(crate) pending_command: Option<String>,
    /// Whether a session handoff (Ctrl+H) should fire once the current workflow cancels.
    pub(crate) handoff_pending: bool,
    /// In-flight spinner entries: (tool_name, output_lines_index, hint).
    /// Vec (not HashMap) so duplicate tool names (parallel calls) each get their own line.
    pub(crate) pending_tool_lines: Vec<(String, usize, Option<String>)>,
    /// Spinner frame counter — advances on each render tick.
    pub(crate) spinner_tick: u8,
    /// Current context token count (updated from ApiCallStarted events).
    pub(crate) current_context_tokens: usize,
    /// Auto-handoff configuration parsed from [tui.auto_handoff].
    pub(crate) auto_handoff: AutoHandoffConfig,
    /// MCP server statuses received from agent init
    pub mcp_servers: Vec<McpServerInfo>,
    /// Set when a terminal resize event is received — triggers terminal.clear()
    /// before the next draw to flush stale content from the old buffer dimensions.
    pub(crate) needs_clear: bool,
    /// Style for reasoning/thinking text (parsed from [tui.colors] config).
    /// Defaults to gray + DIM (visible on all terminals including Linux VT).
    pub(crate) reasoning_style: Style,
}

impl App {
    /// Create a new App instance
    pub fn new() -> Self {
        let (workflow_tx, workflow_rx) = mpsc::unbounded_channel();
        Self {
            input: String::new(),
            cursor_position: 0,
            output_lines: vec![
                "Welcome to Trustee TUI".to_string(),
                "Type a task and press Enter to execute".to_string(),
                "Press Ctrl+C to exit".to_string(),
                "".to_string(),
                "Keyboard shortcuts:".to_string(),
                "  ↑/↓ or Page Up/Down - Scroll output".to_string(),
                "  y - Copy visible text (Output/Todo)".to_string(),
                "  Enter - Execute task".to_string(),
                "  Ctrl+H - Session handoff (fresh context with briefing)".to_string(),
                "  Ctrl+Z - Zoom panel for clean text selection (toggle)".to_string(),
                "  Esc - Cancel workflow / Exit".to_string(),
                "  Ctrl+C - Exit".to_string(),
            ],
            scroll: 0,
            auto_scroll: true,
            max_scroll_cache: 0,
            focus: FocusPanel::Input,
            todo_scroll: 0,
            todo_max_scroll_cache: 0,
            input_scroll: 0,
            input_max_scroll_cache: 0,
            should_quit: false,
            workflow_rx,
            workflow_tx,
            workflow_state: WorkflowState::Idle,
            config_toml: None,
            secrets: None,
            build_info: None,
            resume_info: None,
            backup_resume_info: None,
            todo_lines: Vec::new(),
            input_inner_width_cache: 80,
            output_rect: Rect::default(),
            todo_rect: Rect::default(),
            mcp_rect: Rect::default(),
            input_rect: Rect::default(),
            mcp_scroll: 0,
            mcp_max_scroll_cache: 0,
            zoomed_panel: None,
            cancel_token: CancellationToken::new(),
            pending_command: None,
            handoff_pending: false,
            pending_tool_lines: Vec::new(),
            spinner_tick: 0,
            current_context_tokens: 0,
                       auto_handoff: AutoHandoffConfig::default(),
            mcp_servers: Vec::new(),
            needs_clear: false,
            reasoning_style: Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        }
    }

    /// Parse [tui.auto_handoff] and [tui.colors] from the merged config TOML.
    /// Called after `config_toml` is set (from `lib.rs` or whenever config arrives).
    pub fn parse_auto_handoff_config(&mut self) {
        if let Some(ref config_toml) = self.config_toml {
            if let Ok(table) = config_toml.parse::<toml::Value>() {
                if let Some(tui) = table.get("tui").and_then(|v| v.as_table()) {
                    // Parse [tui.auto_handoff]
                    if let Some(ah) = tui.get("auto_handoff").and_then(|v| v.as_table()) {
                        if let Some(enabled) = ah.get("enabled").and_then(|v| v.as_bool()) {
                            self.auto_handoff.enabled = enabled;
                        }
                        if let Some(threshold) = ah.get("context_threshold").and_then(|v| v.as_integer()) {
                            self.auto_handoff.context_threshold = threshold as usize;
                        }
                    }
                    // Parse [tui.colors]
                    if let Some(colors) = tui.get("colors").and_then(|v| v.as_table()) {
                        let mut style = Style::default();
                        if let Some(color_name) = colors.get("reasoning_color").and_then(|v| v.as_str()) {
                            style = style.fg(parse_color(color_name));
                        } else {
                            style = style.fg(Color::Gray);
                        }
                        if let Some(dim) = colors.get("reasoning_dim").and_then(|v| v.as_bool()) {
                            if dim {
                                style = style.add_modifier(Modifier::DIM);
                            }
                        }
                        self.reasoning_style = style;
                    }
                }
            }
        }
    }

    /// Run the main event loop (async version)
    ///
    /// Task 52: Converted from synchronous to async to enable:
    /// - Running async ABK workflows concurrently with TUI
    /// - Using tokio::select! for responsive event handling
    /// - Non-blocking terminal event polling
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main event loop.
        //
        // Uses a multi-phase non-blocking strategy that guarantees terminal
        // input (especially ESC to cancel) is processed every single iteration,
        // even when the workflow channel is flooded with streaming tokens.
        //
        // The new loop order:
        //   1. Drain ALL pending terminal events (non-blocking, Duration::ZERO)
        //   2. Early-exit check (should_quit)
        //   3. Batch-drain workflow messages with try_recv() (up to 256/cycle)
        //   4. Advance spinner + draw UI
        //   5. If nothing was available, block briefly on select! (idle wait)
        loop {
            // --- Phase 1: Non-blocking terminal event drain ---
            loop {
                if event::poll(std::time::Duration::from_millis(0))? {
                    let ev = event::read()?;
                    self.handle_event(ev)?;
                } else {
                    break;
                }
            }

            // --- Phase 2: Early exit ---
            if self.should_quit {
                break;
            }

            // --- Phase 3: Batch-drain workflow messages ---
            let mut processed_any = false;
            for _ in 0..256 {
                match self.workflow_rx.try_recv() {
                    Ok(msg) => {
                        self.handle_workflow_message(msg);
                        processed_any = true;
                    }
                    Err(_) => break,
                }
            }

            // --- Phase 4: Spinner animation + draw ---
            const FRAMES: [char; 8] = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧'];
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
            let frame = FRAMES[(self.spinner_tick as usize) % FRAMES.len()];
            for (_, idx, _) in &self.pending_tool_lines {
                if *idx < self.output_lines.len() {
                    let rest: String = self.output_lines[*idx].chars().skip(1).collect();
                    self.output_lines[*idx] = format!("{}{}", frame, rest);
                }
            }

            // Clear the terminal buffer if a resize event was received.
            if self.needs_clear {
                terminal.clear()?;
                self.needs_clear = false;
            }

            // Draw the UI
            terminal.draw(|f| self.render(f))?;

            // Re-check quit after drawing (handle_event may have set it)
            if self.should_quit {
                break;
            }

            // --- Phase 5: Idle wait ---
            if !processed_any {
                tokio::select! {
                    biased;

                    msg = self.workflow_rx.recv() => {
                        if let Some(msg) = msg {
                            self.handle_workflow_message(msg);
                        }
                    }

                    result = Self::poll_event() => {
                        if let Some(event) = result? {
                            self.handle_event(event)?;
                        }
                    }
                }
            }
        }

        // Restore terminal — use let _ so all steps run even if one fails
        let _ = disable_raw_mode();
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableBracketedPaste, DisableMouseCapture);
        let _ = terminal.show_cursor();

        Ok(())
    }

    /// Poll for terminal events asynchronously
    async fn poll_event() -> Result<Option<Event>> {
        tokio::task::spawn_blocking(|| {
            if event::poll(std::time::Duration::from_millis(50))? {
                Ok(Some(event::read()?))
            } else {
                Ok(None)
            }
        })
        .await?
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

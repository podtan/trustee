//! TUI Application structure and main loop.
//!
//! `App` wraps a [`trustee_core::session::Session`] and adds TUI-specific
//! state (scroll positions, cursor, focus, rendering caches, etc.).
//!
//! All workflow and session logic lives in `Session`; this module and the
//! other TUI modules only handle presentation and terminal interaction.
//!
//! - [`event`]    — keyboard, mouse, paste, and resize event handling
//! - [`render`]   — TUI rendering with manual line wrapping (orphan fix)
//! - [`workflow`] — thin delegation to `Session` methods + clipboard helpers
//! - [`helpers`]  — text wrapping, visual line computation, color parsing
//! - [`types`]    — re-exports from trustee-core + TUI-specific aliases

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
use anyhow::Result;

use crate::helpers::parse_color;
use crate::types::FocusPanel;

use trustee_core::session::Session;

/// Main application state for the TUI.
///
/// Contains a [`Session`] for core agent state plus TUI-only fields for
/// rendering, scrolling, cursor, focus, and terminal interaction.
pub struct App {
    /// Core session state (shared logic, independent of UI).
    pub session: Session,

    // ---- TUI-specific state ----
    /// Cursor position in input buffer (char index, not byte offset)
    pub cursor_position: usize,
    /// Scroll position in output (vertical). u16::MAX = auto-follow bottom.
    pub scroll: u16,
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
    /// Spinner frame counter — advances on each render tick.
    pub(crate) spinner_tick: u8,
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
        let mut session = Session::new();
        session.output_lines = vec![
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
        ];

        Self {
            session,
            cursor_position: 0,
            scroll: 0,
            max_scroll_cache: 0,
            focus: FocusPanel::Input,
            todo_scroll: 0,
            todo_max_scroll_cache: 0,
            input_scroll: 0,
            input_max_scroll_cache: 0,
            input_inner_width_cache: 80,
            output_rect: Rect::default(),
            todo_rect: Rect::default(),
            mcp_rect: Rect::default(),
            input_rect: Rect::default(),
            mcp_scroll: 0,
            mcp_max_scroll_cache: 0,
            zoomed_panel: None,
            spinner_tick: 0,
            needs_clear: false,
            reasoning_style: Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        }
    }

    /// Parse [tui.auto_handoff] and [tui.colors] from the merged config TOML.
    /// Called after `config_toml` is set (from `lib.rs` or whenever config arrives).
    pub fn parse_auto_handoff_config(&mut self) {
        self.session.parse_auto_handoff_config();

        // Parse [tui.colors] — TUI-specific, stays here (uses ratatui styles)
        if let Some(ref config_toml) = self.session.config_toml {
            if let Ok(table) = config_toml.parse::<toml::Value>() {
                if let Some(tui) = table.get("tui").and_then(|v| v.as_table()) {
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
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

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
            if self.session.should_quit {
                break;
            }

            // --- Phase 3: Batch-drain workflow messages ---
            let mut processed_any = false;
            for _ in 0..256 {
                match self.session.workflow_rx.try_recv() {
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
            for (_, idx, _) in &self.session.pending_tool_lines {
                if *idx < self.session.output_lines.len() {
                    let rest: String = self.session.output_lines[*idx].chars().skip(1).collect();
                    self.session.output_lines[*idx] = format!("{}{}", frame, rest);
                }
            }

            // Clear the terminal buffer if a resize event was received.
            if self.needs_clear {
                terminal.clear()?;
                self.needs_clear = false;
            }

            // Draw the UI
            terminal.draw(|f| self.render(f))?;

            // Re-check quit after drawing
            if self.session.should_quit {
                break;
            }

            // --- Phase 5: Idle wait ---
            if !processed_any {
                tokio::select! {
                    biased;

                    msg = self.session.workflow_rx.recv() => {
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

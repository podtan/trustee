//! TUI Application structure and main loop
//!
//! Task 52: Async TUI Loop
//! Converted from synchronous to async to allow concurrent workflow execution
//! with the TUI event loop using tokio::select!

use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind, EnableBracketedPaste, DisableBracketedPaste, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, StyledGrapheme, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use unicode_segmentation::UnicodeSegmentation;
use anyhow::Result;

use crate::tui_sink::TuiSink;
use abk::cli::ResumeInfo;

/// Which panel currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Output,
    Todo,
    Input,
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
}

/// Build information for ABK (forward declaration)
pub type BuildInfo = abk::cli::BuildInfo;

/// Convert a char index to a byte offset in a string.
/// Panics if `char_idx` > number of chars in `s`.
fn char_to_byte_offset(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_pos, _)| byte_pos)
        .unwrap_or(s.len())
}

/// NBSP constant — not treated as whitespace by ratatui's WordWrapper.
const NBSP: &str = "\u{00a0}";
/// Zero-width space — treated as whitespace by ratatui's WordWrapper.
const ZWSP: &str = "\u{200b}";

/// Count the number of visual (word-wrapped) lines a Text will occupy.
/// Mirrors ratatui's word-wrap algorithm (Wrap { trim: false }) exactly
/// by using grapheme-level iteration to match the Paragraph widget's rendering.
///
/// The old character-division estimate under-counted because ratatui breaks
/// on word boundaries, which can produce significantly more visual lines than
/// a naive ceil(chars / width) calculation.
fn estimate_visual_lines(text: &Text, viewport_width: u16) -> usize {
    let w = viewport_width.saturating_sub(2).max(1) as u16;
    if w == 0 {
        return 1;
    }
    if text.lines.is_empty() {
        return 1;
    }

    let mut count = 0usize;

    for line in &text.lines {
        let mut line_width: u16 = 0;
        let mut word_width: u16 = 0;
        let mut whitespace_width: u16 = 0;
        let mut non_whitespace_previous = false;

        for grapheme in line.styled_graphemes(Style::default()) {
            // Inline ratatui's StyledGrapheme::is_whitespace (pub(crate)):
            // ZWSP counts as whitespace; NBSP does NOT.
            let is_whitespace =
                grapheme.symbol == ZWSP
                || (grapheme.symbol.chars().all(char::is_whitespace) && grapheme.symbol != NBSP);
            let symbol_width = unicode_width::UnicodeWidthStr::width(grapheme.symbol) as u16;

            // ignore symbols wider than line limit
            if symbol_width > w {
                continue;
            }

            let word_found = non_whitespace_previous && is_whitespace;
            // current full word (including whitespace) would overflow (trim=false path)
            let untrimmed_overflow = line_width == 0
                && word_width + whitespace_width + symbol_width > w;

            // append finished segment to current line
            if word_found || untrimmed_overflow {
                // not trimming, so always append whitespace
                line_width += whitespace_width;
                line_width += word_width;
                whitespace_width = 0;
                word_width = 0;
            }

            // pending line fills up limit
            let line_full = line_width >= w;
            // pending word would overflow line limit
            let pending_word_overflow = symbol_width > 0
                && line_width + whitespace_width + word_width >= w;

            if line_full || pending_word_overflow {
                count += 1;
                line_width = 0;
                whitespace_width = 0;

                // don't count first whitespace toward next word
                if is_whitespace {
                    continue;
                }
            }

            if is_whitespace {
                whitespace_width += symbol_width;
            } else {
                word_width += symbol_width;
            }

            non_whitespace_previous = !is_whitespace;
        }

        // append remaining text parts
        if line_width == 0 && word_width == 0 && whitespace_width > 0 {
            count += 1;
        }
        line_width += whitespace_width;
        line_width += word_width;
        if line_width > 0 {
            count += 1;
        }

        // ratatui always emits at least one line per input Line
        if count == 0 {
            count += 1;
        }
    }

    count.max(1)
}

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
    max_scroll_cache: u16,
    /// Which panel has keyboard focus (Tab cycles)
    pub focus: FocusPanel,
    /// Scroll position in todo panel
    pub todo_scroll: u16,
    /// Cached max scroll for todo panel
    todo_max_scroll_cache: u16,
    /// Manual scroll offset for input box (user-driven)
    pub input_scroll: u16,
    /// Cached max scroll for input box
    input_max_scroll_cache: u16,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Receiver for messages from async workflows
    pub workflow_rx: mpsc::UnboundedReceiver<TuiMessage>,
    /// Sender for messages from async workflows (clone and pass to workflow runners)
    pub workflow_tx: mpsc::UnboundedSender<TuiMessage>,
    /// Whether a workflow is currently running
    pub workflow_running: bool,
    /// Configuration TOML for ABK workflows (Task 50)
    pub config_toml: Option<String>,
    /// Secrets for ABK workflows (Task 50)
    pub secrets: Option<std::collections::HashMap<String, String>>,
    /// Build info for ABK workflows (Task 50)
    pub build_info: Option<BuildInfo>,
    /// Resume info from the last completed task for session continuity
    pub resume_info: Option<ResumeInfo>,
    /// Latest todo list from LLM todowrite tool
    pub todo_lines: Vec<String>,
    /// Cached inner width of input box (characters per visual line)
    input_inner_width_cache: usize,
    /// Cached panel rectangles for mouse hit-testing (set during render)
    output_rect: Rect,
    todo_rect: Rect,
    input_rect: Rect,
    /// Whether mouse events are passed through to terminal (for native text selection)
    mouse_passthrough: bool,
    /// Cancellation token for aborting the current workflow
    cancel_token: CancellationToken,
    /// True while spawned workflow task is still alive (even if UI shows idle).
    /// Unlike workflow_running, stays true until ResumeInfo is received.
    workflow_busy: bool,
    /// Command buffered by user during cancellation wind-down.
    pending_command: Option<String>,
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
                "  Ctrl+O - Toggle mouse passthrough (select text)".to_string(),
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
            workflow_running: false,
            config_toml: None,
            secrets: None,
            build_info: None,
            resume_info: None,
            todo_lines: Vec::new(),
            input_inner_width_cache: 80,
            output_rect: Rect::default(),
            todo_rect: Rect::default(),
            input_rect: Rect::default(),
            mouse_passthrough: false,
            cancel_token: CancellationToken::new(),
            workflow_busy: false,
            pending_command: None,
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

        // Main async loop with tokio::select!
        loop {
            // Draw the UI
            terminal.draw(|f| self.render(f))?;

            // Use tokio::select! to handle both terminal events and workflow messages.
            // biased; prioritizes workflow messages over the 50ms event poll so
            // rapid output bursts (e.g. streaming API tokens) update the panel
            // without competing with idle poll timeouts.
            tokio::select! {
                biased;

                // Handle messages from async workflows (higher priority)
                msg = self.workflow_rx.recv() => {
                    if let Some(msg) = msg {
                        self.handle_workflow_message(msg);
                    }
                }

                // Handle terminal events (non-blocking poll)
                result = Self::poll_event() => {
                    if let Some(event) = result? {
                        self.handle_event(event)?;
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableBracketedPaste, DisableMouseCapture)?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Poll for terminal events asynchronously
    /// Uses tokio::task::spawn_blocking to avoid blocking the async runtime
    /// with synchronous crossterm event polling
    async fn poll_event() -> Result<Option<Event>> {
        // Spawn a blocking task to poll for events
        // This prevents the synchronous event::poll from blocking the Tokio runtime
        tokio::task::spawn_blocking(|| {
            // Poll with a short timeout to remain responsive
            if event::poll(std::time::Duration::from_millis(50))? {
                Ok(Some(event::read()?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    /// Handle a terminal event
    fn handle_event(&mut self, event: Event) -> Result<()> {
        // Handle bracketed paste: pasted text arrives as a single event,
        // newlines are replaced with spaces to prevent auto-submit.
        if let Event::Paste(text) = event {
            let sanitized = text.replace('\n', " ").replace('\r', "");
            for c in sanitized.chars() {
                let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                self.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            return Ok(());
        }

        // Handle mouse events: click to focus, scroll wheel to scroll panel
        if let Event::Mouse(mouse) = event {
            // In passthrough mode, ignore all mouse events (terminal handles them)
            if self.mouse_passthrough {
                return Ok(());
            }
            let col = mouse.column;
            let row = mouse.row;
            match mouse.kind {
                MouseEventKind::Down(_) => {
                    // Click sets focus to the panel under the cursor
                    if self.output_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Output;
                    } else if self.todo_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Todo;
                    } else if self.input_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Input;
                    }
                }
                MouseEventKind::ScrollUp => {
                    if self.output_rect.contains((col, row).into()) {
                        self.auto_scroll = false;
                        if self.scroll == u16::MAX {
                            self.scroll = self.max_scroll_cache;
                        }
                        self.scroll = self.scroll.saturating_sub(3);
                    } else if self.todo_rect.contains((col, row).into()) {
                        self.todo_scroll = self.todo_scroll.saturating_sub(3);
                    } else if self.input_rect.contains((col, row).into()) {
                        self.input_scroll = self.input_scroll.saturating_sub(1);
                    }
                }
                MouseEventKind::ScrollDown => {
                    if self.output_rect.contains((col, row).into()) {
                        if self.scroll == u16::MAX { return Ok(()); }
                        self.scroll = self.scroll.saturating_add(3);
                        if self.scroll >= self.max_scroll_cache {
                            self.auto_scroll = true;
                            self.scroll = u16::MAX;
                        }
                    } else if self.todo_rect.contains((col, row).into()) {
                        self.todo_scroll = self.todo_scroll.saturating_add(3)
                            .min(self.todo_max_scroll_cache);
                    } else if self.input_rect.contains((col, row).into()) {
                        self.input_scroll = self.input_scroll.saturating_add(1)
                            .min(self.input_max_scroll_cache);
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        if let Event::Key(key) = event {
            // Exit passthrough mode on any keypress — re-enable mouse capture
            if self.mouse_passthrough {
                execute!(std::io::stdout(), EnableMouseCapture).ok();
                self.mouse_passthrough = false;
                return Ok(());
            }

            // Global keys — work regardless of focus
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Esc => {
                    if self.workflow_running {
                        // Cancel the running workflow and return to idle.
                        // self.cancel_token is replaced with a fresh token each
                        // execute_command() so cancelling here is always safe.
                        // Immediately mark as not running so the UI is responsive
                        // even if the ABK workflow hasn't yielded yet.
                        self.cancel_token.cancel();
                        self.workflow_running = false;
                        self.output_lines.push("⏹ Cancelling...".to_string());
                        // workflow_busy stays true — prevents execute_command while old task finishes
                    } else {
                        self.should_quit = true;
                    }
                    return Ok(());
                }
                // Tab cycles focus: Input → Output → Todo → Input
                KeyCode::Tab => {
                    self.focus = match self.focus {
                        FocusPanel::Input  => FocusPanel::Output,
                        FocusPanel::Output => FocusPanel::Todo,
                        FocusPanel::Todo   => FocusPanel::Input,
                    };
                    return Ok(());
                }
                // Shift+Tab cycles backwards: Input → Todo → Output → Input
                KeyCode::BackTab => {
                    self.focus = match self.focus {
                        FocusPanel::Input  => FocusPanel::Todo,
                        FocusPanel::Todo   => FocusPanel::Output,
                        FocusPanel::Output => FocusPanel::Input,
                    };
                    return Ok(());
                }
                // Ctrl+O: toggle mouse passthrough for native text selection
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    execute!(std::io::stdout(), DisableMouseCapture).ok();
                    self.mouse_passthrough = true;
                    return Ok(());
                }
                _ => {}
            }

            // Focus-specific key handling
            match self.focus {
                FocusPanel::Output => self.handle_output_keys(key.code)?,
                FocusPanel::Todo   => self.handle_todo_keys(key.code)?,
                FocusPanel::Input  => self.handle_input_keys(key.code)?,
            }
        }
        Ok(())
    }

    /// Keys when Output panel is focused: scroll output
    fn handle_output_keys(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up => {
                self.auto_scroll = false;
                if self.scroll == u16::MAX {
                    self.scroll = self.max_scroll_cache;
                }
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                if self.scroll == u16::MAX { return Ok(()); }
                self.scroll = self.scroll.saturating_add(1);
                if self.scroll >= self.max_scroll_cache {
                    self.auto_scroll = true;
                    self.scroll = u16::MAX;
                }
            }
            KeyCode::PageUp => {
                self.auto_scroll = false;
                if self.scroll == u16::MAX {
                    self.scroll = self.max_scroll_cache;
                }
                self.scroll = self.scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                if self.scroll == u16::MAX { return Ok(()); }
                self.scroll = self.scroll.saturating_add(10);
                if self.scroll >= self.max_scroll_cache {
                    self.auto_scroll = true;
                    self.scroll = u16::MAX;
                }
            }
            KeyCode::Home => {
                self.auto_scroll = false;
                self.scroll = 0;
            }
            KeyCode::End => {
                self.auto_scroll = true;
                self.scroll = u16::MAX;
            }
            KeyCode::Char('y') => {
                self.copy_output_to_clipboard();
            }
            KeyCode::Enter => {
                if !self.input.is_empty() && !self.workflow_running {
                    self.focus = FocusPanel::Input;
                    self.execute_command();
                }
            }
            // Typing while output focused → switch to input and type there
            KeyCode::Char(c) => {
                self.focus = FocusPanel::Input;
                let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                self.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            _ => {}
        }
        Ok(())
    }

    /// Keys when Todo panel is focused: scroll todo list
    fn handle_todo_keys(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up => {
                self.todo_scroll = self.todo_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                self.todo_scroll = self.todo_scroll.saturating_add(1)
                    .min(self.todo_max_scroll_cache);
            }
            KeyCode::PageUp => {
                self.todo_scroll = self.todo_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.todo_scroll = self.todo_scroll.saturating_add(10)
                    .min(self.todo_max_scroll_cache);
            }
            KeyCode::Home => { self.todo_scroll = 0; }
            KeyCode::End => { self.todo_scroll = self.todo_max_scroll_cache; }
            // y = copy todo text to clipboard (must be before the generic Char catch-all)
            KeyCode::Char('y') => self.copy_to_clipboard(self.todo_lines.join("\n")),
            // Typing while todo focused → switch to input
            KeyCode::Char(c) if c != 'y' => {
                self.focus = FocusPanel::Input;
                let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                self.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            KeyCode::Enter => {
                if !self.input.is_empty() && !self.workflow_running {
                    self.focus = FocusPanel::Input;
                    self.execute_command();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Keys when Input panel is focused: edit text + scroll input
    fn handle_input_keys(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                if !self.input.is_empty() && !self.workflow_running {
                    self.execute_command();
                }
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    let byte_pos = char_to_byte_offset(&self.input, self.cursor_position - 1);
                    self.input.remove(byte_pos);
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Delete => {
                let char_count = self.input.chars().count();
                if self.cursor_position < char_count {
                    let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                    self.input.remove(byte_pos);
                }
            }
            KeyCode::Up => {
                // Move cursor up by one visual line width
                let w = self.input_inner_width_cache.max(1);
                if self.cursor_position >= w {
                    self.cursor_position -= w;
                } else {
                    self.cursor_position = 0;
                }
            }
            KeyCode::Down => {
                let w = self.input_inner_width_cache.max(1);
                let char_count = self.input.chars().count();
                self.cursor_position = (self.cursor_position + w).min(char_count);
            }
            KeyCode::PageUp => {
                self.input_scroll = self.input_scroll.saturating_sub(3);
            }
            KeyCode::PageDown => {
                self.input_scroll = self.input_scroll.saturating_add(3)
                    .min(self.input_max_scroll_cache);
            }
            KeyCode::Home => { self.cursor_position = 0; }
            KeyCode::End => { self.cursor_position = self.input.chars().count(); }
            KeyCode::Left => {
                if self.cursor_position > 0 { self.cursor_position -= 1; }
            }
            KeyCode::Right => {
                let char_count = self.input.chars().count();
                if self.cursor_position < char_count { self.cursor_position += 1; }
            }
            KeyCode::Char(c) => {
                let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                self.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            _ => {}
        }
        Ok(())
    }

    /// Copy output panel text to the system clipboard.
    fn copy_output_to_clipboard(&mut self) {
        // Strip the \x01 reasoning marker from each line before copying
        let clean: String = self.output_lines.iter()
            .map(|l| l.strip_prefix('\x01').unwrap_or(l).to_owned())
            .collect::<Vec<String>>()
            .join("\n");
        self.copy_to_clipboard(clean);
    }

    /// Copy a string to the system clipboard and show brief feedback.
    fn copy_to_clipboard(&mut self, text: String) {
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

    /// Handle messages from async workflows
    fn handle_workflow_message(&mut self, msg: TuiMessage) {
        match msg {
            TuiMessage::WorkflowCancelled => {
                self.output_lines.push("⏹ Workflow cancelled".to_string());
                self.output_lines.push("".to_string());
                self.workflow_running = false;
                // Don't set workflow_busy = false — ResumeInfo still coming
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
                self.workflow_running = false;
            }
            TuiMessage::WorkflowError(err) => {
                self.output_lines.push(format!("✗ Error: {}", err));
                self.output_lines.push("".to_string());
                self.workflow_running = false;
            }
            TuiMessage::TodoUpdate(content) => {
                self.todo_lines = content.lines().map(|l| l.to_string()).collect();
            }
            TuiMessage::ResumeInfo(info) => {
                self.resume_info = info;
                self.workflow_busy = false; // NOW fully idle
                if self.resume_info.is_some() {
                    if std::env::var("RUST_LOG")
                        .map(|v| v.to_lowercase().contains("debug"))
                        .unwrap_or(false)
                    {
                        self.output_lines.push("🔄 Session preserved — next command will continue this session".to_string());
                    }
                }
                // Auto-execute pending command if any
                if let Some(cmd) = self.pending_command.take() {
                    self.input = cmd;
                    self.execute_command();
                }
            }
        }
        // Auto-scroll to bottom when enabled
        if self.auto_scroll {
            self.scroll = u16::MAX;
        }
    }

    /// Execute the current command in the input buffer
    /// 
    /// Task 50: Wired to ABK's run_task_from_raw_config
    /// Task 55: Creates TuiSink to bridge OutputEvent → TuiMessage channel
    fn execute_command(&mut self) {
        let command = self.input.trim().to_string();
        
        // If previous workflow still winding down, buffer the command
        if self.workflow_busy {
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
        
        // Take resume_info (one-time use — consumed on next command)
        let resume_info = self.resume_info.take();
        
        // Mark workflow as running, re-enable auto-scroll
        self.workflow_running = true;
        self.workflow_busy = true;
        self.auto_scroll = true;

        // Create a fresh cancellation token for this workflow run.
        // Each command gets its own token so ESC cancelling one workflow
        // doesn't affect the next one (CancellationToken never un-cancel).
        self.cancel_token = CancellationToken::new();
        let child_token = self.cancel_token.clone();

        // Create channel for incremental resume_info from ABK checkpoints.
        // ABK sends resume_info after every iteration checkpoint so the TUI
        // always has up-to-date session state — even if ESC cancels mid-workflow.
        let (resume_tx, mut resume_rx) = mpsc::unbounded_channel();

        // Spawn a forwarder task that relays incremental resume_info
        // from ABK's checkpoint channel into the TUI message channel.
        let resume_forward_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(info) = resume_rx.recv().await {
                resume_forward_tx.send(TuiMessage::ResumeInfo(info)).ok();
            }
        });

        // Spawn the workflow with TuiSink-based output
        tokio::spawn(async move {
            // Create TuiSink that bridges OutputEvent → TuiMessage channel.
            let tui_sink: abk::orchestration::output::SharedSink =
                std::sync::Arc::new(TuiSink::new(tx.clone()));

            // Run ABK workflow with the task — bypasses CLI arg parsing.
            // TUI mode is enabled to suppress ABK's console output (stdout/stderr).
            // Output events flow through TuiSink directly to the TUI display.
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
            ).await;

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

    /// Render the TUI
    pub fn render(&mut self, frame: &mut Frame) {
        // Create main layout: output takes remaining space, input gets fixed height
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Min(0),    // Output + Todo area - all remaining space
                Constraint::Length(7), // Input area - fixed 7 rows (5 content + 2 borders)
            ])
            .split(frame.area());

        // Cache rects for mouse hit-testing
        self.input_rect = main_chunks[1];

        // Split output area horizontally: 70% output, 30% todo panel
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // Main output
                Constraint::Percentage(30), // Todo panel
            ])
            .split(main_chunks[0]);

        // Cache rects for mouse hit-testing
        self.output_rect = content_chunks[0];
        self.todo_rect = content_chunks[1];

        // Output area title shows scroll mode

        // Render output area with scrollable content.
        // Lines prefixed with \x01 are reasoning lines and rendered in dark grey.
        let grey_style = Style::default().fg(Color::DarkGray);
        let normal_style = Style::default();
        let styled_lines: Vec<Line> = self.output_lines.iter().flat_map(|raw| {
            let (style, text) = if let Some(stripped) = raw.strip_prefix('\x01') {
                (grey_style, stripped)
            } else {
                (normal_style, raw.as_str())
            };
            // A single output_line may contain embedded newlines (e.g. tool output).
            // Split them so ratatui wraps correctly.
            text.split('\n').map(move |segment| {
                Line::from(Span::styled(segment.to_string(), style))
            }).collect::<Vec<_>>()
        }).collect();

        let display_text = Text::from(styled_lines);
        // Use wrapped visual line count for scroll clamping (not raw line count).
        let content_height = estimate_visual_lines(&display_text, content_chunks[0].width);
        let viewport_height = content_chunks[0].height.saturating_sub(2) as usize;
        let max_scroll = content_height.saturating_sub(viewport_height) as u16;
        self.max_scroll_cache = max_scroll;
        let clamped_scroll = if self.scroll == u16::MAX {
            max_scroll
        } else {
            self.scroll.min(max_scroll)
        };
        let output_title = if self.auto_scroll {
            "Output (↑/↓ to scroll)".to_string()
        } else {
            format!("Output (line {}/{} — ↓ to follow)", clamped_scroll, max_scroll)
        };

        let output_border = if self.focus == FocusPanel::Output {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let output_paragraph = Paragraph::new(display_text)
            .block(
                Block::default()
                    .title(output_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(output_border),
            )
            .wrap(Wrap { trim: false })
            .scroll((clamped_scroll, 0));
        frame.render_widget(output_paragraph, content_chunks[0]);

        // Render todo panel on the right side
        let todo_title = format!("Todos ({})", self.todo_lines.len());
        let todo_text = if self.todo_lines.is_empty() {
            Text::from("No tasks")
        } else {
            Text::from(self.todo_lines.iter().map(|l| Line::from(l.as_str())).collect::<Vec<_>>())
        };
        let todo_content_height = estimate_visual_lines(&todo_text, content_chunks[1].width);
        let todo_viewport = content_chunks[1].height.saturating_sub(2) as usize;
        let todo_max = todo_content_height.saturating_sub(todo_viewport) as u16;
        self.todo_max_scroll_cache = todo_max;
        let todo_clamped = self.todo_scroll.min(todo_max);
        let todo_border = if self.focus == FocusPanel::Todo {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let todo_paragraph = Paragraph::new(todo_text)
            .block(
                Block::default()
                    .title(todo_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(todo_border),
            )
            .wrap(Wrap { trim: false })
            .scroll((todo_clamped, 0));
        frame.render_widget(todo_paragraph, content_chunks[1]);

        // Render input text with a visible block cursor (reversed colors).
        let char_count = self.input.chars().count();
        let cursor_style = if self.focus == FocusPanel::Input {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };
        let input_spans = if self.cursor_position < char_count {
            let before: String = self.input.chars().take(self.cursor_position).collect();
            let at: String = self.input.chars().skip(self.cursor_position).take(1).collect();
            let after: String = self.input.chars().skip(self.cursor_position + 1).collect();
            vec![
                Span::raw(before),
                Span::styled(at, cursor_style),
                Span::raw(after),
            ]
        } else {
            // Cursor at end — show a block space as the cursor
            vec![
                Span::raw(self.input.clone()),
                Span::styled(" ", cursor_style),
            ]
        };
        let input_text = Text::from(Line::from(input_spans));

        // Show status in input title
        let input_title = if self.workflow_running {
            "Input (Running... Esc to cancel)".to_string()
        } else {
            "Input (Ready)".to_string()
        };

        // Compute input scroll: auto-follow cursor, but allow manual override
        let input_inner_width = main_chunks[1].width.saturating_sub(2).max(1) as usize;
        self.input_inner_width_cache = input_inner_width;
        let input_inner_height = main_chunks[1].height.saturating_sub(2) as usize;
        let input_char_count = self.input.chars().count();
        let input_total_visual = if input_inner_width > 0 {
            ((input_char_count + input_inner_width - 1) / input_inner_width).max(1)
        } else { 1 };
        let input_max = input_total_visual.saturating_sub(input_inner_height) as u16;
        self.input_max_scroll_cache = input_max;
        // Auto-scroll to keep cursor visible
        let cursor_visual_line = if input_inner_width > 0 {
            (self.cursor_position / input_inner_width) as u16
        } else { 0 };
        if cursor_visual_line < self.input_scroll {
            self.input_scroll = cursor_visual_line;
        } else if cursor_visual_line >= self.input_scroll + input_inner_height as u16 {
            self.input_scroll = cursor_visual_line - input_inner_height as u16 + 1;
        }
        self.input_scroll = self.input_scroll.min(input_max);
        let input_border = if self.focus == FocusPanel::Input {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let input_paragraph = Paragraph::new(input_text)
            .block(
                Block::default()
                    .title(input_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(input_border),
            )
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false })
            .scroll((self.input_scroll, 0));
        frame.render_widget(input_paragraph, main_chunks[1]);

        // Render mouse passthrough banner if active
        if self.mouse_passthrough {
            let banner = Paragraph::new(Span::styled(
                "📋 Mouse passthrough — select text, press any key to return",
                Style::default().fg(Color::Yellow).bg(Color::DarkGray),
            ));
            let banner_area = Rect {
                x: frame.area().x,
                y: frame.area().y + frame.area().height.saturating_sub(1),
                width: frame.area().width,
                height: 1,
            };
            frame.render_widget(banner, banner_area);
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

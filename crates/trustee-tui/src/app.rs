//! TUI Application structure and main loop
//!
//! Task 52: Async TUI Loop
//! Converted from synchronous to async to allow concurrent workflow execution
//! with the TUI event loop using tokio::select!

use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, EnableBracketedPaste, DisableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use tokio::sync::mpsc;
use anyhow::Result;

use crate::tui_sink::TuiSink;
use abk::cli::ResumeInfo;

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

/// Estimate the number of visual (wrapped) lines a Text will occupy.
fn estimate_visual_lines(text: &Text, viewport_width: u16) -> usize {
    let w = viewport_width.saturating_sub(2).max(1) as usize;
    text.lines.iter().map(|line| {
        let chars: usize = line.spans.iter()
            .map(|s| s.content.chars().count())
            .sum();
        if chars <= w { 1 } else { (chars + w - 1) / w }
    }).sum()
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
                "  Enter - Execute task".to_string(),
                "  Esc or Ctrl+C - Exit".to_string(),
            ],
            scroll: 0,
            auto_scroll: true,
            max_scroll_cache: 0,
            should_quit: false,
            workflow_rx,
            workflow_tx,
            workflow_running: false,
            config_toml: None,
            secrets: None,
            build_info: None,
            resume_info: None,
            todo_lines: Vec::new(),
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
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main async loop with tokio::select!
        loop {
            // Draw the UI
            terminal.draw(|f| self.render(f))?;

            // Use tokio::select! to handle both terminal events and workflow messages
            tokio::select! {
                // Handle terminal events (non-blocking poll)
                result = Self::poll_event() => {
                    if let Some(event) = result? {
                        self.handle_event(event)?;
                    }
                }

                // Handle messages from async workflows
                msg = self.workflow_rx.recv() => {
                    if let Some(msg) = msg {
                        self.handle_workflow_message(msg);
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableBracketedPaste)?;
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

        if let Event::Key(key) = event {
            // Task 26: Enhanced keyboard event handling
            match key.code {
                // Exit with Ctrl+C
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true;
                }
                // Exit with Esc
                KeyCode::Esc => {
                    self.should_quit = true;
                }
                // Submit task with Enter
                KeyCode::Enter => {
                    if !self.input.is_empty() && !self.workflow_running {
                        self.execute_command();
                    }
                }
                // Task 24: Backspace - delete character before cursor
                KeyCode::Backspace => {
                    if self.cursor_position > 0 {
                        let byte_pos = char_to_byte_offset(&self.input, self.cursor_position - 1);
                        self.input.remove(byte_pos);
                        self.cursor_position -= 1;
                    }
                }
                // Delete key - delete character at cursor
                KeyCode::Delete => {
                    let char_count = self.input.chars().count();
                    if self.cursor_position < char_count {
                        let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                        self.input.remove(byte_pos);
                    }
                }
                // Scroll up
                KeyCode::Up => {
                    self.auto_scroll = false;
                    if self.scroll == u16::MAX {
                        self.scroll = self.max_scroll_cache;
                    }
                    self.scroll = self.scroll.saturating_sub(1);
                }
                // Scroll down
                KeyCode::Down => {
                    if self.scroll == u16::MAX {
                        return Ok(());
                    }
                    self.scroll = self.scroll.saturating_add(1);
                    if self.scroll >= self.max_scroll_cache {
                        self.auto_scroll = true;
                        self.scroll = u16::MAX;
                    }
                }
                // Page Up
                KeyCode::PageUp => {
                    self.auto_scroll = false;
                    if self.scroll == u16::MAX {
                        self.scroll = self.max_scroll_cache;
                    }
                    self.scroll = self.scroll.saturating_sub(10);
                }
                // Page Down
                KeyCode::PageDown => {
                    if self.scroll == u16::MAX {
                        return Ok(());
                    }
                    self.scroll = self.scroll.saturating_add(10);
                    if self.scroll >= self.max_scroll_cache {
                        self.auto_scroll = true;
                        self.scroll = u16::MAX;
                    }
                }
                // Task 24: Home - move cursor to beginning
                KeyCode::Home => {
                    self.cursor_position = 0;
                }
                // Task 24: End - move cursor to end
                KeyCode::End => {
                    self.cursor_position = self.input.chars().count();
                }
                // Task 24: Left arrow - move cursor left
                KeyCode::Left => {
                    if self.cursor_position > 0 {
                        self.cursor_position -= 1;
                    }
                }
                // Task 24: Right arrow - move cursor right
                KeyCode::Right => {
                    let char_count = self.input.chars().count();
                    if self.cursor_position < char_count {
                        self.cursor_position += 1;
                    }
                }
                // Task 24: Character input
                KeyCode::Char(c) => {
                    let byte_pos = char_to_byte_offset(&self.input, self.cursor_position);
                    self.input.insert(byte_pos, c);
                    self.cursor_position += 1;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Handle messages from async workflows
    fn handle_workflow_message(&mut self, msg: TuiMessage) {
        match msg {
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
                if self.resume_info.is_some() {
                    self.output_lines.push("🔄 Session preserved — next command will continue this session".to_string());
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
        
        // Clear welcome text and start fresh for this task
        self.output_lines.clear();
        self.scroll = 0;
        
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
        self.auto_scroll = true;
        
        // Spawn the workflow with TuiSink-based output
        tokio::spawn(async move {
            // Create TuiSink that bridges OutputEvent → TuiMessage channel.
            let tui_sink: abk::orchestration::output::SharedSink =
                std::sync::Arc::new(TuiSink::new(tx.clone()));

            // Run ABK workflow with the task — bypasses CLI arg parsing.
            // TUI mode is enabled to suppress ABK's console output (stdout/stderr).
            // Output events flow through TuiSink directly to the TUI display.
            abk::observability::set_tui_mode(true);

            let result: abk::cli::TaskResult = abk::cli::run_task_from_raw_config(
                &config_toml,
                secrets,
                build_info,
                &command,
                Some(tui_sink),
                resume_info,
            ).await.unwrap_or_else(|e| abk::cli::TaskResult {
                success: false,
                error: Some(e.to_string()),
                resume_info: None,
            });

            abk::observability::set_tui_mode(false);

            // Send completion message
            let msg = if result.success {
                TuiMessage::WorkflowCompleted
            } else {
                TuiMessage::WorkflowError(result.error.unwrap_or_default())
            };
            tx.send(msg).ok();

            // Send resume info back for storage in App
            tx.send(TuiMessage::ResumeInfo(result.resume_info)).ok();
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

        // Split output area horizontally: 70% output, 30% todo panel
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // Main output
                Constraint::Percentage(30), // Todo panel
            ])
            .split(main_chunks[0]);

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

        let output_paragraph = Paragraph::new(display_text)
            .block(
                Block::default()
                    .title(output_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
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
        let todo_paragraph = Paragraph::new(todo_text)
            .block(
                Block::default()
                    .title(todo_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(todo_paragraph, content_chunks[1]);

        // Task 24: Render input box with cursor tracking
        // Display input text with cursor position indicator
        let char_count = self.input.chars().count();
        let input_text = if self.cursor_position < char_count {
            // Cursor is in the middle - show cursor position with underline
            let before: String = self.input.chars().take(self.cursor_position).collect();
            let at: String = self.input.chars().skip(self.cursor_position).take(1).collect();
            let after: String = self.input.chars().skip(self.cursor_position + 1).collect();
            format!("{}{}{}", before, at, after)
        } else {
            // Cursor is at the end
            self.input.clone()
        };

        // Show status in input title
        let input_title = if self.workflow_running {
            format!("Input (Running...) - cursor: {}", self.cursor_position)
        } else {
            format!("Input (Ready) - cursor: {}", self.cursor_position)
        };

        // Compute input scroll to keep cursor visible in the input box
        let input_inner_width = main_chunks[1].width.saturating_sub(2).max(1) as usize;
        let input_inner_height = main_chunks[1].height.saturating_sub(2) as usize;
        let input_scroll = if input_inner_width > 0 && input_inner_height > 0 {
            let chars_before_cursor = self.cursor_position;
            let visual_cursor_line = chars_before_cursor / input_inner_width;
            if visual_cursor_line >= input_inner_height {
                (visual_cursor_line - input_inner_height + 1) as u16
            } else {
                0
            }
        } else {
            0
        };
        let input_paragraph = Paragraph::new(Text::from(input_text.as_str()))
            .block(
                Block::default()
                    .title(input_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false })
            .scroll((input_scroll, 0));
        frame.render_widget(input_paragraph, main_chunks[1]);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

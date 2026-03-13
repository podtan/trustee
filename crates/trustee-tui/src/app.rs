//! TUI Application structure and main loop
//!
//! Task 52: Async TUI Loop
//! Converted from synchronous to async to allow concurrent workflow execution
//! with the TUI event loop using tokio::select!

use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use tokio::sync::mpsc;
use anyhow::Result;

/// Messages that can be sent to the TUI from async workflows
#[derive(Debug, Clone)]
pub enum TuiMessage {
    /// A line of output to display
    OutputLine(String),
    /// Workflow completed
    WorkflowCompleted,
    /// Workflow error
    WorkflowError(String),
}

/// Build information for ABK (forward declaration)
pub type BuildInfo = abk::cli::BuildInfo;

/// Main application state for the TUI
pub struct App {
    /// Input buffer for user commands
    pub input: String,
    /// Cursor position in input buffer
    pub cursor_position: usize,
    /// Output log lines
    pub output_lines: Vec<String>,
    /// Scroll position in output (vertical)
    pub scroll: u16,
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
                "  Backspace - Delete character".to_string(),
                "  Esc or Ctrl+C - Exit".to_string(),
            ],
            scroll: 0,
            should_quit: false,
            workflow_rx,
            workflow_tx,
            workflow_running: false,
            config_toml: None,
            secrets: None,
            build_info: None,
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
        execute!(stdout, EnterAlternateScreen)?;
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
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
                        self.input.remove(self.cursor_position - 1);
                        self.cursor_position -= 1;
                    }
                }
                // Delete key - delete character at cursor
                KeyCode::Delete => {
                    if self.cursor_position < self.input.len() {
                        self.input.remove(self.cursor_position);
                    }
                }
                // Task 25: Scroll up with arrow up
                KeyCode::Up => {
                    if self.scroll > 0 {
                        self.scroll -= 1;
                    }
                }
                // Task 25: Scroll down with arrow down
                KeyCode::Down => {
                    let max_scroll = self.output_lines.len().saturating_sub(1) as u16;
                    if self.scroll < max_scroll {
                        self.scroll += 1;
                    }
                }
                // Task 25: Page Up - scroll up by 10 lines
                KeyCode::PageUp => {
                    self.scroll = self.scroll.saturating_sub(10);
                }
                // Task 25: Page Down - scroll down by 10 lines
                KeyCode::PageDown => {
                    let max_scroll = self.output_lines.len().saturating_sub(1) as u16;
                    self.scroll = (self.scroll + 10).min(max_scroll);
                }
                // Task 24: Home - move cursor to beginning
                KeyCode::Home => {
                    self.cursor_position = 0;
                }
                // Task 24: End - move cursor to end
                KeyCode::End => {
                    self.cursor_position = self.input.len();
                }
                // Task 24: Left arrow - move cursor left
                KeyCode::Left => {
                    if self.cursor_position > 0 {
                        self.cursor_position -= 1;
                    }
                }
                // Task 24: Right arrow - move cursor right
                KeyCode::Right => {
                    if self.cursor_position < self.input.len() {
                        self.cursor_position += 1;
                    }
                }
                // Task 24: Character input
                KeyCode::Char(c) => {
                    self.input.insert(self.cursor_position, c);
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
                // Auto-scroll to bottom on new output
                self.scroll = self.output_lines.len().saturating_sub(1) as u16;
            }
            TuiMessage::WorkflowCompleted => {
                self.output_lines.push("✓ Workflow completed".to_string());
                self.output_lines.push("".to_string());
                self.workflow_running = false;
                self.scroll = self.output_lines.len().saturating_sub(1) as u16;
            }
            TuiMessage::WorkflowError(err) => {
                self.output_lines.push(format!("✗ Error: {}", err));
                self.output_lines.push("".to_string());
                self.workflow_running = false;
                self.scroll = self.output_lines.len().saturating_sub(1) as u16;
            }
        }
    }

    /// Execute the current command in the input buffer
    /// 
    /// Task 50: Wired to ABK's run_task_from_raw_config
    /// Spawns an async workflow that sends output through TuiMessage.
    /// Tails the log file to capture ABK's output into the TUI display.
    fn execute_command(&mut self) {
        let command = self.input.trim().to_string();
        
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
        
        // Mark workflow as running
        self.workflow_running = true;
        
        // Determine log file path from config (default /tmp/trustee.log)
        let log_path = config_toml
            .parse::<toml::Value>()
            .ok()
            .and_then(|v| v.get("logging")?.get("log_file")?.as_str().map(String::from))
            .unwrap_or_else(|| "/tmp/trustee.log".to_string());
        
        // Spawn the workflow with log file tailing
        tokio::spawn(async move {
            tx.send(TuiMessage::OutputLine(format!("Executing: {}", command))).ok();
            
            // Record log file size before workflow starts so we only read new content
            let log_start_pos = std::fs::metadata(&log_path)
                .map(|m| m.len())
                .unwrap_or(0);
            
            // Spawn a log file tailer that reads new lines and sends to TUI
            let tail_tx = tx.clone();
            let tail_path = log_path.clone();
            let tail_handle = tokio::spawn(async move {
                let mut last_pos = log_start_pos;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    
                    let current_size = match std::fs::metadata(&tail_path) {
                        Ok(m) => m.len(),
                        Err(_) => continue,
                    };
                    
                    if current_size > last_pos {
                        if let Ok(file) = std::fs::File::open(&tail_path) {
                            use std::io::{Seek, BufRead, BufReader};
                            let mut reader = BufReader::new(file);
                            if reader.seek(std::io::SeekFrom::Start(last_pos)).is_ok() {
                                let mut line = String::new();
                                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                                    let trimmed = line.trim_end().to_string();
                                    if !trimmed.is_empty() {
                                        tail_tx.send(TuiMessage::OutputLine(trimmed)).ok();
                                    }
                                    line.clear();
                                }
                            }
                            last_pos = current_size;
                        }
                    }
                }
            });
            
            // Run ABK workflow with the task — bypasses CLI arg parsing
            // Redirect stdout to /dev/null to prevent raw mode corruption
            let result: Result<(), String> = {
                // Suppress stdout: ABK's tee-write sends output to both file and stdout,
                // but stdout is owned by ratatui in TUI mode. We only want the file output
                // (which the tailer reads). Redirect stdout to /dev/null during execution.
                use std::os::unix::io::AsRawFd;
                let devnull = std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/null")
                    .ok();
                let saved_stdout = unsafe { libc::dup(1) };
                if let Some(ref dn) = devnull {
                    unsafe { libc::dup2(dn.as_raw_fd(), 1); }
                }
                
                let res = abk::cli::run_task_from_raw_config(
                    &config_toml,
                    secrets,
                    build_info,
                    &command,
                ).await.map_err(|e| e.to_string());
                
                // Restore stdout
                if saved_stdout >= 0 {
                    unsafe { 
                        libc::dup2(saved_stdout, 1);
                        libc::close(saved_stdout);
                    }
                }
                
                res
            };
            
            // Give the tailer a moment to catch the last lines
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            tail_handle.abort();
            
            match result {
                Ok(()) => {
                    tx.send(TuiMessage::WorkflowCompleted).ok();
                }
                Err(e) => {
                    tx.send(TuiMessage::WorkflowError(format!("{}", e))).ok();
                }
            }
        });
        
        // Clear input buffer and reset cursor
        self.input.clear();
        self.cursor_position = 0;
        
        // Auto-scroll to bottom
        self.scroll = self.output_lines.len().saturating_sub(1) as u16;
    }

    /// Render the TUI
    pub fn render(&self, frame: &mut Frame) {
        // Task 23: Create main layout with 80/20 split
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Percentage(80), // Output area - 80%
                Constraint::Percentage(20), // Input area - 20%
            ])
            .split(frame.area());

        // Task 25: Render output area with scrollable content
        let output_text = self.output_lines.join("\n");
        let output_paragraph = Paragraph::new(Text::from(output_text))
            .block(
                Block::default()
                    .title("Output (↑/↓ or PgUp/PgDn to scroll)")
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .scroll((self.scroll, 0));
        frame.render_widget(output_paragraph, main_chunks[0]);

        // Task 23: Center the input box visually
        let input_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // Top padding for centering
                Constraint::Length(3), // Input box
            ])
            .split(main_chunks[1]);

        // Task 24: Render input box with cursor tracking
        // Display input text with cursor position indicator
        let input_text = if self.cursor_position < self.input.len() {
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

        let input_paragraph = Paragraph::new(Text::from(input_text.as_str()))
            .block(
                Block::default()
                    .title(input_title)
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .style(Style::default().fg(Color::White));
        frame.render_widget(input_paragraph, input_area[1]);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

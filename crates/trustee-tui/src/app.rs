//! TUI Application structure and main loop

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

/// Main application state for the TUI
pub struct App {
    /// Input buffer for user commands
    pub input: String,
    /// Output log lines
    pub output_lines: Vec<String>,
    /// Scroll position in output
    pub scroll: u16,
    /// Whether the app should quit
    pub should_quit: bool,
}

impl App {
    /// Create a new App instance
    pub fn new() -> Self {
        Self {
            input: String::new(),
            output_lines: vec![
                "Welcome to Trustee TUI".to_string(),
                "Type a task and press Enter to execute".to_string(),
                "Press Ctrl+C to exit".to_string(),
            ],
            scroll: 0,
            should_quit: false,
        }
    }

    /// Run the main event loop
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        loop {
            // Draw the UI
            terminal.draw(|f| self.render(f))?;

            // Handle events
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.should_quit = true;
                        }
                        KeyCode::Char(c) => {
                            self.input.push(c);
                        }
                        KeyCode::Enter => {
                            if !self.input.is_empty() {
                                self.execute_command();
                            }
                        }
                        KeyCode::Backspace => {
                            self.input.pop();
                        }
                        KeyCode::Esc => {
                            self.should_quit = true;
                        }
                        _ => {}
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

    /// Execute the current command in the input buffer
    fn execute_command(&mut self) {
        let command = self.input.trim().to_string();
        
        // Add command to output
        self.output_lines.push(format!("> {}", command));
        
        // TODO: Execute the actual workflow here
        // For now, just echo the command
        self.output_lines.push(format!("Command received: {}", command));
        
        // Clear input buffer
        self.input.clear();
        
        // Auto-scroll to bottom
        self.scroll = self.output_lines.len().saturating_sub(1) as u16;
    }

    /// Render the TUI
    pub fn render(&self, frame: &mut Frame) {
        // Create main layout: output area (top) + input box (bottom)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Min(10), // Output area
                Constraint::Length(3), // Input box
            ])
            .split(frame.area());

        // Render output area
        let output_text = self.output_lines.join("\n");
        let output_paragraph = Paragraph::new(Text::from(output_text))
            .block(
                Block::default()
                    .title("Output")
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .scroll((self.scroll, 0));
        frame.render_widget(output_paragraph, chunks[0]);

        // Render input box
        let input_paragraph = Paragraph::new(Text::from(self.input.as_str()))
            .block(
                Block::default()
                    .title("Input (press Enter to execute)")
                    .title_style(Style::default().add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .style(Style::default().fg(Color::White));
        frame.render_widget(input_paragraph, chunks[1]);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

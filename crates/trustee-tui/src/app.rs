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
    /// Cursor position in input buffer
    pub cursor_position: usize,
    /// Output log lines
    pub output_lines: Vec<String>,
    /// Scroll position in output (vertical)
    pub scroll: u16,
    /// Whether the app should quit
    pub should_quit: bool,
}

impl App {
    /// Create a new App instance
    pub fn new() -> Self {
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

            // Handle events with 100ms timeout
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
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
                            if !self.input.is_empty() {
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
        self.output_lines.push("".to_string()); // Empty line for spacing
        
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

        let input_paragraph = Paragraph::new(Text::from(input_text.as_str()))
            .block(
                Block::default()
                    .title(format!("Input (cursor: {})", self.cursor_position))
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

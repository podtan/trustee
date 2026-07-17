//! Rendering logic for the TUI.
//!
//! All `render()`, `render_zoomed()`, and `render_mcp_status()` methods
//! live here. The output panel uses manual line pre-wrapping and slicing
//! instead of `Paragraph` + `.wrap()` + `.scroll()` to eliminate the
//! orphan character bug (ratatui issue #2213/#2342).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::helpers::{
    build_visual_lines, estimate_visual_lines, slice_visible,
};
use crate::types::{FocusPanel, McpServerStatus};

impl App {
    /// Render the TUI
    pub fn render(&mut self, frame: &mut Frame) {
        // If a panel is zoomed, render only that panel fullscreen (no borders).
        if let Some(panel) = self.zoomed_panel {
            self.render_zoomed(frame, panel);
            return;
        }

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

        // ---- Output panel ----
        let grey_style = self.reasoning_style;
        let normal_style = Style::default();

        // Pre-wrap and flatten all output lines into visual lines.
        // Content width = panel width minus 2 border columns.
        let content_width = content_chunks[0].width.saturating_sub(2) as usize;
        let viewport_height = content_chunks[0].height.saturating_sub(2) as usize;
        let visual_lines = build_visual_lines(
            &self.output_lines,
            content_width,
            grey_style,
            normal_style,
        );

        let content_line_count = visual_lines.len();
        let max_scroll = content_line_count.saturating_sub(viewport_height) as u16;
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

        // Slice to only the visible window — no .wrap(), no .scroll().
        let visible_text = slice_visible(&visual_lines, viewport_height, clamped_scroll as usize);
        let output_paragraph = Paragraph::new(visible_text).block(
            Block::default()
                .title(output_title)
                .title_style(Style::default().add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(output_border),
        );
        frame.render_widget(output_paragraph, content_chunks[0]);

        // Split the right panel vertically: Todos on top, MCP status on bottom.
        let mcp_line_count = self.mcp_servers.len();
        let failed_count = self.mcp_servers
            .iter()
            .filter(|s| s.status == McpServerStatus::Failed && s.error.is_some())
            .count();
        let mcp_height = (2 + mcp_line_count + failed_count)
            .max(4)
            .min((content_chunks[1].height / 2) as usize) as u16;

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),             // Todos — takes remaining space
                Constraint::Length(mcp_height),  // MCP Status — dynamic!
            ])
            .split(content_chunks[1]);

        // Update todo_rect to only the todo portion for mouse hit-testing
        self.todo_rect = right_chunks[0];

        // ---- Todo panel ----
        let todo_title = format!("Todos ({})", self.todo_lines.len());
        let todo_width = right_chunks[0].width.saturating_sub(2) as usize;
        let todo_viewport = right_chunks[0].height.saturating_sub(2) as usize;

        let todo_visual_lines: Vec<Line> = if self.todo_lines.is_empty() {
            vec![Line::from("No tasks")]
        } else {
            self.todo_lines
                .iter()
                .flat_map(|l| {
                    let wrapped = crate::helpers::wrap_line(l, todo_width.max(1));
                    wrapped.into_iter().map(|w| Line::from(w))
                })
                .collect()
        };

        let todo_content_height = todo_visual_lines.len();
        let todo_max = todo_content_height.saturating_sub(todo_viewport) as u16;
        self.todo_max_scroll_cache = todo_max;
        let todo_clamped = self.todo_scroll.min(todo_max);

        let todo_border = if self.focus == FocusPanel::Todo {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let todo_visible = slice_visible(&todo_visual_lines, todo_viewport, todo_clamped as usize);
        let todo_paragraph = Paragraph::new(todo_visible).block(
            Block::default()
                .title(todo_title)
                .title_style(Style::default().add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(todo_border),
        );
        frame.render_widget(todo_paragraph, right_chunks[0]);

        // ---- MCP status panel ----
        self.mcp_rect = right_chunks[1];
        self.render_mcp_status(frame, right_chunks[1]);

        // ---- Input panel ----
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
            vec![Span::raw(self.input.clone()), Span::styled(" ", cursor_style)]
        };
        let input_text = Text::from(Line::from(input_spans));

        let input_title = match self.workflow_state {
            crate::types::WorkflowState::Running => "Input (Running... Esc to cancel)".to_string(),
            crate::types::WorkflowState::Cancelling => "Input (Cancelling...)".to_string(),
            crate::types::WorkflowState::Idle => "Input (Ready)".to_string(),
        };

        let input_inner_width = main_chunks[1].width.saturating_sub(2).max(1);
        self.input_inner_width_cache = input_inner_width as usize;
        let input_inner_height = main_chunks[1].height.saturating_sub(2) as usize;
        let input_total_visual = estimate_visual_lines(&input_text, main_chunks[1].width);
        let input_max = input_total_visual.saturating_sub(input_inner_height) as u16;
        self.input_max_scroll_cache = input_max;

        let cursor_text = if self.cursor_position < char_count {
            let before: String = self.input.chars().take(self.cursor_position + 1).collect();
            Text::from(Line::from(before))
        } else {
            input_text.clone()
        };
        let cursor_visual_line =
            estimate_visual_lines(&cursor_text, main_chunks[1].width).saturating_sub(1) as u16;
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
    }

    /// Render a single panel fullscreen with no borders or margins.
    ///
    /// Used when the user presses Ctrl+Z to zoom into a panel for clean
    /// terminal-native text selection (click-drag → OS copy shortcut).
    fn render_zoomed(&mut self, frame: &mut Frame, panel: FocusPanel) {
        let area = frame.area();

        match panel {
            FocusPanel::Output => {
                let grey_style = self.reasoning_style;
                let normal_style = Style::default();

                let content_width = area.width as usize;
                let viewport_height = area.height as usize;
                let visual_lines = build_visual_lines(
                    &self.output_lines,
                    content_width,
                    grey_style,
                    normal_style,
                );

                let content_line_count = visual_lines.len();
                let max_scroll = content_line_count.saturating_sub(viewport_height) as u16;
                self.max_scroll_cache = max_scroll;
                let clamped_scroll = if self.scroll == u16::MAX {
                    max_scroll
                } else {
                    self.scroll.min(max_scroll)
                };

                let visible_text = slice_visible(&visual_lines, viewport_height, clamped_scroll as usize);
                let paragraph = Paragraph::new(visible_text);
                frame.render_widget(Clear, area);
                frame.render_widget(paragraph, area);
            }
            FocusPanel::Todo => {
                let content_width = area.width as usize;
                let viewport_height = area.height as usize;

                let visual_lines: Vec<Line> = if self.todo_lines.is_empty() {
                    vec![Line::from("No tasks")]
                } else {
                    self.todo_lines
                        .iter()
                        .flat_map(|l| {
                            let wrapped = crate::helpers::wrap_line(l, content_width.max(1));
                            wrapped.into_iter().map(|w| Line::from(w))
                        })
                        .collect()
                };

                let content_line_count = visual_lines.len();
                let max_scroll = content_line_count.saturating_sub(viewport_height) as u16;
                self.todo_max_scroll_cache = max_scroll;
                let todo_clamped = self.todo_scroll.min(max_scroll);

                let visible_text = slice_visible(&visual_lines, viewport_height, todo_clamped as usize);
                let paragraph = Paragraph::new(visible_text);
                frame.render_widget(paragraph, area);
            }
            FocusPanel::Mcp => {
                let grey = Style::default().fg(Color::DarkGray);
                if self.mcp_servers.is_empty() {
                    let paragraph = Paragraph::new(Text::from(Line::from(
                        Span::styled("(none)", grey),
                    )));
                    frame.render_widget(paragraph, area);
                    return;
                }

                let mut lines: Vec<Line> = Vec::new();
                for s in &self.mcp_servers {
                    let (icon, color) = match s.status {
                        McpServerStatus::Connected => ("✓", Color::Green),
                        McpServerStatus::Failed => ("✗", Color::Red),
                    };
                    let count_str = if s.tool_count > 0 {
                        format!("{} tools", s.tool_count)
                    } else {
                        "--".to_string()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", icon), Style::default().fg(color)),
                        Span::raw(s.name.clone()),
                        Span::raw("  "),
                        Span::styled(count_str, grey),
                    ]));

                    if s.status == McpServerStatus::Failed {
                        if let Some(ref err) = s.error {
                            lines.push(Line::from(Span::styled(format!("  {}", err), grey)));
                        }
                    }
                }

                let content_lines = lines.len();
                let viewport_lines = area.height as usize;
                let max_scroll = content_lines.saturating_sub(viewport_lines) as u16;
                self.mcp_max_scroll_cache = max_scroll;
                let clamped_scroll = self.mcp_scroll.min(max_scroll);

                let paragraph = Paragraph::new(Text::from(lines)).scroll((clamped_scroll, 0));
                frame.render_widget(Clear, area);
                frame.render_widget(paragraph, area);
            }
            FocusPanel::Input => {
                let char_count = self.input.chars().count();
                let cursor_style = Style::default().fg(Color::Black).bg(Color::White);
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
                    vec![Span::raw(self.input.clone()), Span::styled(" ", cursor_style)]
                };
                let input_text = Text::from(Line::from(input_spans));
                let paragraph = Paragraph::new(input_text)
                    .style(Style::default().fg(Color::White))
                    .wrap(Wrap { trim: false })
                    .scroll((self.input_scroll, 0));
                frame.render_widget(paragraph, area);
            }
        }
    }

    /// Render the MCP server status panel (bottom of right column).
    ///
    /// Shows ✓/✗ icons + server name + tool count for each MCP server.
    /// Panel height is dynamic: grows with server count, capped at 50% of right column.
    /// Failed servers show a truncated error message on the line below.
    fn render_mcp_status(&mut self, frame: &mut Frame, area: Rect) {
        let connected = self.mcp_servers
            .iter()
            .filter(|s| s.status == McpServerStatus::Connected)
            .count();
        let mcp_title = format!("MCP ({}/{})", connected, self.mcp_servers.len());

        let grey = Style::default().fg(Color::DarkGray);
        let border_style = if self.focus == FocusPanel::Mcp {
            Style::default().fg(Color::Blue)
        } else {
            grey
        };
        let block = Block::default()
            .title(mcp_title)
            .title_style(Style::default().add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(border_style);

        if self.mcp_servers.is_empty() {
            let paragraph = Paragraph::new(Text::from(Line::from(Span::styled("(none)", grey))))
                .block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        for s in &self.mcp_servers {
            let (icon, color) = match s.status {
                McpServerStatus::Connected => ("✓", Color::Green),
                McpServerStatus::Failed => ("✗", Color::Red),
            };
            let count_str = if s.tool_count > 0 {
                format!("{} tools", s.tool_count)
            } else {
                "--".to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", icon), Style::default().fg(color)),
                Span::raw(s.name.clone()),
                Span::raw("  "),
                Span::styled(count_str, grey),
            ]));

            if s.status == McpServerStatus::Failed {
                if let Some(ref err) = s.error {
                    let max_len = (area.width as usize).saturating_sub(6);
                    let truncated = if err.len() > max_len {
                        format!("  {}", &err[..max_len.saturating_sub(1)])
                    } else {
                        format!("  {}", err)
                    };
                    lines.push(Line::from(Span::styled(truncated, grey)));
                }
            }
        }

        let content_lines = lines.len();
        let viewport_lines = area.height.saturating_sub(2) as usize;
        let max_scroll = content_lines.saturating_sub(viewport_lines) as u16;
        self.mcp_max_scroll_cache = max_scroll;
        let scroll = if self.focus == FocusPanel::Mcp {
            self.mcp_scroll.min(max_scroll)
        } else {
            max_scroll // auto-scroll to show latest entries
        };

        let paragraph = Paragraph::new(Text::from(lines))
            .block(block)
            .scroll((scroll, 0));
        frame.render_widget(paragraph, area);
    }
}

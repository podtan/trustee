//! Terminal event handling: keyboard, mouse, paste, and resize events.
//!
//! Key dispatch is based on `FocusPanel`:
//! - Output: scroll output, `y` to copy, typing redirects to input
//! - Todo: scroll todo, `y` to copy, typing redirects to input
//! - Mcp: scroll MCP list, typing redirects to input
//! - Input: text editing with cursor movement

use crossterm::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind, EnableMouseCapture, DisableMouseCapture},
    execute,
};
use anyhow::Result;

use crate::app::App;
use crate::helpers::char_to_byte_offset;
use crate::types::{FocusPanel, WorkflowState};

impl App {
    /// Handle a terminal event
    pub(crate) fn handle_event(&mut self, event: Event) -> Result<()> {
        // Handle terminal resize
        if let Event::Resize(_, _) = event {
            self.needs_clear = true;
            return Ok(());
        }

        // Handle bracketed paste
        if let Event::Paste(text) = event {
            let sanitized = text.replace('\n', " ").replace('\r', "");
            for c in sanitized.chars() {
                let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position);
                self.session.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            return Ok(());
        }

        // Handle mouse events
        if let Event::Mouse(mouse) = event {
            let col = mouse.column;
            let row = mouse.row;
            match mouse.kind {
                MouseEventKind::Down(_) => {
                    if self.output_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Output;
                    } else if self.todo_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Todo;
                    } else if self.mcp_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Mcp;
                    } else if self.input_rect.contains((col, row).into()) {
                        self.focus = FocusPanel::Input;
                    }
                }
                MouseEventKind::ScrollUp => {
                    if self.output_rect.contains((col, row).into()) {
                        self.session.auto_scroll = false;
                        if self.scroll == u16::MAX {
                            self.scroll = self.max_scroll_cache;
                        }
                        self.scroll = self.scroll.saturating_sub(3);
                    } else if self.todo_rect.contains((col, row).into()) {
                        self.todo_scroll = self.todo_scroll.saturating_sub(3);
                    } else if self.mcp_rect.contains((col, row).into()) {
                        self.mcp_scroll = self.mcp_scroll.saturating_sub(3);
                    } else if self.input_rect.contains((col, row).into()) {
                        self.input_scroll = self.input_scroll.saturating_sub(1);
                    }
                }
                MouseEventKind::ScrollDown => {
                    if self.output_rect.contains((col, row).into()) {
                        if self.scroll == u16::MAX {
                            return Ok(());
                        }
                        self.scroll = self.scroll.saturating_add(3);
                        if self.scroll >= self.max_scroll_cache {
                            self.session.auto_scroll = true;
                            self.scroll = u16::MAX;
                        }
                    } else if self.todo_rect.contains((col, row).into()) {
                        self.todo_scroll = self.todo_scroll
                            .saturating_add(3)
                            .min(self.todo_max_scroll_cache);
                    } else if self.mcp_rect.contains((col, row).into()) {
                        self.mcp_scroll = self.mcp_scroll
                            .saturating_add(3)
                            .min(self.mcp_max_scroll_cache);
                    } else if self.input_rect.contains((col, row).into()) {
                        self.input_scroll = self.input_scroll
                            .saturating_add(1)
                            .min(self.input_max_scroll_cache);
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

            // Global keys — work regardless of focus
            match key.code {
                // Ctrl+Z: toggle zoom
                KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.zoomed_panel.is_some() {
                        self.zoomed_panel = None;
                        execute!(std::io::stdout(), EnableMouseCapture).ok();
                    } else {
                        self.zoomed_panel = Some(self.focus);
                        execute!(std::io::stdout(), DisableMouseCapture).ok();
                    }
                    return Ok(());
                }
                // When zoomed, consume all other keys
                _ if self.zoomed_panel.is_some() => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.session.should_quit = true;
                    return Ok(());
                }
                KeyCode::Esc => {
                    if self.session.workflow_state == WorkflowState::Running {
                        self.session.cancel_token.cancel();
                        self.session.workflow_state = WorkflowState::Cancelling;
                        self.session.output_lines.push("⏹ Cancelling...".to_string());
                    } else if self.session.workflow_state == WorkflowState::Idle {
                        self.session.should_quit = true;
                    }
                    return Ok(());
                }
                // Tab cycles focus
                KeyCode::Tab => {
                    self.focus = match self.focus {
                        FocusPanel::Input  => FocusPanel::Output,
                        FocusPanel::Output => FocusPanel::Todo,
                        FocusPanel::Todo   => FocusPanel::Mcp,
                        FocusPanel::Mcp    => FocusPanel::Input,
                    };
                    return Ok(());
                }
                // Shift+Tab cycles backwards
                KeyCode::BackTab => {
                    self.focus = match self.focus {
                        FocusPanel::Input  => FocusPanel::Mcp,
                        FocusPanel::Mcp    => FocusPanel::Todo,
                        FocusPanel::Todo   => FocusPanel::Output,
                        FocusPanel::Output => FocusPanel::Input,
                    };
                    return Ok(());
                }
                // Ctrl+H: session handoff
                KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match self.session.workflow_state {
                        WorkflowState::Idle => self.trigger_handoff(self.session.input.trim().to_string()),
                        WorkflowState::Running => {
                            self.session.cancel_token.cancel();
                            self.session.workflow_state = WorkflowState::Cancelling;
                            self.session.handoff_pending = true;
                            self.session.output_lines.push("⏹ Cancelling before handoff...".to_string());
                        }
                        WorkflowState::Cancelling => {
                            self.session.handoff_pending = true;
                        }
                    }
                    return Ok(());
                }
                _ => {}
            }

            // Focus-specific key handling
            match self.focus {
                FocusPanel::Output => self.handle_output_keys(key.code)?,
                FocusPanel::Todo   => self.handle_todo_keys(key.code)?,
                FocusPanel::Mcp    => self.handle_mcp_keys(key.code)?,
                FocusPanel::Input  => self.handle_input_keys(key.code)?,
            }
        }
        Ok(())
    }

    /// Keys when Output panel is focused: scroll output
    fn handle_output_keys(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up => {
                self.session.auto_scroll = false;
                if self.scroll == u16::MAX {
                    self.scroll = self.max_scroll_cache;
                }
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                if self.scroll == u16::MAX {
                    return Ok(());
                }
                self.scroll = self.scroll.saturating_add(1);
                if self.scroll >= self.max_scroll_cache {
                    self.session.auto_scroll = true;
                    self.scroll = u16::MAX;
                }
            }
            KeyCode::PageUp => {
                self.session.auto_scroll = false;
                if self.scroll == u16::MAX {
                    self.scroll = self.max_scroll_cache;
                }
                self.scroll = self.scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                if self.scroll == u16::MAX {
                    return Ok(());
                }
                self.scroll = self.scroll.saturating_add(10);
                if self.scroll >= self.max_scroll_cache {
                    self.session.auto_scroll = true;
                    self.scroll = u16::MAX;
                }
            }
            KeyCode::Home => {
                self.session.auto_scroll = false;
                self.scroll = 0;
            }
            KeyCode::End => {
                self.session.auto_scroll = true;
                self.scroll = u16::MAX;
            }
            KeyCode::Char('y') => {
                self.copy_output_to_clipboard();
            }
            // Enter while output focused → switch to input and execute
            KeyCode::Enter => {
                if !self.session.input.is_empty() && self.session.workflow_state == WorkflowState::Idle {
                    self.focus = FocusPanel::Input;
                    self.execute_command();
                }
            }
            // Typing while output focused → switch to input
            KeyCode::Char(c) if c != 'y' => {
                if self.session.workflow_state != WorkflowState::Idle {
                    return Ok(());
                }
                self.focus = FocusPanel::Input;
                let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position);
                self.session.input.insert(byte_pos, c);
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
                self.todo_scroll = self.todo_scroll
                    .saturating_add(1)
                    .min(self.todo_max_scroll_cache);
            }
            KeyCode::PageUp => {
                self.todo_scroll = self.todo_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.todo_scroll = self.todo_scroll
                    .saturating_add(10)
                    .min(self.todo_max_scroll_cache);
            }
            KeyCode::Home => { self.todo_scroll = 0; }
            KeyCode::End => { self.todo_scroll = self.todo_max_scroll_cache; }
            KeyCode::Char('y') => self.copy_to_clipboard(self.session.todo_lines.join("\n")),
            // Typing while todo focused → switch to input
            KeyCode::Char(c) if c != 'y' => {
                if self.session.workflow_state != WorkflowState::Idle {
                    return Ok(());
                }
                self.focus = FocusPanel::Input;
                let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position);
                self.session.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            KeyCode::Enter => {
                if !self.session.input.is_empty() && self.session.workflow_state == WorkflowState::Idle {
                    self.focus = FocusPanel::Input;
                    self.execute_command();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Keys when MCP panel is focused: scroll MCP server list
    fn handle_mcp_keys(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up => {
                self.mcp_scroll = self.mcp_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                self.mcp_scroll = self.mcp_scroll
                    .saturating_add(1)
                    .min(self.mcp_max_scroll_cache);
            }
            KeyCode::PageUp => {
                self.mcp_scroll = self.mcp_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.mcp_scroll = self.mcp_scroll
                    .saturating_add(10)
                    .min(self.mcp_max_scroll_cache);
            }
            KeyCode::Home => { self.mcp_scroll = 0; }
            KeyCode::End => { self.mcp_scroll = self.mcp_max_scroll_cache; }
            // Typing while MCP focused → switch to input
            KeyCode::Char(c) => {
                if self.session.workflow_state != WorkflowState::Idle {
                    return Ok(());
                }
                self.focus = FocusPanel::Input;
                let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position);
                self.session.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            KeyCode::Enter => {
                if !self.session.input.is_empty() && self.session.workflow_state == WorkflowState::Idle {
                    self.focus = FocusPanel::Input;
                    self.execute_command();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Keys when Input panel is focused: text editing + cursor movement
    fn handle_input_keys(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                if !self.session.input.is_empty() && self.session.workflow_state == WorkflowState::Idle {
                    self.execute_command();
                }
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position - 1);
                    self.session.input.remove(byte_pos);
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Delete => {
                let char_count = self.session.input.chars().count();
                if self.cursor_position < char_count {
                    let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position);
                    self.session.input.remove(byte_pos);
                }
            }
            KeyCode::Up => {
                let w = self.input_inner_width_cache.max(1);
                if self.cursor_position >= w {
                    self.cursor_position -= w;
                } else {
                    self.cursor_position = 0;
                }
            }
            KeyCode::Down => {
                let w = self.input_inner_width_cache.max(1);
                let char_count = self.session.input.chars().count();
                self.cursor_position = (self.cursor_position + w).min(char_count);
            }
            KeyCode::PageUp => {
                self.input_scroll = self.input_scroll.saturating_sub(3);
            }
            KeyCode::PageDown => {
                self.input_scroll = self.input_scroll
                    .saturating_add(3)
                    .min(self.input_max_scroll_cache);
            }
            KeyCode::Home => { self.cursor_position = 0; }
            KeyCode::End => { self.cursor_position = self.session.input.chars().count(); }
            KeyCode::Left => {
                if self.cursor_position > 0 { self.cursor_position -= 1; }
            }
            KeyCode::Right => {
                let char_count = self.session.input.chars().count();
                if self.cursor_position < char_count { self.cursor_position += 1; }
            }
            KeyCode::Char(c) => {
                if self.session.workflow_state != WorkflowState::Idle {
                    return Ok(());
                }
                let byte_pos = char_to_byte_offset(&self.session.input, self.cursor_position);
                self.session.input.insert(byte_pos, c);
                self.cursor_position += 1;
            }
            _ => {}
        }
        Ok(())
    }
}

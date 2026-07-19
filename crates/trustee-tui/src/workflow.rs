//! Workflow handling — thin delegation to `Session` + TUI-only clipboard.
//!
//! All core workflow logic (handle_workflow_message, execute_command,
//! trigger_handoff) lives in [`trustee_core::session::Session`]. This module
//! provides wrapper methods on `App` that delegate to the session and then
//! sync TUI-specific state (auto-scroll, cursor reset).

use crate::app::App;
use trustee_core::types::TuiMessage;

impl App {
    /// Handle messages from async workflows — delegates to Session.
    pub(crate) fn handle_workflow_message(&mut self, msg: TuiMessage) {
        // Check if this is a HandoffReady before delegating (need to sync cursor after)
        let is_handoff = matches!(msg, TuiMessage::HandoffReady(_));

        self.session.handle_workflow_message(msg);

        // After HandoffReady, Session sets input to the briefing and calls
        // execute_command. We need to sync the cursor position.
        if is_handoff {
            self.cursor_position = self.session.input.chars().count();
        }

        // Sync auto-scroll: Session sets auto_scroll = true on new output,
        // and the TUI reads scroll as u16::MAX when auto_scroll is true.
        if self.session.auto_scroll {
            self.scroll = u16::MAX;
        }
    }

    /// Execute the current command — delegates to Session, syncs TUI cursor.
    pub(crate) fn execute_command(&mut self) {
        self.session.execute_command();
        self.cursor_position = 0;

        if self.session.auto_scroll {
            self.scroll = u16::MAX;
        }
    }

    /// Trigger a session handoff — delegates to Session.
    pub(crate) fn trigger_handoff(&mut self, hint: String) {
        self.session.trigger_handoff(hint);

        if self.session.auto_scroll {
            self.scroll = u16::MAX;
        }
    }

    /// Copy output panel text to the system clipboard.
    pub(crate) fn copy_output_to_clipboard(&mut self) {
        // Strip the \x01 reasoning marker from each line before copying
        let clean: String = self.session.output_lines.iter()
            .map(|l| l.strip_prefix('\x01').unwrap_or(l).to_owned())
            .collect::<Vec<String>>()
            .join("\n");
        self.copy_to_clipboard(clean);
    }

    /// Copy a string to the system clipboard and show brief feedback.
    pub(crate) fn copy_to_clipboard(&mut self, text: String) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => match clipboard.set_text(&text) {
                Ok(()) => {
                    self.session.output_lines.push("📋 Copied to clipboard".to_string());
                }
                Err(e) => {
                    self.session.output_lines.push(format!("✗ Clipboard error: {}", e));
                }
            },
            Err(e) => {
                self.session.output_lines.push(format!("✗ Clipboard unavailable: {}", e));
            }
        }
    }
}

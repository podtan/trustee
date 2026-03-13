//! Trustee TUI - Terminal User Interface for Trustee Agent
//!
//! This crate provides a terminal-based user interface for interacting with
//! the Trustee agent. It uses ratatui for rendering and crossterm for terminal
//! control.

mod app;

pub use app::App;

/// Run the TUI application
/// 
/// This function is synchronous and should be called from async context
pub fn run() -> anyhow::Result<()> {
    let mut app = App::new();
    app.run()
}

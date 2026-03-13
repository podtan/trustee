//! Trustee TUI - Terminal User Interface for Trustee Agent
//!
//! This crate provides a terminal-based user interface for interacting with
//! the Trustee agent. It uses ratatui for rendering and crossterm for terminal
//! control.

mod app;

pub use app::App;

/// Run the TUI application
pub fn run() -> anyhow::Result<()> {
    // TODO: Implement TUI entry point
    // For now, this is a placeholder that will be implemented in subsequent tasks
    
    // Create a tokio runtime for the async app
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut app = App::new();
        app.run().await
    })
}

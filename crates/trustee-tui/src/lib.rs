//! Trustee TUI - Terminal User Interface for Trustee Agent
//!
//! This crate provides a terminal-based user interface for interacting with
//! the Trustee agent. It uses ratatui for rendering and crossterm for terminal
//! control.

mod app;

pub use app::App;

use std::collections::HashMap;

/// Build information passed from the main binary
pub type BuildInfo = abk::cli::BuildInfo;

/// Run the TUI application with configuration
/// 
/// Task 50: This function accepts the merged configuration and secrets
/// and will wire them to ABK's run_from_raw_config for workflow execution.
/// 
/// This function is async to allow concurrent workflow execution with the TUI event loop.
pub async fn run(
    config_toml: String,
    secrets: HashMap<String, String>,
    build_info: BuildInfo,
) -> anyhow::Result<()> {
    let mut app = App::new();
    
    // Store config and secrets in the app for workflow execution
    app.config_toml = Some(config_toml);
    app.secrets = Some(secrets);
    app.build_info = Some(build_info);
    
    app.run().await
}

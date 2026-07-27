//! Trustee TUI - Terminal User Interface for Trustee Agent
//!
//! This crate provides a terminal-based user interface for interacting with
//! the Trustee agent. It uses ratatui for rendering and crossterm for terminal
//! control.

mod app;
mod event;
mod helpers;
mod render;
mod types;
mod workflow;

pub use app::App;
pub use types::TuiMessage;
pub use trustee_core::types::BuildInfo;

use std::collections::HashMap;

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

    // Store config and secrets in the session for workflow execution
    app.session.config_toml = Some(config_toml.clone());
    app.session.secrets = Some(secrets);
    app.session.build_info = Some(build_info);

    // Extract agent name from config for stateless operation
    if let Ok(table) = config_toml.parse::<toml::Value>() {
        if let Some(name) = table.get("agent").and_then(|a| a.get("name")).and_then(|n| n.as_str()) {
            app.session.agent_name = name.to_string();
        }
    }

    // Parse [tui.auto_handoff] settings from the merged config
    app.parse_auto_handoff_config();

    app.run().await
}

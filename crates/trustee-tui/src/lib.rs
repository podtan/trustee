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

use abk::cli::ResumeInfo;

/// Run the TUI application with configuration
///
/// Task 50: This function accepts the merged configuration and secrets
/// and will wire them to ABK's run_from_raw_config for workflow execution.
///
/// This function is async to allow concurrent workflow execution with the TUI event loop.
///
/// # Arguments
/// * `config_toml` - Merged TOML configuration string
/// * `secrets` - Key-value secrets map
/// * `build_info` - Build-time metadata
/// * `resume_info` - Optional checkpoint resume info (from `trustee resume -i`)
/// * `home_dir` - Optional per-user home directory for checkpoint isolation
/// * `identity` - Optional agent identity content prepended to the system prompt
pub async fn run(
    config_toml: String,
    secrets: HashMap<String, String>,
    build_info: BuildInfo,
    resume_info: Option<ResumeInfo>,
    home_dir: Option<std::path::PathBuf>,
    identity: Option<String>,
) -> anyhow::Result<()> {
    let mut app = App::new();

    // Store config and secrets in the session for workflow execution
    app.session.config_toml = Some(config_toml.clone());
    app.session.secrets = Some(secrets);
    app.session.build_info = Some(build_info);

    // Set per-user home_dir for checkpoint isolation
    app.session.home_dir = home_dir;

    // Set optional agent identity
    app.session.identity = identity;

    // Extract agent name from config for stateless operation
    if let Ok(table) = config_toml.parse::<toml::Value>() {
        if let Some(name) = table.get("agent").and_then(|a| a.get("name")).and_then(|n| n.as_str()) {
            app.session.agent_name = name.to_string();
        }
    }

    // Parse [tui.auto_handoff] settings from the merged config
    app.parse_auto_handoff_config();

    // Set resume_info if provided (from `trustee resume -i`)
    app.session.resume_info = resume_info;

    app.run().await
}

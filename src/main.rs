//! Trustee - A general-purpose agent that can morph into different specialized agents

use std::collections::HashMap;
use std::path::PathBuf;

/// Build-time metadata embedded by build.rs
fn build_info() -> abk::cli::BuildInfo {
    abk::cli::BuildInfo::new(
        option_env!("GIT_SHA"),
        option_env!("BUILD_DATE"),
        option_env!("RUSTC_VERSION"),
        option_env!("BUILD_PROFILE"),
    )
}

/// Load secrets from a .env file into a HashMap
/// 
/// Format: KEY=VALUE (one per line, # for comments)
fn load_env_file(path: &PathBuf) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut secrets = HashMap::new();
    
    if !path.exists() {
        return Ok(secrets); // Empty if file doesn't exist
    }
    
    let content = std::fs::read_to_string(path)?;
    for line in content.lines() {
        let line = line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        // Parse KEY=VALUE
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            // Remove surrounding quotes if present
            let value = value.trim_matches('"').trim_matches('\'').to_string();
            secrets.insert(key, value);
        }
    }
    
    Ok(secrets)
}

/// Get the paths for config and secrets based on agent name
fn get_config_paths(agent_name: &str) -> (PathBuf, PathBuf) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let share_dir = PathBuf::from(home).join(format!(".{}", agent_name));
    
    let config_path = share_dir.join("config").join(format!("{}.toml", agent_name));
    let secrets_path = share_dir.join(".env");
    
    (config_path, secrets_path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Determine agent name from the project config (for init) or use "trustee" as default
    let agent_name = "trustee";
    
    // Check if this is the init command (special case - use project config)
    let args: Vec<String> = std::env::args().collect();
    let is_init = args.get(1).map(|s| s.as_str()) == Some("init");
    
    if is_init {
        // Init command uses the old path-based approach to set up the environment
        abk::cli::run_configured_cli_from_config_with_build_info("config/trustee.toml", Some(build_info())).await
    } else {
        // All other commands: load config and secrets, pass to ABK
        let (config_path, secrets_path) = get_config_paths(agent_name);
        
        // Check if config exists
        if !config_path.exists() {
            eprintln!("Error: Configuration not found at: {}", config_path.display());
            eprintln!("\nRun 'trustee init --force' to set up your environment.");
            std::process::exit(1);
        }
        
        // Load config TOML
        let config_toml = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config from {}: {}", config_path.display(), e))?;
        
        // Load secrets from .env file
        let secrets = load_env_file(&secrets_path)
            .map_err(|e| format!("Failed to read secrets from {}: {}", secrets_path.display(), e))?;
        
        // Run with raw config (ABK does NOT read files)
        abk::cli::run_with_raw_config_and_build_info(&config_toml, secrets, Some(build_info())).await
    }
}

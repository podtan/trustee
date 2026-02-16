//! Trustee - A general-purpose agent that can morph into different specialized agents

use std::collections::HashMap;
use std::path::PathBuf;

use figment::providers::{Format, Toml};
use figment::Figment;
use getmyconfig::{ConfigReader, StorageConfig};

/// Embedded default configuration - compiled into the binary
const DEFAULT_CONFIG: &str = include_str!("../config/trustee_default.toml");

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
    parse_env_content(&content, &mut secrets);
    
    Ok(secrets)
}

/// Parse .env content into a HashMap (reusable for both local and remote .env files)
fn parse_env_content(content: &str, secrets: &mut HashMap<String, String>) {
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
}

/// Get the paths for config and secrets based on agent name
/// Returns (config_path, env_path, config_filename, env_filename)
fn get_config_paths(agent_name: &str) -> (PathBuf, PathBuf, String, String) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let share_dir = PathBuf::from(home).join(format!(".{}", agent_name));
    
    // Try to read local .env first to get custom file names
    let local_env_path = share_dir.join(".env");
    let mut config_filename = format!("{}.toml", agent_name);
    let mut env_filename = String::new();
    
    if local_env_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&local_env_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    match key {
                        "TRUSTEE_CONFIG_FILE" => config_filename = value.to_string(),
                        "TRUSTEE_ENV_FILE" => env_filename = value.to_string(),
                        _ => {}
                    }
                }
            }
        }
    }
    
    // If env file name not specified, use default .env
    if env_filename.is_empty() {
        env_filename = ".env".to_string();
    }
    
    let config_path = share_dir.join("config").join(&config_filename);
    let env_path = share_dir.join(&env_filename);
    
    (config_path, env_path, config_filename, env_filename)
}

/// Build a StorageConfig from GETMYCONFIG_* environment variables in the secrets map.
/// Returns None if the required variables are not set.
fn build_storage_config(secrets: &HashMap<String, String>) -> Option<StorageConfig> {
    let endpoint = secrets.get("GETMYCONFIG_ENDPOINT").filter(|s| !s.is_empty())?;
    let access_key = secrets.get("GETMYCONFIG_ACCESS_KEY").filter(|s| !s.is_empty())?;
    let secret_key = secrets.get("GETMYCONFIG_SECRET_KEY").filter(|s| !s.is_empty())?;
    let bucket = secrets.get("GETMYCONFIG_BUCKET").filter(|s| !s.is_empty())?;
    let encryption_key = secrets.get("GETMYCONFIG_ENCRYPTION_KEY").filter(|s| !s.is_empty())?;
    let region = secrets.get("GETMYCONFIG_REGION").filter(|s| !s.is_empty()).cloned();

    // Ensure endpoint has protocol
    let endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.clone()
    } else {
        format!("https://{}", endpoint)
    };

    Some(StorageConfig {
        endpoint,
        access_key: access_key.clone(),
        secret_key: secret_key.clone(),
        bucket: bucket.clone(),
        region,
        encryption_key: encryption_key.clone(),
    })
}

/// Try to load config and secrets from remote encrypted storage.
/// Returns (config_toml, secrets_env) on success, or None if remote is not configured/fails.
async fn load_remote_config(
    local_secrets: &HashMap<String, String>,
) -> Option<(String, HashMap<String, String>)> {
    let storage_config = build_storage_config(local_secrets)?;

    let reader = match ConfigReader::new(storage_config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[getmyconfig] Failed to create reader: {}", e);
            return None;
        }
    };

    // Read config file name from local secrets (default to "trustee.toml.enc")
    let config_file_name = local_secrets
        .get("GETMYCONFIG_CONFIG_FILE")
        .filter(|s| !s.is_empty())
        .unwrap_or(&"trustee.toml.enc".to_string())
        .clone();

    // Read env file name from local secrets (default to ".env.enc")
    let env_file_name = local_secrets
        .get("GETMYCONFIG_ENV_FILE")
        .filter(|s| !s.is_empty())
        .unwrap_or(&".env.enc".to_string())
        .clone();

    // Fetch and decrypt config file
    let config_toml = match reader.read_raw(&config_file_name).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => {
                eprintln!("[getmyconfig] ✓ Loaded {} from remote storage", config_file_name);
                s
            }
            Err(e) => {
                eprintln!("[getmyconfig] {} is not valid UTF-8: {}", config_file_name, e);
                return None;
            }
        },
        Err(e) => {
            eprintln!("[getmyconfig] Failed to read {}: {}", config_file_name, e);
            return None;
        }
    };

    // Fetch and decrypt env file
    let mut remote_secrets = HashMap::new();
    match reader.read_raw(&env_file_name).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(content) => {
                parse_env_content(&content, &mut remote_secrets);
                eprintln!(
                    "[getmyconfig] ✓ Loaded {} from remote storage ({} keys)",
                    env_file_name,
                    remote_secrets.len()
                );
            }
            Err(e) => {
                eprintln!("[getmyconfig] {} is not valid UTF-8: {}", env_file_name, e);
                return None;
            }
        },
        Err(e) => {
            eprintln!("[getmyconfig] Failed to read {}: {}", env_file_name, e);
            return None;
        }
    }

    Some((config_toml, remote_secrets))
}

/// Merge embedded defaults with user overrides using figment.
/// Returns the merged TOML string ready for ABK.
fn merge_config(user_config_toml: &str) -> Result<String, Box<dyn std::error::Error>> {
    let merged: toml::Table = Figment::new()
        .merge(Toml::string(DEFAULT_CONFIG))
        .merge(Toml::string(user_config_toml))
        .extract()
        .map_err(|e| format!("Failed to merge configuration: {}", e))?;

    let merged_toml = toml::to_string(&merged)
        .map_err(|e| format!("Failed to serialize merged config: {}", e))?;

    Ok(merged_toml)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Determine agent name from the project config (for init) or use "trustee" as default
    let agent_name = "trustee";
    
    // Check if this is the init command (special case - use project config)
    let args: Vec<String> = std::env::args().collect();
    let is_init = args.get(1).map(|s| s.as_str()) == Some("init");
    
    if is_init {
        // Init command uses the old path-based approach to set up the environment.
        // It reads config/trustee.toml (the minimal user config) from the project directory.
        // But we still need to merge with defaults so ABK gets a complete config.
        let project_config = std::fs::read_to_string("config/trustee.toml")
            .unwrap_or_default();
        let merged = merge_config(&project_config)?;
        let secrets = HashMap::new();
        abk::cli::run_from_raw_config(&merged, secrets, Some(build_info())).await
    } else {
        // All other commands: load config and secrets, pass to ABK
        let (config_path, secrets_path, _config_filename, _env_filename) = get_config_paths(agent_name);
        
        // Check if local config exists (needed as fallback and for GETMYCONFIG_* vars)
        if !config_path.exists() && !secrets_path.exists() {
            eprintln!("Error: Configuration not found at: {}", config_path.display());
            eprintln!("\nRun 'trustee init --force' to set up your environment.");
            std::process::exit(1);
        }
        
        // Load local .env first (contains GETMYCONFIG_* connection params)
        let local_secrets = load_env_file(&secrets_path)
            .map_err(|e| format!("Failed to read secrets from {}: {}", secrets_path.display(), e))?;
        
        // Try remote config first, fall back to local
        let (user_config_toml, secrets) = match load_remote_config(&local_secrets).await {
            Some((remote_config, remote_secrets)) => {
                // Merge: remote secrets take priority, but keep local GETMYCONFIG_* vars
                let mut merged = local_secrets.clone();
                merged.extend(remote_secrets);
                (remote_config, merged)
            }
            None => {
                // Fall back to local config
                if !config_path.exists() {
                    eprintln!("Error: Configuration not found at: {}", config_path.display());
                    eprintln!("Remote config also unavailable.");
                    eprintln!("\nRun 'trustee init --force' to set up your environment.");
                    std::process::exit(1);
                }
                
                let config_toml = std::fs::read_to_string(&config_path)
                    .map_err(|e| format!("Failed to read config from {}: {}", config_path.display(), e))?;
                
                eprintln!("[getmyconfig] Using local config fallback");
                (config_toml, local_secrets)
            }
        };
        
        // Merge embedded defaults with user overrides (from local or S3)
        let merged_config = merge_config(&user_config_toml)?;
        
        // Run with merged config (ABK does NOT read files)
        abk::cli::run_from_raw_config(&merged_config, secrets, Some(build_info())).await
    }
}

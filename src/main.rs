//! Trustee - A general-purpose agent that can morph into different specialized agents

use std::collections::HashMap;
use std::path::PathBuf;

use figment::providers::{Format, Toml};
use figment::Figment;
use getmyconfig::{ConfigReader, StorageConfig};

/// Embedded default configuration - compiled into the binary
const DEFAULT_CONFIG: &str = include_str!("../config/trustee_default.toml");

/// Embedded Cedar policy files - written to ~/.trustee/policies/ during init
const CEDAR_DEFAULT_POLICY: &str = include_str!("../config/policies/trustee_default.cedar");
const CEDAR_SCHEMA: &str = include_str!("../config/policies/trustee_schema.cedarschema");

/// Build-time metadata embedded by build.rs
fn build_info() -> abk::cli::BuildInfo {
    abk::cli::BuildInfo::new(
        option_env!("GIT_SHA"),
        option_env!("BUILD_DATE"),
        option_env!("RUSTC_VERSION"),
        option_env!("BUILD_PROFILE"),
    )
}

/// Debug logging helper — only prints when RUST_LOG contains "debug".
fn log_debug_main(msg: &str) {
    if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
        eprintln!("{}", msg);
    }
}

/// Load secrets from a .env file into a HashMap
/// 
/// Format: KEY=VALUE (one per line, # for comments)
/// 
/// After loading from file, merges process environment variables with
/// GETMYCONFIG_ prefix. This allows orchestrators like TRP to inject
/// remote config credentials via env vars (Turtle Zero bootstrapping).
/// Process env vars take precedence over file values.
fn load_env_file(path: &PathBuf) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut secrets = HashMap::new();
    
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        parse_env_content(&content, &mut secrets);
    }
    
    // Merge process environment variables so TRP (or other orchestrators)
    // can inject GETMYCONFIG_* credentials without touching the .env file.
    // Process env vars override file values for Turtle Zero bootstrapping.
    for (key, value) in std::env::vars() {
        if key.starts_with("GETMYCONFIG_") {
            secrets.insert(key, value);
        }
    }
    
    Ok(secrets)
}

/// Inject secrets into the process environment at startup.
///
/// This is called single-threaded before any concurrent access (web server,
/// TUI, CLI). It makes `${VAR}` references in config TOML resolvable.
/// Existing env vars take precedence (are NOT overwritten).
///
/// In web mode, per-user secrets are handled separately in
/// `apply_user_isolation()` — they never touch the process env.
fn inject_secrets_into_env(secrets: &HashMap<String, String>) {
    for (key, value) in secrets {
        if std::env::var(key).is_err() {
            std::env::set_var(key, value);
        }
    }
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
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
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

/// Compute a user hash from the OS username.
///
/// Returns SHA-256(username)[:16] as a hex string.
/// Used for per-user checkpoint isolation: ~/.trustee/users/{user_hash}/
fn compute_user_hash() -> String {
    use sha2::{Digest, Sha256};
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let mut hasher = Sha256::new();
    hasher.update(username.as_bytes());
    let result = hasher.finalize();
    format!("{:016x}", u64::from_be_bytes(result[..8].try_into().unwrap()))
}

/// Get the per-user home directory for checkpoint storage.
///
/// Returns ~/.trustee/users/{user_hash}/
fn get_user_home_dir() -> Option<std::path::PathBuf> {
    let user_hash = compute_user_hash();
    dirs::home_dir().map(|h| h.join(".trustee").join("users").join(&user_hash))
}

/// Find the most recently created session ID from the checkpoint storage.
///
/// Scans `{home_dir}/projects/{project_id}/sessions/` and returns the
/// session directory with the newest `session_metadata.json`.
fn find_latest_session_id(ctx: &abk::context::RunContext) -> Option<String> {
    let home_dir = ctx.resolve_home_dir().ok()?;
    let project_id = ctx.project.as_ref().map(|p| p.id.clone())
        .unwrap_or_else(|| {
            let cwd = std::env::current_dir().unwrap_or_default();
            abk::checkpoint::project_id_from_path(&cwd)
                .unwrap_or_else(|_| "default".to_string())
        });

    let sessions_dir = home_dir.join("projects").join(&project_id).join("sessions");
    if !sessions_dir.exists() {
        return None;
    }

    let mut latest: Option<(std::time::SystemTime, String)> = None;
    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let metadata_path = entry.path().join("session_metadata.json");
            if metadata_path.exists() {
                if let Ok(meta) = std::fs::metadata(&metadata_path) {
                    if let Ok(modified) = meta.modified() {
                        let sid = entry.file_name().to_string_lossy().to_string();
                        match &latest {
                            None => latest = Some((modified, sid)),
                            Some((prev, _)) if modified > *prev => latest = Some((modified, sid)),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    latest.map(|(_, sid)| sid)
}

/// Write Cedar policy files to ~/.{agent_name}/policies/ if they don't exist.
///
/// Creates the directory and writes default policy + schema files.
/// Existing files are NOT overwritten (user customizations preserved).
/// Called during `trustee init`.
fn deploy_cedar_policies(agent_name: &str) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let policies_dir = home.join(format!(".{}", agent_name)).join("policies");

    if let Err(e) = std::fs::create_dir_all(&policies_dir) {
        eprintln!("[init] Warning: Failed to create {}: {}", policies_dir.display(), e);
        return;
    }

    let policy_file = policies_dir.join("trustee_default.cedar");
    if !policy_file.exists() {
        if let Err(e) = std::fs::write(&policy_file, CEDAR_DEFAULT_POLICY) {
            eprintln!("[init] Warning: Failed to write {}: {}", policy_file.display(), e);
        } else {
            eprintln!("[init] Created: {}", policy_file.display());
        }
    }

    let schema_file = policies_dir.join("trustee_schema.cedarschema");
    if !schema_file.exists() {
        if let Err(e) = std::fs::write(&schema_file, CEDAR_SCHEMA) {
            eprintln!("[init] Warning: Failed to write {}: {}", schema_file.display(), e);
        } else {
            eprintln!("[init] Created: {}", schema_file.display());
        }
    }
}

/// Merge embedded defaults with user overrides using figment.
/// Returns the merged TOML string ready for ABK.
/// The binary version (from Cargo.toml at compile time) is always injected as the
/// highest-priority layer so [agent].version never needs to be set manually.
///
/// Config layering (lowest to highest priority):
/// 1. Embedded DEFAULT_CONFIG
/// 2. Global user config (~/.trustee/config.toml)
/// 3. Per-user config (~/.trustee/users/{user_hash}/config.toml) — if exists
/// 4. Binary version override
fn merge_config(user_config_toml: &str) -> Result<String, Box<dyn std::error::Error>> {
    let version_override = format!(
        "[agent]\nversion = \"{v}\"\n\n[cli]\nversion = \"{v}\"\n",
        v = env!("CARGO_PKG_VERSION")
    );

    let mut figment = Figment::new()
        .merge(Toml::string(DEFAULT_CONFIG))
        .merge(Toml::string(user_config_toml));

    // Layer per-user config overrides if they exist
    if let Some(user_home) = get_user_home_dir() {
        let user_config = user_home.join("config.toml");
        if user_config.exists() {
            if let Ok(per_user_toml) = std::fs::read_to_string(&user_config) {
                figment = figment.merge(Toml::string(&per_user_toml));
            }
        }
    }

    let merged: toml::Table = figment
        .merge(Toml::string(&version_override))
        .extract()
        .map_err(|e| format!("Failed to merge configuration: {}", e))?;

    let merged_toml = toml::to_string(&merged)
        .map_err(|e| format!("Failed to serialize merged config: {}", e))?;

    Ok(merged_toml)
}

/// Prepend agent identity to `lifecycle.system_template` in the config TOML.
///
/// Used in CLI mode where config goes directly to ABK (no Session layer).
/// On parse/serialize failure, returns the config unchanged (best-effort).
fn inject_identity_into_config(config_toml: String, identity: &str) -> String {
    let Ok(mut table) = config_toml.parse::<toml::Value>() else {
        return config_toml;
    };

    let lifecycle = table
        .get_mut("lifecycle")
        .and_then(|v| v.as_table_mut());

    if let Some(lifecycle) = lifecycle {
        let existing = lifecycle
            .get("system_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let combined = format!("{}\n\n{}", identity, existing);
        lifecycle.insert(
            "system_template".to_string(),
            toml::Value::String(combined),
        );
    } else {
        let mut ltable = toml::value::Table::new();
        ltable.insert(
            "system_template".to_string(),
            toml::Value::String(identity.to_string()),
        );
        if let Some(table) = table.as_table_mut() {
            table.insert("lifecycle".to_string(), toml::Value::Table(ltable));
        }
    }

    toml::to_string(&table).unwrap_or(config_toml)
}

/// Restore terminal to normal state — called from panic hook and signal handlers.
/// Uses `let _` so all steps run even if one fails.
#[cfg(feature = "tui")]
fn restore_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableBracketedPaste,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
    );
}

/// Setup panic hook to restore terminal state before showing panic message.
/// This is critical for TUI mode - if code panics while ratatui is in raw mode,
/// the terminal becomes unusable without this hook.
#[cfg(feature = "tui")]
fn setup_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        restore_terminal();
        original_hook(panic_info);
    }));
}

/// Run TUI mode when no arguments provided
#[cfg(feature = "tui")]
async fn run_tui_mode() -> Result<(), Box<dyn std::error::Error>> {
    // Set ABK_AGENT_NAME early so the global logger creates files in /tmp/trustee/
    // instead of the default /tmp/agent/. This MUST happen before Logger::new()
    // because Logger reads ABK_AGENT_NAME to determine the log directory.
    // Note: Kept for now until ABK fully removes the env var dependency.
    std::env::set_var("ABK_AGENT_NAME", "trustee");

    // Initialize ABK's global logger first so current_log_path() works
    // This ensures all tee_println() calls write to the same log file
    let logger = abk::observability::Logger::new(None, None)?;
    abk::observability::init_global_logger(logger);
    
    // Load config and secrets for TUI mode
    let agent_name = "trustee";
    let (config_path, secrets_path, _, _) = get_config_paths(agent_name);
    
    // Load local .env first (contains GETMYCONFIG_* connection params)
    let local_secrets = load_env_file(&secrets_path)
        .map_err(|e| format!("Failed to read secrets from {}: {}", secrets_path.display(), e))?;
    
    // Try remote config first, fall back to local
    let (user_config_toml, secrets) = match load_remote_config(&local_secrets).await {
        Some((remote_config, remote_secrets)) => {
            let mut merged = local_secrets.clone();
            merged.extend(remote_secrets);
            (remote_config, merged)
        }
        None => {
            if !config_path.exists() {
                eprintln!("Error: Configuration not found at: {}", config_path.display());
                eprintln!("Remote config also unavailable.");
                eprintln!("\nRun 'trustee init --force' to set up your environment.");
                std::process::exit(1);
            }
            
            let config_toml = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read config from {}: {}", config_path.display(), e))?;
            
            (config_toml, local_secrets)
        }
    };
    
    // Merge embedded defaults with user overrides
    let merged_config = merge_config(&user_config_toml)?;

    // Inject secrets into process env for ${VAR} substitution in config.
    // Safe: single-threaded, before any concurrent access.
    inject_secrets_into_env(&secrets);

    // Restore terminal cleanly on SIGTERM (e.g. `kill <pid>` from another terminal)
    #[cfg(unix)]
    tokio::spawn(async {
        if let Ok(mut sig) = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ) {
            sig.recv().await;
            restore_terminal();
            std::process::exit(0);
        }
    });

    // Launch the TUI application with config and per-user home_dir
    let user_home = get_user_home_dir();
    let identity = std::env::var("TRUSTEE_IDENTITY").ok().filter(|s| !s.is_empty());
    trustee_tui::run(merged_config, secrets, build_info(), None, user_home, identity).await?;
    Ok(())
}

/// Run resume-into-TUI mode: `trustee resume -i` or `trustee resume --session <id>`
///
/// Intercepts the resume command before it reaches ABK's CLI. Instead of
/// restoring + writing a file, it:
/// 1. Loads config/secrets
/// 2. Displays sessions (interactive) or resolves by session_id
/// 3. Resolves the latest checkpoint to ResumeInfo
/// 4. Launches the TUI with that ResumeInfo pre-loaded
#[cfg(feature = "tui")]
async fn run_resume_tui_mode(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use abk::cli::commands::resume::select_session_interactive;
    use abk::cli::runner::{AbkCheckpointAccess, RawConfigCommandContext};
    use abk::cli::{CheckpointAccess, CommandContext};

    std::env::set_var("ABK_AGENT_NAME", "trustee");
    let logger = abk::observability::Logger::new(None, None)?;
    abk::observability::init_global_logger(logger);

    // Load config (same as run_tui_mode)
    let agent_name = "trustee";
    let (config_path, secrets_path, _, _) = get_config_paths(agent_name);
    let local_secrets = load_env_file(&secrets_path)
        .map_err(|e| format!("Failed to read secrets from {}: {}", secrets_path.display(), e))?;
    let (user_config_toml, secrets) = match load_remote_config(&local_secrets).await {
        Some((remote_config, remote_secrets)) => {
            let mut merged = local_secrets.clone();
            merged.extend(remote_secrets);
            (remote_config, merged)
        }
        None => {
            let config_toml = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read config from {}: {}", config_path.display(), e))?;
            (config_toml, local_secrets)
        }
    };
    let merged_config = merge_config(&user_config_toml)?;

    // Inject secrets into process env for ${VAR} substitution in config.
    inject_secrets_into_env(&secrets);

    // Parse config for ABK
    let config: abk::config::Configuration = toml::from_str(&merged_config)
        .map_err(|e| format!("Failed to parse config TOML: {}", e))?;

    // Compute per-user home_dir for session discovery
    let user_home = get_user_home_dir();

    // Build context with RunContext so checkpoint access uses per-user home_dir
    let run_ctx = abk::context::RunContext::new().with_agent_name("trustee");
    let run_ctx = if let Some(ref home) = user_home {
        run_ctx.with_home_dir(home.clone())
    } else {
        run_ctx
    };
    let ctx = RawConfigCommandContext::with_agent_name(config, Some("trustee"))?
        .with_run_context(run_ctx);
    let checkpoint_access = match &user_home {
        Some(home) => AbkCheckpointAccess::with_config_and_home(ctx.config(), home.clone()),
        None => AbkCheckpointAccess::with_config(ctx.config()),
    };

    // Parse args: look for --session <id> or -s <id>
    let explicit_session_id: Option<&str> = {
        let mut iter = args.iter();
        let mut result = None;
        while let Some(arg) = iter.next() {
            if (arg == "--session" || arg == "-s") {
                if let Some(val) = iter.next() {
                    result = Some(val.as_str());
                }
            }
        }
        result
    };

    // Get session selection
    let (session_id, project_path) = if let Some(sid) = explicit_session_id {
        // Direct session_id provided — find it across all projects
        let projects = checkpoint_access.list_projects().await
            .map_err(|e| format!("Failed to list projects: {}", e))?;
        let mut found = None;
        for project in &projects {
            let sessions = checkpoint_access.list_sessions(&project.project_path).await
                .map_err(|e| format!("Failed to list sessions: {}", e))?;
            if sessions.iter().any(|s| s.session_id == sid) {
                found = Some((sid.to_string(), project.project_path.clone()));
                break;
            }
        }
        match found {
            Some(x) => x,
            None => {
                eprintln!("Session '{}' not found in any project.", sid);
                std::process::exit(1);
            }
        }
    } else {
        // Interactive selection
        match select_session_interactive(&ctx, &checkpoint_access).await? {
            Some(x) => x,
            None => return Ok(()), // User cancelled or no sessions
        }
    };

    // Resolve latest checkpoint
    let checkpoints = checkpoint_access.list_checkpoints(&project_path, &session_id).await
        .map_err(|e| format!("Failed to list checkpoints: {}", e))?;
    let latest = checkpoints.iter().max_by_key(|cp| cp.created_at)
        .ok_or_else(|| format!("No checkpoints found in session '{}'", session_id))?;

    eprintln!("🔄 Resuming session: {} (checkpoint: {}, iteration: {})",
        session_id, latest.checkpoint_id, latest.iteration);

    let resume_info = abk::cli::ResumeInfo {
        session_id: session_id.clone(),
        checkpoint_id: latest.checkpoint_id.clone(),
        iteration: latest.iteration as u32,
        project_path: Some(project_path),
    };

    // Launch TUI with resume_info and per-user home_dir
    let user_home = get_user_home_dir();
    let identity = std::env::var("TRUSTEE_IDENTITY").ok().filter(|s| !s.is_empty());
    trustee_tui::run(merged_config, secrets, build_info(), Some(resume_info), user_home, identity).await?;
    Ok(())
}

/// Run Web/API mode: starts the trustee-api HTTP + WebSocket server.
///
/// Usage: `trustee web [--addr 0.0.0.0:3000]`
#[cfg(feature = "web")]
async fn run_web_mode(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Parse optional arguments
    let mut addr: std::net::SocketAddr = "0.0.0.0:3000".parse().unwrap();
    let mut no_tls = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--addr" || args[i] == "-a" {
            if i + 1 < args.len() {
                addr = args[i + 1].parse().map_err(|e| format!("Invalid addr: {}", e))?;
                i += 2;
            } else {
                eprintln!("Error: --addr requires a value (e.g. 0.0.0.0:3000)");
                std::process::exit(1);
            }
        } else if args[i] == "--no-tls" {
            no_tls = true;
            i += 1;
        } else {
            i += 1;
        }
    }

    // Set ABK_AGENT_NAME for logging
    std::env::set_var("ABK_AGENT_NAME", "trustee");

    // Initialize logger
    let logger = abk::observability::Logger::new(None, None)?;
    abk::observability::init_global_logger(logger);

    // Initialize tracing subscriber for axum
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // Load config and secrets (same logic as TUI mode)
    let agent_name = "trustee";
    let (config_path, secrets_path, _, _) = get_config_paths(agent_name);

    let local_secrets = load_env_file(&secrets_path)
        .map_err(|e| format!("Failed to read secrets from {}: {}", secrets_path.display(), e))?;

    let (user_config_toml, secrets) = match load_remote_config(&local_secrets).await {
        Some((remote_config, remote_secrets)) => {
            let mut merged = local_secrets.clone();
            merged.extend(remote_secrets);
            (remote_config, merged)
        }
        None => {
            if !config_path.exists() {
                eprintln!("Error: Configuration not found at: {}", config_path.display());
                eprintln!("Remote config also unavailable.");
                eprintln!("\nRun 'trustee init --force' to set up your environment.");
                std::process::exit(1);
            }
            let config_toml = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read config from {}: {}", config_path.display(), e))?;
            (config_toml, local_secrets)
        }
    };

    let merged_config = merge_config(&user_config_toml)?;

    // Inject shared secrets into process env for ${VAR} substitution in config.
    // Safe: single-threaded, before web server starts accepting connections.
    // Per-user secrets are handled separately in apply_user_isolation().
    inject_secrets_into_env(&secrets);

    let scheme = if no_tls { "http" } else { "https" };
    eprintln!("🌐 Starting Trustee Web on {}://{}", scheme, addr);

    trustee_api::run(merged_config, secrets, build_info(), addr, !no_tls).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup panic hook for TUI mode (restores terminal on panic)
    #[cfg(feature = "tui")]
    setup_panic_hook();
    
    // Check if running without arguments - launch TUI if feature is enabled
    let args: Vec<String> = std::env::args().collect();
    #[cfg(feature = "tui")]
    if args.len() == 1 {
        // No arguments provided - launch TUI mode
        return run_tui_mode().await;
    }

    // Intercept the "web" command — starts the API + Web UI server
    #[cfg(feature = "web")]
    if args.get(1).map(|s| s.as_str()) == Some("web") {
        return run_web_mode(&args[2..]).await;
    }

    // Intercept "resume -i" / "resume --interactive" — launches TUI with resume_info
    #[cfg(feature = "tui")]
    if args.get(1).map(|s| s.as_str()) == Some("resume") {
        let has_interactive = args.iter().any(|a| a == "-i" || a == "--interactive");
        let has_session = args.iter().any(|a| a == "--session" || a == "-s");
        if has_interactive || has_session {
            return run_resume_tui_mode(&args[2..]).await;
        }
    }

    // Defensive: ensure terminal is not in raw mode from a previous TUI session
    // that was hard-killed (SIGKILL, crash, etc.) without restoring terminal state.
    #[cfg(feature = "tui")]
    let _ = crossterm::terminal::disable_raw_mode();
    
    // Determine agent name from the project config (for init) or use "trustee" as default
    let agent_name = "trustee";
    
    // Intercept the "upgrade" command — handled by trustee-upgrade, not ABK
    let is_upgrade = args.get(1).map(|s| s.as_str()) == Some("upgrade");
    if is_upgrade {
        return run_upgrade_command(&args[2..]).await;
    }

    // Check if this is the init command (special case - use project config)
    let is_init = args.get(1).map(|s| s.as_str()) == Some("init");
    
    if is_init {
        // Init command: prefer the user's existing config at ~/.trustee/config/trustee.toml
        // over the project's config/trustee.toml. This prevents `trustee init --force`
        // from overwriting user customizations (MCP servers, auto-handoff settings, etc.).
        let (installed_config_path, _, _, _) = get_config_paths(agent_name);

        // Deploy Cedar policy files to ~/.trustee/policies/ (non-destructive)
        deploy_cedar_policies(agent_name);

        let project_config = if installed_config_path.exists() {
            eprintln!("[init] Using existing config: {}", installed_config_path.display());
            std::fs::read_to_string(&installed_config_path)
                .unwrap_or_else(|_| {
                    std::fs::read_to_string("config/trustee.toml").unwrap_or_default()
                })
        } else {
            std::fs::read_to_string("config/trustee.toml").unwrap_or_default()
        };
        let merged = merge_config(&project_config)?;
        let secrets = HashMap::new();
        // Build RunContext with per-user home_dir
        let run_ctx = {
            let mut rc = abk::context::RunContext::new().with_agent_name("trustee");
            if let Some(ref home) = get_user_home_dir() {
                rc = rc.with_home_dir(home.clone());
            }
            rc
        };
        abk::cli::run_from_raw_config(&merged, secrets, Some(build_info()), Some(&run_ctx)).await
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
        let mut merged_config = merge_config(&user_config_toml)?;
        
        // Inject agent identity from env var if provided (CLI mode).
        // TUI/Web modes handle identity at the session level.
        if let Ok(identity) = std::env::var("TRUSTEE_IDENTITY") {
            if !identity.is_empty() {
                merged_config = inject_identity_into_config(merged_config, &identity);
            }
        }
        
        // Inject secrets into process env for ${VAR} substitution in config.
        // Safe: single-threaded CLI process.
        inject_secrets_into_env(&secrets);

        // Build RunContext with per-user home_dir
        let run_ctx = {
            let mut rc = abk::context::RunContext::new().with_agent_name("trustee");
            if let Some(ref home) = get_user_home_dir() {
                rc = rc.with_home_dir(home.clone());
            }
            rc
        };
        
        // Clone for title generation (secrets are consumed by run_from_raw_config)
        let title_secrets = secrets.clone();
        let title_config = merged_config.clone();
        let title_ctx = run_ctx.clone();
        
        // Extract task text from args for title generation
        let title_task: Option<String> = {
            let task_args: Vec<&str> = args.iter()
                .skip_while(|a| a != &"run")
                .skip(1) // skip "run" itself
                .map(|s| s.as_str())
                .collect();
            if task_args.is_empty() { None } else { Some(task_args.join(" ")) }
        };

        // Run with merged config (ABK does NOT read files)
        let run_result = abk::cli::run_from_raw_config(&merged_config, secrets, Some(build_info()), Some(&run_ctx)).await;
        
        // After successful run, generate and persist LLM session title (Solution B)
        if run_result.is_ok() {
            if let Some(ref task_text) = title_task {
                // Find the session_id from the most recently modified session
                if let Some(sid) = find_latest_session_id(&title_ctx) {
                    // Only generate if the title hasn't been LLM-set yet
                    if abk::cli::should_generate_title(&title_ctx, &sid, task_text).await {
                        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
                            eprintln!("[session] generating session title...");
                        }
                        match abk::cli::generate_session_title(&title_config, title_secrets, task_text).await {
                            Ok(Some(title)) => {
                                if let Err(e) = abk::cli::persist_session_title(&title_ctx, &title_config, &sid, &title).await {
                                    if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
                                        eprintln!("[session] title persist failed: {}", e);
                                    }
                                }
                            }
                            _ => {} // empty or error — keep default title
                        }
                    } else {
                        log_debug_main("[session] title already set, skipping generation");
                    }
                }
            }
        }
        
        run_result
    }
}

/// Handle the `trustee upgrade` command.
///
/// Parses upgrade-specific args and delegates to the trustee-upgrade crate.
async fn run_upgrade_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use clap::{Arg, ArgAction, Command};

    let cmd = Command::new("trustee upgrade")
        .about("Download and install the latest trustee release")
        .arg(
            Arg::new("check")
                .long("check")
                .help("Only check for updates without installing")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .short('f')
                .help("Force upgrade even if already up-to-date")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("version")
                .long("version-target")
                .short('v')
                .help("Upgrade to a specific version (e.g. 0.1.84)")
                .value_name("VERSION"),
        )
        .arg(
            Arg::new("repo")
                .long("repo")
                .help("GitHub repository (owner/repo) to download from")
                .value_name("REPO")
                .default_value("podtan/trustee"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("Show what would happen without making changes")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("prerelease")
                .long("prerelease")
                .help("Include pre-release versions")
                .action(ArgAction::SetTrue),
        );

    let matches = cmd.try_get_matches_from(std::iter::once("trustee upgrade".to_string()).chain(args.iter().cloned()))?;

    let opts = trustee_upgrade::UpgradeOptions {
        check_only: matches.get_flag("check"),
        force: matches.get_flag("force"),
        dry_run: matches.get_flag("dry-run"),
        prerelease: matches.get_flag("prerelease"),
        target_version: matches.get_one::<String>("version").map(|s| s.to_string()),
        repo: matches.get_one::<String>("repo").cloned(),
        current_version: env!("CARGO_PKG_VERSION").to_string(),
    };

    match trustee_upgrade::run_upgrade(opts).await {
        Ok(result) => {
            println!("{}", result.summary());
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Upgrade failed: {e}");
            std::process::exit(1);
        }
    }
}

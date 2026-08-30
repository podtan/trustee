//! THQ (Torpi) auto-registration.
//!
//! Two registration modes (16E):
//!
//! 1. **Per-agent-user** (the agents-as-users model): every directory under
//!    `~/.trustee/users/{hash}/` whose overlay config
//!    (`users/{hash}/config/trustee.toml`) carries a `[thq]` section is
//!    registered as its OWN agent in THQ — one process, N agents. Each entry
//!    gets a stable per-user id (`users/{hash}/agent_id`), an identity
//!    Bearer from that user's `.env` (attribution; the THQ registration
//!    route is open), and a heartbeat whose `status` reflects live session
//!    state (`idle` / `running`).
//!
//! 2. **Legacy single** (pre-0.12 behavior): when NO per-user `[thq]`
//!    entries exist but the process config has one, the whole process
//!    registers once under `[thq].agent_name` with id `~/.trustee/agent_id`.
//!
//! The per-user `[thq]` sections are read from the overlay FILES at boot.
//! Changing them requires a restart — MCP tool sets stay hot-reloadable,
//! THQ identity does not.
//!
//! Registration payloads match Torpi's `AgentEntry` and re-POST with the
//! same `id` is an upsert, which is exactly what the heartbeat does.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Configuration for THQ registration, parsed from a `[thq]` TOML section.
#[derive(Debug, Clone)]
pub struct ThqConfig {
    /// Base URL of the Torpi instance (e.g. <https://torpi.tanbal.ir>).
    pub torpi_url: String,
    /// This agent's externally-reachable URL (e.g. <https://192.168.1.10:3000>).
    pub advertise_url: String,
    /// Human-friendly agent name (e.g. "trustee-podtan").
    pub agent_name: String,
    /// Agent role (default: "general").
    pub agent_role: String,
    /// Hardcoded capabilities list.
    pub capabilities: Vec<String>,
    /// Hardcoded tags list.
    pub tags: Vec<String>,
    /// Re-registration interval in seconds (default: 30).
    pub heartbeat_interval: u64,
    /// Optional Bearer token for authenticating with the THQ API.
    /// Sent as `Authorization: Bearer <token>` on every registration request.
    /// When None, registration is unauthenticated (legacy Torpi instances).
    pub registration_token: Option<String>,
    /// Owning user's subject identifier (typically their JWT `sub` UUID).
    /// When set, Torpi associates this agent with the user so they can
    /// manage it through the THQ UI without admin privileges.
    /// Set from the `[thq] owner_id` config field.
    /// 16E: for agent-users this MUST be the agent's Kanidm `sub` — it is
    /// the key used to read live session state for the busy/idle heartbeat,
    /// matching the agent `user_key` pin (agents are keyed by `sub`).
    pub owner_id: Option<String>,
}

impl ThqConfig {
    /// Parse from a trustee config TOML (shared config or a per-user overlay).
    ///
    /// Reads the `[thq]` section. Returns `None` if the section is absent
    /// (registration disabled).  `torpi_url` and `advertise_url` are required;
    /// all other fields have defaults.
    pub fn from_toml(config_toml: &str) -> Option<Self> {
        let table: toml::Table = toml::from_str(config_toml).ok()?;
        let thq = table.get("thq")?.as_table()?;

        let torpi_url = thq
            .get("torpi_url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string())?;
        let advertise_url = thq
            .get("advertise_url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string())?;
        let agent_name = thq
            .get("agent_name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| {
                std::env::var("HOSTNAME")
                    .or_else(|_| std::env::var("COMPUTERNAME"))
                    .unwrap_or_else(|_| "trustee".to_string())
            });
        let agent_role = thq
            .get("agent_role")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();
        let capabilities = thq
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let tags = thq
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let heartbeat_interval = thq
            .get("heartbeat_interval")
            .and_then(|v| v.as_integer())
            .map(|v| v as u64)
            .unwrap_or(30);
        let registration_token = thq
            .get("registration_token")
            .and_then(|v| v.as_str())
            .map(String::from);
        let owner_id = thq
            .get("owner_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        Some(Self {
            torpi_url,
            advertise_url,
            agent_name,
            agent_role,
            capabilities,
            tags,
            heartbeat_interval,
            registration_token,
            owner_id,
        })
    }
}

/// Registration payload matching Torpi's `AgentEntry` struct.
#[derive(Debug, Serialize, Deserialize)]
struct AgentEntry {
    id: String,
    name: String,
    endpoint: String,
    role: String,
    capabilities: Vec<String>,
    status: String,
    tags: Vec<String>,
    last_seen: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_id: Option<String>,
}

/// One discovered agent-user (16E): a `users/{hash}/` home whose overlay
/// config carries a `[thq]` section.
#[derive(Debug, Clone)]
pub struct DiscoveredAgent {
    /// Hash directory name under `~/.trustee/users/`.
    pub user_hash: String,
    /// The user's home directory (`~/.trustee/users/{hash}/`).
    pub user_home: std::path::PathBuf,
    /// Parsed `[thq]` section from the user's overlay config.
    pub config: ThqConfig,
}

/// Scan `~/.trustee/users/*/config/trustee.toml` for per-user `[thq]`
/// sections. Deterministic order (sorted by hash) so startup logs and
/// heartbeat staggering are reproducible.
pub fn discover_user_agents() -> Vec<DiscoveredAgent> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    discover_user_agents_in(
        &std::path::PathBuf::from(home)
            .join(".trustee")
            .join("users"),
    )
}

/// Testable core of [`discover_user_agents`].
pub fn discover_user_agents_in(users_dir: &std::path::Path) -> Vec<DiscoveredAgent> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(users_dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let user_home = entry.path();
        if !user_home.is_dir() {
            continue;
        }
        let overlay = user_home.join("config").join("trustee.toml");
        let Ok(content) = std::fs::read_to_string(&overlay) else {
            continue;
        };
        if let Some(config) = ThqConfig::from_toml(&content) {
            found.push(DiscoveredAgent {
                user_hash: user_home
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                user_home,
                config,
            });
        }
    }
    found.sort_by(|a, b| a.user_hash.cmp(&b.user_hash));
    found
}

/// Resolve or create a persistent agent ID at an explicit file path (16E).
///
/// Reads from the file if present; otherwise generates a UUID v4, persists
/// it, and returns it. Per-agent identities live at
/// `users/{hash}/agent_id`; the legacy single identity at
/// `~/.trustee/agent_id`.
fn resolve_agent_id_at(id_file: &std::path::Path) -> Result<String, std::io::Error> {
    // Try existing
    if id_file.exists() {
        let id = std::fs::read_to_string(id_file)?;
        let id = id.trim().to_string();
        if !id.is_empty() {
            return Ok(id);
        }
    }

    // Generate new
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = id_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(id_file, &id)?;
    tracing::info!("Generated new agent ID: {} -> {}", id, id_file.display());
    Ok(id)
}

/// Legacy single-process id (`~/.trustee/agent_id`).
fn resolve_agent_id() -> Result<String, std::io::Error> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    resolve_agent_id_at(
        &std::path::PathBuf::from(home)
            .join(".trustee")
            .join("agent_id"),
    )
}

/// The issuer declared by the overlay's first `service-account` credential —
/// the origin the agent's service token was minted for (16F: exchange is
/// origin-bound, so it must travel with the entry).
fn read_overlay_service_issuer(user_home: &std::path::Path) -> Option<String> {
    let overlay = std::fs::read_to_string(user_home.join("config").join("trustee.toml")).ok()?;
    let v: toml::Value = overlay.parse().ok()?;
    let creds = v.get("mcp")?.get("credentials")?.as_table()?;
    for (_name, cred) in creds {
        if cred.get("type").and_then(|t| t.as_str()) == Some("service-account") {
            if let Some(issuer) = cred.get("issuer_url").and_then(|i| i.as_str()) {
                return Some(issuer.to_string());
            }
        }
    }
    None
}

/// Best-effort identity Bearer for a per-agent registration: first matching
/// key in the user's `.env`. The THQ registration route is OPEN — this is
/// attribution, not authentication. Unresolved `${VAR}` placeholders are
/// skipped (they mean the secret was never provisioned).
fn read_user_bearer(user_home: &std::path::Path) -> Option<String> {
    let env = std::fs::read_to_string(user_home.join(".env")).ok()?;
    const KEYS: [&str; 4] = [
        "THQ_SERVICE_TOKEN",
        "FAME_SERVICE_TOKEN",
        "FARZAN_SERVICE_ACCOUNT",
        "KANIDM_SERVICE_TOKEN",
    ];
    for key in KEYS {
        let prefix = format!("{key}=");
        for line in env.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix(&prefix) {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() && !value.starts_with("${") {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Busy = any of the user's sessions currently running.
///
/// `user_key` is the THQ `owner_id` (the agent's Kanidm `sub`) — the same
/// key the agent authenticates with, per the 16D agent user_key pin.
/// Session mutexes are tokio (async): Arc clones are collected and the
/// DashMap guard is dropped BEFORE any await.
async fn is_busy(state: &crate::state::ServerState, user_key: Option<&str>) -> bool {
    let Some(key) = user_key else {
        return false;
    };
    let Some(user) = state.sessions.get(key) else {
        return false;
    };
    let session_locks: Vec<_> = user
        .sessions
        .iter()
        .map(|e| e.value().session.clone())
        .collect();
    drop(user);
    for session in session_locks {
        if session.lock().await.workflow_state == trustee_core::types::WorkflowState::Running {
            return true;
        }
    }
    false
}

/// Build a reqwest client that accepts self-signed certs (Torpi may use them).
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client for THQ registration")
}

/// The registration/heartbeat loop: POST the entry immediately, then re-POST
/// every heartbeat interval. `status` is recomputed every tick from live
/// session state when `state` is provided. Failures are logged, never fatal.
async fn registration_loop(
    client: reqwest::Client,
    url: String,
    mut entry: AgentEntry,
    interval_secs: u64,
    bearer: Option<String>,
    status_key: Option<String>,
    state: Option<crate::state::ServerState>,
    label: String,
) {
    let mut first = true;
    loop {
        entry.status = match state.as_ref() {
            Some(state) => {
                if is_busy(state, status_key.as_deref()).await {
                    "running".to_string()
                } else {
                    "idle".to_string()
                }
            }
            None => "idle".to_string(),
        };
        entry.last_seen = chrono::Utc::now().to_rfc3339();

        let body = serde_json::to_value(&entry).unwrap_or_default();

        let mut request = client.post(&url).json(&body);
        if let Some(ref token) = bearer {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        match request.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    if first {
                        tracing::info!(
                            "THQ registration successful: {} at {}",
                            entry.name,
                            entry.endpoint
                        );
                        first = false;
                    } else {
                        tracing::debug!("THQ heartbeat successful: {}", label);
                    }
                } else {
                    let text = resp.text().await.unwrap_or_default();
                    tracing::warn!(
                        "THQ registration returned {}: {} ({})",
                        status,
                        text.chars().take(200).collect::<String>(),
                        label
                    );
                }
            }
            Err(e) => {
                if first {
                    tracing::warn!("THQ registration failed (will retry): {} ({})", e, label);
                } else {
                    tracing::debug!("THQ heartbeat failed: {} ({})", e, label);
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// Legacy single-process registration (pre-0.12 behavior). Used only when
/// NO per-user `[thq]` entries exist.
pub fn spawn(config: ThqConfig) {
    let agent_id = match resolve_agent_id() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(
                "Failed to resolve agent ID: {} — THQ registration disabled",
                e
            );
            return;
        }
    };

    tracing::info!(
        "THQ registration enabled: agent={} (id={}) -> {}",
        config.agent_name,
        agent_id,
        config.torpi_url
    );

    let entry = AgentEntry {
        id: agent_id,
        name: config.agent_name.clone(),
        endpoint: config.advertise_url.clone(),
        role: config.agent_role.clone(),
        capabilities: config.capabilities.clone(),
        status: "idle".to_string(),
        tags: config.tags.clone(),
        last_seen: chrono::Utc::now().to_rfc3339(),
        owner_id: config.owner_id.clone(),
    };

    let url = format!("{}/thq/api/agents", config.torpi_url);
    let bearer = config.registration_token.clone();

    tokio::spawn(registration_loop(
        build_http_client(),
        url,
        entry,
        config.heartbeat_interval,
        bearer,
        None,
        None,
        config.agent_name,
    ));
}

/// 16E: register EVERY agent-user found under `~/.trustee/users/` (per-user
/// `[thq]` overlays); fall back to the legacy single-process registration
/// only when none exist — so a legacy install keeps its exact behavior and
/// an agents-as-users install never double-registers a machine entry.
pub fn spawn_all(legacy: Option<ThqConfig>, state: crate::state::ServerState) {
    let agents = discover_user_agents();
    if agents.is_empty() {
        match legacy {
            Some(config) => {
                tracing::info!(
                    "THQ: no per-user [thq] entries under users/ — legacy single registration ({})",
                    config.agent_name
                );
                spawn(config);
            }
            None => tracing::debug!("THQ registration not configured (no [thq] section)"),
        }
        return;
    }

    tracing::info!(
        "THQ: registering {} agent-users from users/ (legacy_single=false)",
        agents.len()
    );
    let client = build_http_client();

    for agent in agents.iter() {
        let label = format!("{} ({})", agent.config.agent_name, agent.user_hash);
        let agent_id = match resolve_agent_id_at(&agent.user_home.join("agent_id")) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(
                    "THQ: cannot resolve agent id for {} — skipping: {}",
                    label,
                    e
                );
                continue;
            }
        };
        let bearer = read_user_bearer(&agent.user_home);
        let url = format!("{}/thq/api/agents", agent.config.torpi_url);

        // 16F: register the dispatch target so THQ-proxied sessions can be
        // impersonated AS this agent-user (see crate::xagent).
        if agent.config.owner_id.as_deref().map(str::len).unwrap_or(0) > 0 {
            state.thq_dispatch.insert(
                agent.config.agent_name.clone(),
                crate::state::ThqDispatchEntry {
                    user_key: agent.config.owner_id.clone().unwrap_or_default(),
                    service_token: bearer.clone(),
                    issuer_url: read_overlay_service_issuer(&agent.user_home),
                },
            );
        } else {
            tracing::warn!(
                "THQ: agent-user {} has no [thq].owner_id — NOT dispatchable (16F)",
                label
            );
        }

        let entry = AgentEntry {
            id: agent_id.clone(),
            name: agent.config.agent_name.clone(),
            endpoint: agent.config.advertise_url.clone(),
            role: agent.config.agent_role.clone(),
            capabilities: agent.config.capabilities.clone(),
            status: "idle".to_string(),
            tags: agent.config.tags.clone(),
            last_seen: chrono::Utc::now().to_rfc3339(),
            owner_id: agent.config.owner_id.clone(),
        };

        tracing::info!(
            "THQ: agent-user {} -> {} as id {} (owner={})",
            label,
            agent.config.torpi_url,
            agent_id,
            agent.config.owner_id.as_deref().unwrap_or("<none>")
        );

        tokio::spawn(registration_loop(
            client.clone(),
            url,
            entry,
            agent.config.heartbeat_interval,
            bearer,
            agent.config.owner_id.clone(),
            Some(state.clone()),
            label,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com"
advertise_url = "https://10.0.0.5:3000"
agent_name = "edge-paris"
agent_role = "code-review"
capabilities = ["rust", "docker"]
tags = ["edge", "arm64"]
heartbeat_interval = 60
"#;
        let cfg = ThqConfig::from_toml(toml).expect("should parse");
        assert_eq!(cfg.torpi_url, "https://torpi.example.com");
        assert_eq!(cfg.advertise_url, "https://10.0.0.5:3000");
        assert_eq!(cfg.agent_name, "edge-paris");
        assert_eq!(cfg.agent_role, "code-review");
        assert_eq!(cfg.capabilities, vec!["rust", "docker"]);
        assert_eq!(cfg.tags, vec!["edge", "arm64"]);
        assert_eq!(cfg.heartbeat_interval, 60);
        assert!(cfg.registration_token.is_none());
        assert!(cfg.owner_id.is_none());
    }

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com/"
advertise_url = "https://10.0.0.5:3000/"
"#;
        let cfg = ThqConfig::from_toml(toml).expect("should parse");
        assert_eq!(cfg.torpi_url, "https://torpi.example.com"); // trailing slash trimmed
        assert_eq!(cfg.advertise_url, "https://10.0.0.5:3000");
        assert_eq!(cfg.agent_role, "general");
        assert!(cfg.capabilities.is_empty());
        assert!(cfg.tags.is_empty());
        assert_eq!(cfg.heartbeat_interval, 30);
        assert!(cfg.registration_token.is_none());
        assert!(cfg.owner_id.is_none());
    }

    #[test]
    fn parse_registration_token() {
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com"
advertise_url = "https://10.0.0.5:3000"
registration_token = "secret-agent-token"
"#;
        let cfg = ThqConfig::from_toml(toml).expect("should parse");
        assert_eq!(
            cfg.registration_token.as_deref(),
            Some("secret-agent-token")
        );
    }

    #[test]
    fn parse_owner_id() {
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com"
advertise_url = "https://10.0.0.5:3000"
owner_id = "d0f5c4ba-9c10-4ff7-85a4-f2c0e588a55a"
"#;
        let cfg = ThqConfig::from_toml(toml).expect("should parse");
        assert_eq!(
            cfg.owner_id.as_deref(),
            Some("d0f5c4ba-9c10-4ff7-85a4-f2c0e588a55a")
        );
    }

    #[test]
    fn parse_empty_owner_id_is_none() {
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com"
advertise_url = "https://10.0.0.5:3000"
owner_id = ""
"#;
        let cfg = ThqConfig::from_toml(toml).expect("should parse");
        assert!(cfg.owner_id.is_none());
    }

    #[test]
    fn parse_no_section_returns_none() {
        let toml = r#"
[oidc]
issuer_url = "https://example.com"
"#;
        assert!(ThqConfig::from_toml(toml).is_none());
    }

    #[test]
    fn parse_missing_required_returns_none() {
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com"
"#;
        assert!(ThqConfig::from_toml(toml).is_none()); // missing advertise_url
    }

    #[test]
    fn parse_overlay_with_mcp_and_thq() {
        // 16E: the per-user overlay carries [mcp] AND [thq]; the parser must
        // find [thq] and ignore the rest.
        let toml = r#"
[mcp]
enabled = true

[mcp.credentials.x]
type = "service-account"
service_token = "${FAME_SERVICE_TOKEN}"

[[mcp.servers]]
name = "fame"
url = "https://fame.example/abc"
credentials = "x"

[thq]
torpi_url = "https://torpi.example.com"
advertise_url = "https://10.0.0.5:3000"
agent_name = "ravand"
owner_id = "1a71c077-b3b3-4581-b605-925c3f276f30"
"#;
        let cfg = ThqConfig::from_toml(toml).expect("should parse overlay");
        assert_eq!(cfg.agent_name, "ravand");
        assert_eq!(
            cfg.owner_id.as_deref(),
            Some("1a71c077-b3b3-4581-b605-925c3f276f30")
        );
    }

    #[test]
    fn agent_entry_serializes_correctly() {
        let entry = AgentEntry {
            id: "test-id".to_string(),
            name: "test".to_string(),
            endpoint: "https://localhost:3000".to_string(),
            role: "general".to_string(),
            capabilities: vec!["rust".to_string()],
            status: "idle".to_string(),
            tags: vec![],
            last_seen: "2025-01-01T00:00:00Z".to_string(),
            owner_id: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], "test-id");
        assert_eq!(v["status"], "idle");
        assert!(v["capabilities"].is_array());
        // owner_id should be absent when None (skip_serializing_if)
        assert!(v.get("owner_id").is_none());
    }

    #[test]
    fn agent_entry_serializes_owner_id() {
        let entry = AgentEntry {
            id: "test-id".to_string(),
            name: "test".to_string(),
            endpoint: "https://localhost:3000".to_string(),
            role: "general".to_string(),
            capabilities: vec!["rust".to_string()],
            status: "idle".to_string(),
            tags: vec![],
            last_seen: "2025-01-01T00:00:00Z".to_string(),
            owner_id: Some("user-uuid-123".to_string()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["owner_id"], "user-uuid-123");
    }

    // ── 16E tests ───────────────────────────────────────────────────────

    /// Unique temp dir without the tempfile dev-dependency.
    fn temp_users_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("trustee-thq-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn discover_finds_only_users_with_thq() {
        let base = temp_users_dir("discover");
        let thq_overlay = format!(
            "[mcp]\nenabled = true\n\n[thq]\ntorpi_url = \"https://torpi.example.com\"\nadvertise_url = \"https://10.0.0.5:3000\"\nagent_name = \"ravand\"\n"
        );
        for (hash, content) in [
            ("aaaa1111", thq_overlay.as_str()),
            ("bbbb2222", "[mcp]\nenabled = true\n"), // no thq
            ("cccc3333", "not toml at all {{{"),     // unparsable
        ] {
            let dir = base.join(hash).join("config");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("trustee.toml"), content).unwrap();
        }

        let found = discover_user_agents_in(&base);
        assert_eq!(found.len(), 1, "only the [thq]-bearing user is discovered");
        assert_eq!(found[0].user_hash, "aaaa1111");
        assert_eq!(found[0].config.agent_name, "ravand");
        assert_eq!(
            found[0].user_home,
            base.join("aaaa1111"),
            "user_home points at the hash dir"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_is_deterministic_and_sorted() {
        let base = temp_users_dir("sorted");
        for hash in ["dddd4444", "bbbb2222", "cccc3333"] {
            let dir = base.join(hash).join("config");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("trustee.toml"),
                "[thq]\ntorpi_url = \"https://t.example\"\nadvertise_url = \"https://10.0.0.5:3000\"\n",
            )
            .unwrap();
        }
        let found = discover_user_agents_in(&base);
        let hashes: Vec<&str> = found.iter().map(|a| a.user_hash.as_str()).collect();
        assert_eq!(hashes, vec!["bbbb2222", "cccc3333", "dddd4444"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_missing_dir_is_empty() {
        let base = temp_users_dir("missing");
        let found = discover_user_agents_in(&base.join("does-not-exist"));
        assert!(found.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_agent_id_at_is_stable_and_persists() {
        let base = temp_users_dir("agentid");
        let id_file = base.join("agent_id");

        let first = resolve_agent_id_at(&id_file).unwrap();
        assert!(!first.is_empty());
        assert!(id_file.exists(), "id persisted");

        let second = resolve_agent_id_at(&id_file).unwrap();
        assert_eq!(first, second, "id is stable across calls");

        // Whitespace-padded existing id is trimmed, not regenerated.
        std::fs::write(&id_file, format!("  {first}\n")).unwrap();
        let third = resolve_agent_id_at(&id_file).unwrap();
        assert_eq!(first, third);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn read_overlay_service_issuer_picks_service_account_credential() {
        let base = std::env::temp_dir().join(format!("trustee-thq-issuer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("config");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("trustee.toml"),
            "[mcp.credentials.fame_service]\ntype = \"service-account\"\nservice_token = \"${FAME_SERVICE_TOKEN}\"\nissuer_url = \"https://idp.tanbal.ir/oauth2/openid/pdt-api\"\n\n[mcp.credentials.interactive]\ntype = \"interactive\"\nissuer_url = \"https://ignored.example/\"\n",
        )
        .unwrap();
        let issuer = read_overlay_service_issuer(&base);
        assert_eq!(
            issuer.as_deref(),
            Some("https://idp.tanbal.ir/oauth2/openid/pdt-api"),
            "issuer must come from the service-account credential, not another type"
        );
        // No overlay file -> None.
        assert!(read_overlay_service_issuer(&base.join("nope")).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn read_user_bearer_priority_and_placeholder_skip() {
        let base = temp_users_dir("bearer");

        // No .env → None.
        assert!(read_user_bearer(&base).is_none());

        // Placeholder (unresolved secret) is skipped.
        std::fs::write(
            base.join(".env"),
            "FAME_SERVICE_TOKEN=${FAME_SERVICE_TOKEN}\n",
        )
        .unwrap();
        assert!(
            read_user_bearer(&base).is_none(),
            "unresolved placeholder skipped"
        );

        // Priority: THQ_SERVICE_TOKEN wins over FAME_SERVICE_TOKEN.
        std::fs::write(
            base.join(".env"),
            "FAME_SERVICE_TOKEN=fame-token\nTHQ_SERVICE_TOKEN=thq-token\n",
        )
        .unwrap();
        assert_eq!(read_user_bearer(&base).as_deref(), Some("thq-token"));

        // Quoted values are unwrapped; fallthrough to FAME works.
        std::fs::write(base.join(".env"), "FAME_SERVICE_TOKEN=\"fame-token\"\n").unwrap();
        assert_eq!(read_user_bearer(&base).as_deref(), Some("fame-token"));
        let _ = std::fs::remove_dir_all(&base);
    }
}

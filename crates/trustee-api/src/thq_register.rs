//! THQ (Torpi) auto-registration.
//!
//! When a `[thq]` section is present in the trustee config TOML, the web
//! server spawns a background task that registers this agent with the
//! specified Torpi instance and periodically re-registers (heartbeat).
//!
//! Registration is unauthenticated — Torpi's `/thq/api/agents/register`
//! endpoint accepts the POST without a Bearer token. When auth enforcement
//! is added later (NGHR fbfd4a11), the agent will send its OIDC token.
//!
//! The agent generates a stable UUID v4 on first run and persists it to
//! `~/.trustee/agent_id` so the same identity survives restarts.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Configuration for THQ registration, parsed from the `[thq]` TOML section.
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
}

impl ThqConfig {
    /// Parse from the merged trustee config TOML.
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

        Some(Self {
            torpi_url,
            advertise_url,
            agent_name,
            agent_role,
            capabilities,
            tags,
            heartbeat_interval,
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
}

/// Resolve or create a persistent agent ID.
///
/// Reads from `~/.trustee/agent_id`. If not found, generates a new UUID v4,
/// persists it, and returns it. This ensures the agent identity survives
/// process restarts.
fn resolve_agent_id() -> Result<String, std::io::Error> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let id_file = std::path::PathBuf::from(home)
        .join(".trustee")
        .join("agent_id");

    // Try existing
    if id_file.exists() {
        let id = std::fs::read_to_string(&id_file)?;
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
    std::fs::write(&id_file, &id)?;
    tracing::info!("Generated new agent ID: {} -> {}", id, id_file.display());
    Ok(id)
}

/// Build a reqwest client that accepts self-signed certs (Torpi may use them).
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client for THQ registration")
}

/// Spawn the registration background task.
///
/// On startup, immediately attempts to register. Then re-registers every
/// `heartbeat_interval` seconds. Failures are logged but never crash the
/// web server — the task retries indefinitely.
pub fn spawn(config: ThqConfig) {
    let agent_id = match resolve_agent_id() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to resolve agent ID: {} — THQ registration disabled", e);
            return;
        }
    };

    tracing::info!(
        "THQ registration enabled: agent={} (id={}) -> {}",
        config.agent_name,
        agent_id,
        config.torpi_url
    );

    let client = build_http_client();
    let entry = AgentEntry {
        id: agent_id.clone(),
        name: config.agent_name.clone(),
        endpoint: config.advertise_url.clone(),
        role: config.agent_role.clone(),
        capabilities: config.capabilities.clone(),
        status: "idle".to_string(),
        tags: config.tags.clone(),
        last_seen: chrono::Utc::now().to_rfc3339(),
    };

    let url = format!("{}/thq/api/agents", config.torpi_url);
    let interval_secs = config.heartbeat_interval;

    tokio::spawn(async move {
        let mut first = true;
        loop {
            let body = serde_json::to_value(&entry).unwrap_or_default();
            let mut body = body;
            body["last_seen"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());

            match client.post(&url).json(&body).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        if first {
                            tracing::info!("THQ registration successful: {} at {}", entry.name, entry.endpoint);
                            first = false;
                        } else {
                            tracing::debug!("THQ heartbeat successful");
                        }
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        tracing::warn!(
                            "THQ registration returned {}: {}",
                            status,
                            text.chars().take(200).collect::<String>()
                        );
                    }
                }
                Err(e) => {
                    if first {
                        tracing::warn!("THQ registration failed (will retry): {}", e);
                    } else {
                        tracing::debug!("THQ heartbeat failed: {}", e);
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    });
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
        };
        let json = serde_json::to_string(&entry).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], "test-id");
        assert_eq!(v["status"], "idle");
        assert!(v["capabilities"].is_array());
    }
}

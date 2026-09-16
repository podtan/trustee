//! 16F: per-agent THQ dispatch lane — boot-time population of
//! `ServerState.thq_dispatch`.
//!
//! This module is the dispatch half of the old torpi-era registration
//! module (`thq_register.rs`), EXTRACTED UNCHANGED by the 0.15.0
//! enrollment rewrite. The two concepts are now hard-separated:
//!
//! - **Enrollment** (`thq_register.rs`, v0.3 lane): the trustee PROCESS
//!   registers itself as a runtime against THQ. Process-level, one loop,
//!   zero coupling to users.
//! - **Dispatch** (THIS module): agents-as-users. Per-user `[thq]` overlay
//!   sections declare which agent-user a THQ-dispatched session must run
//!   AS (`owner_id` = the agent's Kanidm `sub`, `service_token` = the env
//!   var holding her service credential). The `xagent` router consumes the
//!   table built here. Registration-era keys (`torpi_url`, `advertise_url`)
//!   in overlays are INERT here — dispatch only reads its own three keys.
//!
//! Boot-only lifecycle: overlays are read once at boot (restart to change),
//! exactly as before the rewrite.

use crate::state::{ServerState, ThqDispatchEntry};

/// Per-user `[thq]` overlay fields the DISPATCH lane reads.
///
/// Deliberately minimal: the dispatch lane keys on `agent_name`, runs AS
/// `owner_id`, and needs the declared `service_token` to be dispatchable.
/// Everything else in the overlay is another lane's business.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    /// The dispatch key — also the `xagent` path segment.
    pub agent_name: String,
    /// The agent's Kanidm `sub` (16E sub-pin): session bucket, per-user
    /// home, MCP loader cache, and Cedar principal all resolve through it.
    pub owner_id: Option<String>,
    /// Issue 8e0a1215: the env var holding her service credential
    /// (`service_token = "${PAYDAR_SERVICE_ACCOUNT}"` or bare `KEY`).
    pub service_token: Option<String>,
}

impl DispatchConfig {
    /// Parse the `[thq]` section of a per-user overlay. Returns `None` if
    /// the section is absent (user is not a dispatch target). Only the
    /// three dispatch keys are read; registration-era keys are ignored.
    pub fn from_toml(config_toml: &str) -> Option<Self> {
        let table: toml::Table = toml::from_str(config_toml).ok()?;
        let thq = table.get("thq")?.as_table()?;
        let agent_name = thq
            .get("agent_name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| {
                std::env::var("HOSTNAME")
                    .or_else(|_| std::env::var("COMPUTERNAME"))
                    .unwrap_or_else(|_| "trustee".to_string())
            });
        let owner_id = thq
            .get("owner_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let service_token = thq
            .get("service_token")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Some(Self {
            agent_name,
            owner_id,
            service_token,
        })
    }
}

/// One discovered agent-user: a `users/{hash}/` home whose overlay config
/// carries a `[thq]` section.
#[derive(Debug, Clone)]
pub struct DiscoveredAgent {
    /// Hash directory name under `~/.trustee/users/`.
    pub user_hash: String,
    /// The user's home directory (`~/.trustee/users/{hash}/`).
    pub user_home: std::path::PathBuf,
    /// Parsed `[thq]` section from the user's overlay config.
    pub config: DispatchConfig,
}

/// Scan `~/.trustee/users/*/config/trustee.toml` for per-user `[thq]`
/// sections. Deterministic order (sorted by hash) so startup logs are
/// reproducible.
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
        if let Some(config) = DispatchConfig::from_toml(&content) {
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

/// The issuer declared by the overlay's first `service-account` credential —
/// the origin the agent's service token was minted for (16F: exchange is
/// origin-bound, so it must travel with the credential).
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

/// 16F: every service-account issuer declared by the discovered agent-user
/// overlays — the vhosts the deployed agents' tokens are actually minted on.
pub fn discover_service_issuers() -> Vec<String> {
    let mut out = Vec::new();
    for agent in discover_user_agents() {
        if let Some(issuer) = read_overlay_service_issuer(&agent.user_home) {
            if !out.contains(&issuer) {
                out.push(issuer);
            }
        }
    }
    out
}

/// Issue 8e0a1215: outcome of resolving an agent-user's dispatch credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceTokenResolution {
    /// `[thq].service_token` declared and resolved from the agent's `.env`.
    /// The ONLY way an agent becomes dispatchable — the hardcoded key scan
    /// (THQ/FAME/FARZAN/KANIDM_SERVICE_TOKEN) was removed in api 0.13.0.
    Resolved(String),
    /// No `[thq].service_token` declared at all. The agent is NOT
    /// dispatchable; with an `owner_id` present (dispatch intent) boot
    /// FAILS LOUD — an undeclared credential is the same silent-skip shape
    /// this issue was filed to kill.
    Undeclared,
    /// `[thq].service_token` DECLARED but its variable did not resolve from
    /// the agent's `.env` (missing file, missing key, or an unresolved
    /// `${…}` placeholder value). Boot FAILS LOUD naming agent + variable;
    /// the agent is NOT dispatchable. Never a silent skip.
    DeclaredUnresolved { var: String },
}

/// Extract the env-var name from a `[thq].service_token` declaration.
/// Accepts the canonical `"${KEY}"` wrapper and a bare `"KEY"`. Malformed
/// input returns the raw trimmed string — the lookup then fails and the
/// boot error names exactly what the config said.
fn declared_service_var(decl: &str) -> String {
    let s = decl.trim();
    match s.strip_prefix("${").and_then(|r| r.strip_suffix('}')) {
        Some(inner) => inner.trim().to_string(),
        None => s.to_string(),
    }
}

/// Look up `key` in a `.env` file (`KEY=value`, quotes unwrapped, `#`
/// comments skipped). A missing key, an empty value, or an unresolved
/// `${…}` placeholder value (the house "never provisioned" convention)
/// all yield None. The user's `.env` is the
/// ONLY source — no process-env fallback, so a declared credential must be
/// provisioned where the agent's identity lives.
fn lookup_env_value(env_path: &std::path::Path, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let env = std::fs::read_to_string(env_path).ok()?;
    let prefix = format!("{key}=");
    for line in env.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() && !value.starts_with("${") {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Issue 8e0a1215: resolve an agent-user's dispatch credential.
///
/// The `[thq].service_token` declaration is the ONLY source — the config
/// declares, the code never guesses. Declared-but-unresolved and
/// undeclared are both distinct loud outcomes
/// ([`ServiceTokenResolution`]); neither silently degrades.
pub fn resolve_user_service_token(
    user_home: &std::path::Path,
    config: &DispatchConfig,
) -> ServiceTokenResolution {
    match config.service_token.as_deref() {
        Some(decl) => {
            let var = declared_service_var(decl);
            match lookup_env_value(&user_home.join(".env"), &var) {
                Some(value) => ServiceTokenResolution::Resolved(value),
                None => ServiceTokenResolution::DeclaredUnresolved { var },
            }
        }
        None => ServiceTokenResolution::Undeclared,
    }
}

/// 0.19.4: keep the dispatch table in step with ONE materialization pass.
/// Called from the enrollment loop right after
/// [`crate::materialize::apply_profiles`]. Kills the "newly bound profile
/// needs a runtime restart" wart — and its silent twin: a DRAINED profile
/// used to stay console-dispatchable until restart.
///
/// Per profile outcome:
/// - `Applied` | `Unchanged` → upsert the entry from the materialized
///   overlay's `[thq]` anchor (`agent_name` → owner_id + resolved service
///   token + issuer). Silent when an identical entry already exists — the
///   pull runs every heartbeat cycle.
/// - `Gated` / `Failed` / missing anchor → remove any entry whose
///   `user_key` equals the profile id (the materialized anchor form).
///   Old-world 16E entries carry a Kanidm sub, which can never collide
///   with a profile id — legacy agents are untouchable by this path.
pub fn refresh_after_apply(
    table: &dashmap::DashMap<String, ThqDispatchEntry>,
    trustee_home: &std::path::Path,
    report: &crate::materialize::ApplyReport,
) {
    use crate::materialize::ProfileOutcome;

    for pr in &report.profiles {
        let user_dir = trustee_home
            .join("users")
            .join(trustee_core::user_hash(&pr.profile_id));
        let overlay = user_dir.join("config").join("trustee.toml");
        let cfg = std::fs::read_to_string(&overlay)
            .ok()
            .and_then(|c| DispatchConfig::from_toml(&c));
        let wants_entry = matches!(
            pr.outcome,
            ProfileOutcome::Applied | ProfileOutcome::Unchanged
        );

        match (wants_entry, cfg) {
            (true, Some(cfg)) if !cfg.owner_id.as_deref().unwrap_or_default().is_empty() => {
                match resolve_user_service_token(&user_dir, &cfg) {
                    ServiceTokenResolution::Resolved(bearer) => {
                        let entry = ThqDispatchEntry {
                            user_key: cfg.owner_id.clone().unwrap_or_default(),
                            service_token: Some(bearer),
                            issuer_url: read_overlay_service_issuer(&user_dir),
                        };
                        let changed = match table.get(&cfg.agent_name) {
                            Some(existing) => {
                                let e = existing.value();
                                e.user_key != entry.user_key
                                    || e.service_token != entry.service_token
                                    || e.issuer_url != entry.issuer_url
                            }
                            None => true,
                        };
                        table.insert(cfg.agent_name.clone(), entry);
                        if changed {
                            tracing::info!(
                                target: "thq",
                                "THQ dispatch refresh: agent {} upserted (profile {}) — console-reachable, no restart needed",
                                cfg.agent_name, pr.profile_id
                            );
                        }
                    }
                    other => {
                        remove_by_user_key(table, &pr.profile_id);
                        tracing::warn!(
                            target: "thq",
                            "THQ dispatch refresh: agent {} anchor present but credential unresolved ({other:?}) — entry removed",
                            cfg.agent_name
                        );
                    }
                }
            }
            _ => {
                if remove_by_user_key(table, &pr.profile_id) {
                    tracing::info!(
                        target: "thq",
                        "THQ dispatch refresh: profile {} no longer materialized — dispatch entry removed",
                        pr.profile_id
                    );
                }
            }
        }
    }
}

/// Remove the dispatch entry (if any) whose `user_key` equals the profile id
/// — the materialized-anchor form. Returns true when an entry was removed.
fn remove_by_user_key(
    table: &dashmap::DashMap<String, ThqDispatchEntry>,
    user_key: &str,
) -> bool {
    let key = table
        .iter()
        .find(|kv| kv.value().user_key == user_key)
        .map(|kv| kv.key().clone());
    match key {
        Some(k) => table.remove(&k).is_some(),
        None => false,
    }
}

/// 16F: build the dispatch table from per-user `[thq]` overlays.
///
/// The dispatch lane of the old `thq_register::spawn_all`, extracted
/// unchanged by the 0.15.0 enrollment rewrite: every discovered agent-user
/// with an `owner_id` (dispatch intent) lands in `ServerState.thq_dispatch`
/// keyed by `agent_name`, carrying her resolved service credential and
/// issuer. Loud errors on undeclared/unresolved credentials — the agent
/// stays out of the table, never a silent skip. Registration is NOT this
/// function's business anymore (see `thq_register`).
pub fn populate(state: ServerState) {
    let agents = discover_user_agents();
    if agents.is_empty() {
        tracing::debug!("THQ dispatch: no per-user [thq] entries under users/");
        return;
    }

    tracing::info!(
        "THQ dispatch table: {} agent-user(s) from users/",
        agents.len()
    );

    for agent in agents.iter() {
        let label = format!("{} ({})", agent.config.agent_name, agent.user_hash);
        // Issue 8e0a1215: credential resolution is config-DECLARED, not
        // code-guessed. An explicit [thq].service_token wins; absent →
        // loud error; declared-but-unresolved → LOUD error, non-dispatchable.
        let resolution = resolve_user_service_token(&agent.user_home, &agent.config);
        let bearer = match &resolution {
            ServiceTokenResolution::Resolved(token) => Some(token.clone()),
            ServiceTokenResolution::Undeclared => {
                tracing::error!(
                    "THQ: agent {} has NO [thq].service_token declaration — dispatch credentials are \
                     config-declared since api 0.13.0 (the hardcoded THQ/FAME/FARZAN/KANIDM key scan \
                     was removed). Add service_token = \"${{YOUR_KEY}}\" to the [thq] section of {} — \
                     agent NOT dispatchable until then.",
                    label,
                    agent.user_home.join("config").join("trustee.toml").display()
                );
                None
            }
            ServiceTokenResolution::DeclaredUnresolved { var } => {
                tracing::error!(
                    "THQ: agent {} declared [thq].service_token = \"${{{var}}}\" but {var} is NOT set in {} — \
                     agent NOT dispatchable. Set the variable in the agent's .env (a placeholder value \
                     like ${{...}} does not count) or fix the declaration.",
                    label,
                    agent.user_home.join(".env").display()
                );
                None
            }
        };

        // 16F: register the dispatch target so THQ-proxied sessions can be
        // impersonated AS this agent-user (see crate::xagent).
        if agent.config.owner_id.as_deref().map(str::len).unwrap_or(0) > 0 {
            if !matches!(resolution, ServiceTokenResolution::Resolved(_)) {
                // Already SCREAMED above — the agent stays out of the
                // dispatch table (non-dispatchable). Never a silent skip.
            } else {
                state.thq_dispatch.insert(
                    agent.config.agent_name.clone(),
                    ThqDispatchEntry {
                        user_key: agent.config.owner_id.clone().unwrap_or_default(),
                        service_token: bearer,
                        issuer_url: read_overlay_service_issuer(&agent.user_home),
                    },
                );
            }
        } else {
            tracing::warn!(
                "THQ: agent-user {} has no [thq].owner_id — NOT dispatchable (16F)",
                label
            );
        }

        tracing::info!(
            "THQ: agent-user {} (owner={})",
            label,
            agent.config.owner_id.as_deref().unwrap_or("<none>")
        );
    }
}

/// Testable core of [`populate`], writing into an explicit table.
#[cfg(test)]
pub fn populate_into(
    table: &dashmap::DashMap<String, ThqDispatchEntry>,
    agents: &[DiscoveredAgent],
) {
    for agent in agents {
        let resolution = resolve_user_service_token(&agent.user_home, &agent.config);
        if agent.config.owner_id.as_deref().map(str::len).unwrap_or(0) == 0 {
            continue;
        }
        if let ServiceTokenResolution::Resolved(token) = resolution {
            table.insert(
                agent.config.agent_name.clone(),
                ThqDispatchEntry {
                    user_key: agent.config.owner_id.clone().unwrap_or_default(),
                    service_token: Some(token),
                    issuer_url: read_overlay_service_issuer(&agent.user_home),
                },
            );
        }
    }
}

/// Shared test fixture: a fresh temp dir standing in for a user home.
#[cfg(test)]
fn temp_users_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("trustee-dispatch-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;

    #[test]
    fn parse_dispatch_keys_only() {
        // 0.15.0: registration-era keys in an overlay are INERT — dispatch
        // reads agent_name/owner_id/service_token and nothing else.
        let toml = r#"
[thq]
torpi_url = "https://torpi.example.com"
advertise_url = "https://10.0.0.5:3000"
agent_name = "ravand"
owner_id = "1a71c077-b3b3-4581-b605-925c3f276f30"
service_token = "${PAYDAR_SERVICE_ACCOUNT}"
"#;
        let cfg = DispatchConfig::from_toml(toml).expect("should parse");
        assert_eq!(cfg.agent_name, "ravand");
        assert_eq!(
            cfg.owner_id.as_deref(),
            Some("1a71c077-b3b3-4581-b605-925c3f276f30")
        );
        assert_eq!(
            cfg.service_token.as_deref(),
            Some("${PAYDAR_SERVICE_ACCOUNT}")
        );
    }

    #[test]
    fn parse_minimal_overlay_no_registration_keys() {
        // The new shape: no torpi_url/advertise_url at all.
        let toml = r#"
[thq]
agent_name = "nox"
owner_id = "d0f5c4ba-9c10-4ff7-85a4-f2c0e588a55a"
"#;
        let cfg = DispatchConfig::from_toml(toml).expect("should parse");
        assert_eq!(cfg.agent_name, "nox");
        assert!(cfg.service_token.is_none());
    }

    #[test]
    fn parse_no_section_returns_none() {
        let toml = "[oidc]\nissuer_url = \"https://example.com\"\n";
        assert!(DispatchConfig::from_toml(toml).is_none());
    }

    #[test]
    fn empty_owner_id_is_none() {
        let toml = "[thq]\nagent_name = \"x\"\nowner_id = \"\"\n";
        let cfg = DispatchConfig::from_toml(toml).unwrap();
        assert!(cfg.owner_id.is_none());
    }

    #[test]
    fn discover_finds_only_users_with_thq() {
        let base = temp_users_dir("discover");
        let thq_overlay = "[mcp]\nenabled = true\n\n[thq]\nagent_name = \"ravand\"\n";
        for (hash, content) in [
            ("aaaa1111", thq_overlay),
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
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_is_deterministic_and_sorted() {
        let base = temp_users_dir("sorted");
        for hash in ["dddd4444", "bbbb2222", "cccc3333"] {
            let dir = base.join(hash).join("config");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("trustee.toml"), "[thq]\nagent_name = \"a\"\n").unwrap();
        }
        let found = discover_user_agents_in(&base);
        let hashes: Vec<&str> = found.iter().map(|a| a.user_hash.as_str()).collect();
        assert_eq!(hashes, vec!["bbbb2222", "cccc3333", "dddd4444"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn read_overlay_service_issuer_picks_service_account_credential() {
        let base = std::env::temp_dir().join(format!("trustee-dispatch-issuer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("config");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("trustee.toml"),
            "[thq]\nagent_name = \"a\"\n\n[mcp.credentials.fame_service]\ntype = \"service-account\"\nservice_token = \"${FAME_SERVICE_TOKEN}\"\nissuer_url = \"https://idp.tanbal.ir/oauth2/openid/pdt-api\"\n\n[mcp.credentials.interactive]\ntype = \"interactive\"\nissuer_url = \"https://ignored.example/\"\n",
        )
        .unwrap();
        assert_eq!(
            read_overlay_service_issuer(&base).as_deref(),
            Some("https://idp.tanbal.ir/oauth2/openid/pdt-api"),
            "issuer must come from the service-account credential, not another type"
        );
        assert!(read_overlay_service_issuer(&base.join("nope")).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn populate_into_builds_table_for_declared_and_resolved() {
        let base = temp_users_dir("populate");
        let home = base.join("aaaa1111");
        std::fs::create_dir_all(home.join("config")).unwrap();
        std::fs::write(
            home.join("config").join("trustee.toml"),
            "[thq]\nagent_name = \"ravand\"\nowner_id = \"sub-uuid\"\nservice_token = \"${KEY}\"\n\n[mcp.credentials.svc]\ntype = \"service-account\"\nissuer_url = \"https://idp.example\"\n",
        )
        .unwrap();
        std::fs::write(home.join(".env"), "KEY=token-1\n").unwrap();

        let agents = discover_user_agents_in(&base);
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();
        populate_into(&table, &agents);
        let e = table.get("ravand").expect("dispatch entry").clone();
        assert_eq!(e.user_key, "sub-uuid");
        assert_eq!(e.service_token.as_deref(), Some("token-1"));
        assert_eq!(e.issuer_url.as_deref(), Some("https://idp.example"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn populate_into_skips_unresolved_credential() {
        // Declared but unresolved → loud at populate(), and the entry is
        // NOT inserted (non-dispatchable, never a silent skip).
        let base = temp_users_dir("populate-unresolved");
        let home = base.join("aaaa1111");
        std::fs::create_dir_all(home.join("config")).unwrap();
        std::fs::write(
            home.join("config").join("trustee.toml"),
            "[thq]\nagent_name = \"ghost\"\nowner_id = \"sub-uuid\"\nservice_token = \"${NEVER_SET}\"\n",
        )
        .unwrap();

        let agents = discover_user_agents_in(&base);
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();
        populate_into(&table, &agents);
        assert!(table.get("ghost").is_none(), "unresolved credential must not dispatch");
        let _ = std::fs::remove_dir_all(&base);
    }
}

// ---------------------------------------------------------------------------
// Issue 8e0a1215: [thq].service_token declared credential key
// ---------------------------------------------------------------------------

#[cfg(test)]
mod service_token_declaration_tests {
    use super::*;

    /// Minimal overlay TOML with an optional service_token declaration.
    fn thq_toml(service_token: Option<&str>) -> String {
        let mut s = String::from("[thq]\nagent_name = \"ravand\"\n");
        if let Some(st) = service_token {
            s.push_str(&format!("service_token = \"{st}\"\n"));
        }
        s
    }

    #[test]
    fn parse_declared_service_token_field() {
        let cfg = DispatchConfig::from_toml(&thq_toml(Some("${PAYDAR_SERVICE_ACCOUNT}")))
            .expect("should parse");
        assert_eq!(
            cfg.service_token.as_deref(),
            Some("${PAYDAR_SERVICE_ACCOUNT}"),
            "declaration is stored verbatim; resolution happens at boot"
        );
        assert!(
            DispatchConfig::from_toml(&thq_toml(None))
                .unwrap()
                .service_token
                .is_none(),
            "absent field = None (non-dispatchable, loud when owner_id set)"
        );
    }

    #[test]
    fn declared_var_accepts_wrapped_and_bare_forms() {
        assert_eq!(
            declared_service_var("${PAYDAR_SERVICE_ACCOUNT}"),
            "PAYDAR_SERVICE_ACCOUNT"
        );
        assert_eq!(
            declared_service_var("  ${ PAYDAR_SERVICE_ACCOUNT }  "),
            "PAYDAR_SERVICE_ACCOUNT"
        );
        assert_eq!(
            declared_service_var("PAYDAR_SERVICE_ACCOUNT"),
            "PAYDAR_SERVICE_ACCOUNT"
        );
        // Malformed input surfaces verbatim in the loud error, never swallowed.
        assert_eq!(declared_service_var(""), "");
        assert_eq!(declared_service_var("${"), "${");
    }

    /// THE PAYDAR CASE (live incident, issue 8e0a1215): an agent whose
    /// credential lives under a key OUTSIDE the legacy four dispatches with
    /// zero source changes — config-declared, not code-guessed.
    #[test]
    fn declared_field_wins_over_legacy_scan_with_custom_key_name() {
        let base = temp_users_dir("declared-wins");
        std::fs::write(
            base.join(".env"),
            "THQ_SERVICE_TOKEN=legacy-token\nPAYDAR_SERVICE_ACCOUNT=paydar-token\n",
        )
        .unwrap();
        let cfg = DispatchConfig::from_toml(&thq_toml(Some("${PAYDAR_SERVICE_ACCOUNT}"))).unwrap();
        assert_eq!(
            resolve_user_service_token(&base, &cfg),
            ServiceTokenResolution::Resolved("paydar-token".to_string()),
            "explicit declaration must win over the legacy priority list"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn declared_but_env_file_missing_is_loud_unresolved() {
        let base = temp_users_dir("declared-no-env");
        let cfg = DispatchConfig::from_toml(&thq_toml(Some("${PAYDAR_SERVICE_ACCOUNT}"))).unwrap();
        assert_eq!(
            resolve_user_service_token(&base, &cfg),
            ServiceTokenResolution::DeclaredUnresolved {
                var: "PAYDAR_SERVICE_ACCOUNT".to_string()
            },
            ".env missing entirely → same loud path, never a silent skip"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn declared_but_key_missing_from_env_is_loud_unresolved() {
        let base = temp_users_dir("declared-no-key");
        std::fs::write(base.join(".env"), "THQ_SERVICE_TOKEN=legacy-token\n").unwrap();
        let cfg = DispatchConfig::from_toml(&thq_toml(Some("${PAYDAR_SERVICE_ACCOUNT}"))).unwrap();
        assert_eq!(
            resolve_user_service_token(&base, &cfg),
            ServiceTokenResolution::DeclaredUnresolved {
                var: "PAYDAR_SERVICE_ACCOUNT".to_string()
            },
            "legacy key presence must NOT satisfy a different declared key"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn declared_but_placeholder_value_is_unresolved() {
        let base = temp_users_dir("declared-placeholder");
        // House convention: a ${...} VALUE means the secret was never
        // provisioned.
        std::fs::write(
            base.join(".env"),
            "PAYDAR_SERVICE_ACCOUNT=${PAYDAR_SERVICE_ACCOUNT}\n",
        )
        .unwrap();
        let cfg = DispatchConfig::from_toml(&thq_toml(Some("${PAYDAR_SERVICE_ACCOUNT}"))).unwrap();
        assert_eq!(
            resolve_user_service_token(&base, &cfg),
            ServiceTokenResolution::DeclaredUnresolved {
                var: "PAYDAR_SERVICE_ACCOUNT".to_string()
            }
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The hardcoded key scan is REMOVED (api 0.13.0): an agent declaring
    /// nothing is `Undeclared` — non-dispatchable, loud at boot when
    /// `owner_id` (dispatch intent) is set. Legacy keys in .env are INERT.
    #[test]
    fn undeclared_declaration_is_undeclared_even_with_legacy_keys_present() {
        let base = temp_users_dir("undeclared");
        // The blessed-four keys sit right there in .env — they no longer
        // conjure a credential. Config declares or the agent is out.
        std::fs::write(base.join(".env"), "THQ_SERVICE_TOKEN=thq-token\n").unwrap();
        let cfg = DispatchConfig::from_toml(&thq_toml(None)).unwrap();
        assert_eq!(
            resolve_user_service_token(&base, &cfg),
            ServiceTokenResolution::Undeclared,
            "no declaration → Undeclared; the legacy scan must NOT rescue it"
        );
        // Same answer with no .env at all — the outcome is config-shaped,
        // not env-shaped.
        assert_eq!(
            resolve_user_service_token(&base.join("no-env"), &cfg),
            ServiceTokenResolution::Undeclared
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}


#[cfg(test)]
mod refresh_tests {
    use super::*;
    use crate::materialize::{ApplyReport, ProfileOutcome, ProfileReport};
    use std::path::PathBuf;
    use dashmap::DashMap;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trustee-disp-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_materialized(home: &PathBuf, pid: &str, agent_name: &str, token_value: &str) {
        let dir = home
            .join("users")
            .join(trustee_core::user_hash(pid));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(
            dir.join("config").join("trustee.toml"),
            format!(
                "[agent]\nname = \"{agent_name}\"\n\n[thq]\nagent_name = \"{agent_name}\"\nowner_id = \"{pid}\"\nservice_token = \"farzan_service_account\"\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join(".env"), format!("farzan_service_account={token_value}\n")).unwrap();
    }

    fn report(pid: &str, outcome: ProfileOutcome) -> ApplyReport {
        ApplyReport {
            profiles: vec![ProfileReport {
                profile_id: pid.to_string(),
                outcome,
            }],
        }
    }

    #[test]
    fn refresh_upserts_entry_for_applied_profile() {
        let home = temp_home("up");
        make_materialized(&home, "prof-1", "Farzan", "tok-1");
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();

        refresh_after_apply(&table, &home, &report("prof-1", ProfileOutcome::Applied));

        let e = table.get("Farzan").expect("entry must exist");
        assert_eq!(e.user_key, "prof-1", "user_key = the stable binding id");
        assert_eq!(e.service_token.as_deref(), Some("tok-1"));
    }

    #[test]
    fn refresh_is_idempotent_for_unchanged_profiles() {
        let home = temp_home("idem");
        make_materialized(&home, "prof-1", "Farzan", "tok-1");
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();

        refresh_after_apply(&table, &home, &report("prof-1", ProfileOutcome::Applied));
        refresh_after_apply(&table, &home, &report("prof-1", ProfileOutcome::Unchanged));

        assert_eq!(table.len(), 1, "no duplicate entries across pull cycles");
        assert_eq!(table.get("Farzan").unwrap().service_token.as_deref(), Some("tok-1"));
    }

    #[test]
    fn refresh_removes_entry_when_profile_drains() {
        let home = temp_home("drain");
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();
        table.insert(
            "Farzan".to_string(),
            ThqDispatchEntry {
                user_key: "prof-1".to_string(),
                service_token: Some("tok-1".to_string()),
                issuer_url: None,
            },
        );
        // Drain deleted the overlay config — nothing on disk to re-discover.
        let outcome = ProfileOutcome::Gated {
            desired: "stopped".to_string(),
            drained: true,
        };
        refresh_after_apply(&table, &home, &report("prof-1", outcome));

        assert!(table.get("Farzan").is_none(), "drained profile must leave the table");
    }

    #[test]
    fn refresh_removes_entry_when_profile_is_reclaimed() {
        // v0.19.5: a profile deleted in THQ is Reclaimed by the applier —
        // its dispatch entry must leave the table in the same pull cycle.
        let home = temp_home("reclaim");
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();
        table.insert(
            "Farzan".to_string(),
            ThqDispatchEntry {
                user_key: "prof-1".to_string(),
                service_token: Some("tok-1".to_string()),
                issuer_url: None,
            },
        );
        refresh_after_apply(&table, &home, &report("prof-1", ProfileOutcome::Reclaimed));

        assert!(table.get("Farzan").is_none(), "reclaimed profile must leave the table");
    }

    #[test]
    fn refresh_removes_stale_entry_when_apply_fails() {
        let home = temp_home("fail");
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();
        table.insert(
            "Farzan".to_string(),
            ThqDispatchEntry {
                user_key: "prof-1".to_string(),
                service_token: Some("tok-1".to_string()),
                issuer_url: None,
            },
        );
        // Failed apply = no materialized dir → not dispatchable.
        let outcome = ProfileOutcome::Failed {
            reason: "secrets is a bare value".to_string(),
        };
        refresh_after_apply(&table, &home, &report("prof-1", outcome));

        assert!(table.get("Farzan").is_none(), "stale entry must go on loud failure");
    }

    #[test]
    fn refresh_never_touches_old_world_16e_entries() {
        let home = temp_home("legacy");
        make_materialized(&home, "prof-1", "Farzan", "tok-1");
        let table: DashMap<String, ThqDispatchEntry> = DashMap::new();
        table.insert(
            "legacy-nox".to_string(),
            ThqDispatchEntry {
                user_key: "kanidm-sub-uuid".to_string(),
                service_token: Some("legacy-tok".to_string()),
                issuer_url: None,
            },
        );

        // A DIFFERENT profile fails → removal is keyed, not wholesale.
        refresh_after_apply(&table, &home, &report("prof-9", ProfileOutcome::Failed {
            reason: "x".to_string(),
        }));
        assert!(table.contains_key("legacy-nox"), "unrelated legacy entry untouched");
    }
}

//! Materialization applier — the runtime APPLIES its bound profiles.
//!
//! This is the owed dispatch (scope line of the 0.18.0 enrollment release:
//! "payload logged, NOT applied" — this module closes that scope).
//!
//! For every bound profile in the materialization pull, the runtime
//! materializes the binding as a FIRST-CLASS AGENT-USER under
//! `~/.trustee/users/{user_hash(profile_id)}/` — the same per-user
//! machinery the dispatch lane already drives (overlay config, `.env`,
//! MCP loader cache). The profile IS the binding, so the binding key is
//! the user key.
//!
//! Per profile:
//! - `config/trustee.toml` — `[agent] name` (the identity's name),
//!   `[lifecycle] system_template` (the persona), `[llm] model` (the
//!   model@provider selector; honored when `[users].allow_llm_overlay`
//!   is on), plus the `[mcp]` section parsed out of the payload's
//!   `mcp_servers` content. Unparsable MCP content is skipped LOUDLY —
//!   recorded in the marker, never silently dropped.
//! - `.env` (0600) — the profile's `secrets` string parsed as KEY=VALUE
//!   lines. SECRET VALUES NEVER APPEAR IN ANY LOG LINE: the applier logs
//!   names and lengths only.
//! - `materialized.json` — a marker with the content fingerprint: a
//!   second pull with UNCHANGED content skips the rewrite (idempotent;
//!   only real changes touch disk).
//!
//! `desired_state` is recorded in the marker and logged; observed state
//! remains the runtime's REAL process health (the state-report lane owns
//! it — a materialized-but-idle process honestly reports idle).

use sha2::{Digest, Sha256};

use std::path::Path;

/// What the applier decided for ONE profile this pass.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileOutcome {
    /// Freshly written or updated this pass.
    Applied,
    /// Content unchanged (fingerprint match) — nothing touched.
    Unchanged,
    /// desired_state is not "running": NOT materialized (skip-with-record);
    /// `drained` = a previously materialized user was stripped (overlay +
    /// secrets removed, history preserved).
    Gated { desired: String, drained: bool },
    /// Could not be applied — reason is loud and per-profile.
    Failed { reason: String },
}

/// Per-profile report entry.
#[derive(Debug, Clone)]
pub struct ProfileReport {
    pub profile_id: String,
    pub outcome: ProfileOutcome,
}

/// Outcome of one apply pass over the pull payload.
#[derive(Debug, Default, Clone)]
pub struct ApplyReport {
    pub profiles: Vec<ProfileReport>,
}

impl ApplyReport {
    pub fn applied(&self) -> impl Iterator<Item = &str> {
        self.profiles
            .iter()
            .filter(|p| p.outcome == ProfileOutcome::Applied)
            .map(|p| p.profile_id.as_str())
    }

    /// The observed_state string the state-report lane sends for this
    /// profile — the LAUNCH SEMANTICS contract (Paydar item 2, decided:
    /// option (b) refined). The ladder tells the truth per profile:
    /// - process has live sessions            → "running"
    /// - materialized, ready, not yet used    → "materialized"
    /// - desired_state gated it               → "stopped"
    /// - apply failed                         → "failed"
    pub fn observed_for(&self, profile_id: &str, busy: bool) -> String {
        let Some(p) = self.profiles.iter().find(|p| p.profile_id == profile_id) else {
            return if busy {
                "running: live session(s)".to_string()
            } else {
                "materialized: ready".to_string()
            };
        };
        match &p.outcome {
            ProfileOutcome::Applied | ProfileOutcome::Unchanged => {
                if busy {
                    "running: live session(s) in process".to_string()
                } else {
                    "materialized: ready".to_string()
                }
            }
            ProfileOutcome::Gated { desired, drained } => format!(
                "stopped: gated by desired_state ({desired}{})",
                if *drained { ", drained" } else { "" }
            ),
            ProfileOutcome::Failed { reason } => format!("failed: {reason}"),
        }
    }
}

/// Apply every bound profile in the pull payload. `trustee_home` is the
/// `~/.trustee` directory. Never panics on payload shapes — every profile
/// is independent, failures are per-profile and loud.
pub fn apply_profiles(trustee_home: &Path, payload: &serde_json::Value) -> ApplyReport {
    let mut report = ApplyReport::default();
    let Some(profiles) = payload.get("profiles").and_then(|v| v.as_array()) else {
        return report;
    };
    for p in profiles {
        let Some(profile_id) = p.get("profile_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let outcome = match apply_one(trustee_home, p) {
            Ok(o) => o,
            Err(reason) => ProfileOutcome::Failed { reason },
        };
        report.profiles.push(ProfileReport {
            profile_id: profile_id.to_string(),
            outcome,
        });
    }
    report
}

/// THQ entity titles carry display prefixes ("Agent: Farzan") — the
/// materialized agent-user's own name strips them (the convention, ruled
/// 2026-09-14: titles keep the prefix, the user name does not). The marker
/// keeps the RAW title for traceability.
const TITLE_PREFIXES: [&str; 4] = ["Agent: ", "Identity: ", "Runtime: ", "Profile: "];

fn strip_title_prefix(title: &str) -> String {
    for p in TITLE_PREFIXES {
        if let Some(rest) = title.strip_prefix(p) {
            return rest.to_string();
        }
    }
    title.to_string()
}

/// Apply one profile. `Ok(true)` = written, `Ok(false)` = unchanged.
fn apply_one(trustee_home: &Path, p: &serde_json::Value) -> Result<ProfileOutcome, String> {
    let profile_id = p
        .get("profile_id")
        .and_then(|v| v.as_str())
        .ok_or("no profile_id")?;
    let raw_name = p
        .get("identity")
        .and_then(|i| i.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = strip_title_prefix(&raw_name);
    let identity_id = p
        .get("identity")
        .and_then(|i| i.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let persona = p
        .get("identity")
        .and_then(|i| i.get("persona"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = p.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mcp_servers = p
        .get("mcp_servers")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let secrets = p.get("secrets").and_then(|v| v.as_str()).unwrap_or("");
    let desired_state = p
        .get("desired_state")
        .and_then(|v| v.as_str())
        .unwrap_or("running")
        .to_string();

    let user_dir = trustee_home
        .join("users")
        .join(trustee_core::user_hash(profile_id));
    let marker_path = user_dir.join("materialized.json");

    // v0.19.1 (Paydar item 1): desired_state GATES the applier — whitelist
    // "running". A stopped/paused profile must not materialize-and-run:
    // fresh → skip-with-record; previously materialized → DRAIN (the
    // applier-owned overlay + secrets are removed, the dir and any history
    // stay, the marker records the drain). Flipping back to running
    // re-materializes via the fingerprint miss.
    if desired_state != "running" {
        let was_materialized = marker_path.exists();
        if was_materialized {
            let _ = std::fs::remove_file(user_dir.join("config").join("trustee.toml"));
            let _ = std::fs::remove_file(user_dir.join(".env"));
            let marker = serde_json::json!({
                "profile_id": profile_id,
                "desired_state": desired_state,
                "materialized": false,
                "drained_at": chrono::Utc::now().to_rfc3339(),
            });
            std::fs::write(
                &marker_path,
                serde_json::to_vec_pretty(&marker).map_err(|e| format!("serialize marker: {e}"))?,
            )
            .map_err(|e| format!("write marker: {e}"))?;
            tracing::warn!(
                target: "thq",
                "THQ apply: profile {profile_id} DRAINED (desired_state={desired_state}) —                  overlay and secrets removed; history preserved"
            );
            return Ok(ProfileOutcome::Gated {
                desired: desired_state,
                drained: true,
            });
        }
        tracing::info!(
            target: "thq",
            "THQ apply: profile {profile_id} GATED (desired_state={desired_state}) — not materialized"
        );
        return Ok(ProfileOutcome::Gated {
            desired: desired_state,
            drained: false,
        });
    }

    // --- build the overlay (guaranteed-valid TOML via the toml crate) ---
    let mut overlay = toml::Table::new();
    let mut agent = toml::Table::new();
    if !name.is_empty() {
        agent.insert("name".into(), toml::Value::String(name.clone()));
    }
    if !agent.is_empty() {
        overlay.insert("agent".into(), toml::Value::Table(agent));
    }
    if !persona.is_empty() {
        let mut lifecycle = toml::Table::new();
        lifecycle.insert("system_template".into(), toml::Value::String(persona.clone()));
        overlay.insert("lifecycle".into(), toml::Value::Table(lifecycle));
    }
    if !model.is_empty() {
        // model@provider selector form — applied verbatim; honored when
        // [users].allow_llm_overlay is enabled in the runtime's config.
        let mut llm = toml::Table::new();
        llm.insert("model".into(), toml::Value::String(model.clone()));
        overlay.insert("llm".into(), toml::Value::Table(llm));
    }
    // The [mcp] section rides in the payload's mcp_servers content. Parse
    // (standalone doc with an [mcp] table, or a bare [mcp]/[[mcp.servers]]
    // fragment); anything unparsable is a LOUD per-profile skip — never a
    // silently-broken overlay for the whole process.
    let mut mcp_fragment = String::new();
    let mut credential_names: Vec<String> = Vec::new();
    if !mcp_servers.trim().is_empty() {
        match mcp_servers.parse::<toml::Table>() {
            Ok(frag) => {
                let mcp = frag.get("mcp").cloned().ok_or_else(|| {
                    "mcp_servers content parses as TOML but carries no [mcp] table".to_string()
                })?;
                let mcp = toml::Value::Table(match mcp.as_table().cloned() {
                    Some(t) => t,
                    None => return Err("mcp_servers [mcp] is not a table".to_string()),
                });
                // Distinct credential names the servers reference — the
                // secrets resolver may need them to map a bare value.
                credential_names = mcp_credential_names(&mcp);
                // serialize WITH the key context so [[mcp.servers]] keeps its
                // full path (an inner-table serialization would emit bare
                // [[servers]] — a different table in the overlay)
                let mut doc = toml::Table::new();
                doc.insert("mcp".into(), mcp);
                mcp_fragment = format!(
                    "\n# [mcp] — materialized from THQ profile {profile_id}\n{}\n",
                    toml::to_string_pretty(&toml::Value::Table(doc))
                        .map_err(|e| format!("serialize mcp: {e}"))?
                );
            }
            Err(e) => {
                return Err(format!(
                    "mcp_servers content is not valid TOML (skipped loudly): {e}"
                ));
            }
        }
    }
    let header = format!(
        "# Materialized by trustee from THQ — profile {profile_id}, identity {identity_id}.\n\
         # DO NOT HAND-EDIT: the next materialization pull rewrites changed content.\n"
    );
    let overlay_str = format!(
        "{header}{}\n{mcp_fragment}",
        toml::to_string_pretty(&toml::Value::Table(overlay.clone()))
            .map_err(|e| format!("serialize overlay: {e}"))?
    );

    // --- secrets → .env. Values NEVER logged — lengths/counts only. ---
    let env_str = resolve_env_text(secrets, &credential_names)?;

    // --- fingerprint: skip the rewrite when nothing changed ---
    let mut hasher = Sha256::new();
    hasher.update(overlay_str.as_bytes());
    hasher.update(b"\x00");
    hasher.update(env_str.as_bytes());
    let fingerprint = hex(&hasher.finalize());

    if let Ok(existing) = std::fs::read_to_string(&marker_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&existing) {
            if v.get("fingerprint").and_then(|f| f.as_str()) == Some(fingerprint.as_str()) {
                return Ok(ProfileOutcome::Unchanged); // unchanged — skip the rewrite
            }
        }
    }

    // --- write ---
    let config_dir = user_dir.join("config");
    std::fs::create_dir_all(&config_dir).map_err(|e| format!("mkdir: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&user_dir, std::fs::Permissions::from_mode(0o700));
    }
    std::fs::write(config_dir.join("trustee.toml"), &overlay_str)
        .map_err(|e| format!("write overlay: {e}"))?;
    std::fs::write(user_dir.join(".env"), &env_str).map_err(|e| format!("write .env: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            user_dir.join(".env"),
            std::fs::Permissions::from_mode(0o600),
        )
        .map_err(|e| format!("chmod .env: {e}"))?;
    }

    // Secret lengths only — values never cross a log line.
    let secret_kv = env_str
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
        .count();
    tracing::info!(
        target: "thq",
        "THQ apply: profile {profile_id} materialized (identity {identity_id:?}, desired={desired_state}, \
         secrets: {secret_kv} entr{}, fingerprint {fingerprint})",
        if secret_kv == 1 { "y" } else { "ies" }
    );

    let marker = serde_json::json!({
        "profile_id": profile_id,
        "identity_id": identity_id,
        "name": raw_name,
        "desired_state": desired_state,
        "fingerprint": fingerprint,
        "applied_at": chrono::Utc::now().to_rfc3339(),
        "mcp_applied": !mcp_fragment.is_empty(),
    });
    std::fs::write(
        &marker_path,
        serde_json::to_vec_pretty(&marker).map_err(|e| format!("serialize marker: {e}"))?,
    )
    .map_err(|e| format!("write marker: {e}"))?;
    Ok(ProfileOutcome::Applied)
}

/// Distinct `credentials` names referenced by an `[mcp]` table's servers —
/// the hooks a bare-value secrets string can be mapped to.
fn mcp_credential_names(mcp: &toml::Value) -> Vec<String> {
    let mut names: Vec<String> = mcp
        .get("servers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.get("credentials").and_then(|v| v.as_str()))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

/// Resolve the profile's `secrets` string into `.env` file text.
///
/// Contract (thq profile metadata): `KEY=VALUE` lines, `#` comments
/// skipped. Two real-world shapes beyond the contract are honored rather
/// than silently dropped — the 0.19.0/0.19.1 defect: a non-KEY=VALUE
/// secrets string parsed to a header-only (effectively EMPTY) .env while
/// the rest of the materialization succeeded, so the agent dir looked
/// fine and the agent ran credential-less (owner report, 2026-09-14):
///   - a JSON object of string values → flattened to KEY=VALUE;
///   - a bare value (no `=` anywhere) → mapped to the SINGLE credential
///     name the profile's mcp_servers reference.
/// A bare value with zero or multiple distinct credential names is a
/// LOUD per-profile failure — never a silent empty .env.
/// Values are NEVER logged — only lengths/counts.
fn resolve_env_text(secrets: &str, credential_names: &[String]) -> Result<String, String> {
    const HEADER: &str = "# Materialized from THQ profile secrets — 0600, never logged.\n";
    let trimmed = secrets.trim();
    if trimmed.is_empty() {
        return Ok(HEADER.to_string());
    }

    // KEY=VALUE lines (the contract).
    let kv: Vec<(&str, &str)> = trimmed
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            let k = k.trim();
            if k.is_empty() {
                return None;
            }
            Some((k, v.trim()))
        })
        .collect();
    if !kv.is_empty() {
        let mut out = String::from(HEADER);
        for (k, v) in kv {
            out.push_str(&format!("{k}={v}\n"));
        }
        return Ok(out);
    }

    // JSON object of string values.
    if trimmed.starts_with('{') {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(trimmed) {
            if !map.is_empty() && map.values().all(serde_json::Value::is_string) {
                let mut out = String::from(HEADER);
                for (k, v) in map {
                    out.push_str(&format!("{k}={}\n", v.as_str().unwrap_or_default()));
                }
                return Ok(out);
            }
        }
    }

    // Bare value → map to the single referenced credential name.
    match credential_names {
        [name] => Ok(format!("{HEADER}{name}={trimmed}\n")),
        other => Err(format!(
            "secrets is a bare value ({} chars, not KEY=VALUE or a JSON object) and the \
             profile references {} credential name(s) — cannot map it to .env unambiguously",
            trimmed.len(),
            other.len(),
        )),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trustee-mat-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture_payload(secrets: &str) -> serde_json::Value {
        json!({
            "profiles": [{
                "profile_id": "prof-1111",
                "name": "Profile: Farzan-nox",
                "desired_state": "running",
                "mcp_servers": "[mcp]\nenabled = true\n\n[[mcp.servers]]\nname = \"fame\"\nurl = \"https://fame.example\"\n",
                "model": "GLM-5.3-Flash@glm-zai",
                "secrets": secrets,
                "identity": {
                    "id": "ident-2222",
                    "name": "Agent: Farzan",
                    "agent_id": "fame-123",
                    "persona": "Be helpful, concise, Persian-friendly."
                }
            }],
            "total": 1
        })
    }

    fn user_dir(home: &Path, pid: &str) -> std::path::PathBuf {
        home.join("users").join(trustee_core::user_hash(pid))
    }

    #[test]
    fn applies_profile_to_user_home() {
        let home = tmp("apply");
        let report = apply_profiles(&home, &fixture_payload("farzan_service_account=sec-ret-1"));
        assert_eq!(
            report.profiles[0].outcome,
            ProfileOutcome::Applied
        );

        let ud = user_dir(&home, "prof-1111");
        let overlay = std::fs::read_to_string(ud.join("config").join("trustee.toml")).unwrap();
        assert!(overlay.contains("name = \"Farzan\""), "prefix stripped: {overlay}");
        assert!(
            !overlay.contains("Agent: Farzan"),
            "the [agent] name must not carry the title prefix"
        );
        assert!(
            overlay.contains("Be helpful, concise, Persian-friendly."),
            "persona applied: {overlay}"
        );
        assert!(overlay.contains("GLM-5.3-Flash@glm-zai"), "model pin applied");
        assert!(overlay.contains("[[mcp.servers]]"), "mcp fragment applied");
        assert!(overlay.contains("https://fame.example"));

        let env = std::fs::read_to_string(ud.join(".env")).unwrap();
        assert!(env.contains("farzan_service_account=sec-ret-1"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(ud.join(".env"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "secrets .env is owner-only");
        }

        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(ud.join("materialized.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(marker["profile_id"], "prof-1111");
        assert_eq!(marker["name"], "Agent: Farzan", "marker keeps the RAW title");
        assert_eq!(marker["desired_state"], "running");
        assert_eq!(marker["mcp_applied"], true);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn second_pull_with_unchanged_content_skips_rewrite() {
        let home = tmp("idem");
        assert_eq!(apply_profiles(&home, &fixture_payload("k=v")).applied().count(), 1);
        let report = apply_profiles(&home, &fixture_payload("k=v"));
        assert_eq!(
            report.profiles[0].outcome,
            ProfileOutcome::Unchanged
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn bare_secret_maps_to_single_mcp_credential() {
        let home = tmp("bare1");
        let mut payload = fixture_payload("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmYXJ6YW4ifQ.sig");
        payload["profiles"][0]["mcp_servers"] = json!(
            "[mcp]\nenabled = true\n\n[[mcp.servers]]\nname = \"fame\"\nurl = \"https://fame.example\"\ncredentials = \"farzan_service_account\"\n"
        );
        let report = apply_profiles(&home, &payload);
        assert!(
            matches!(report.profiles[0].outcome, ProfileOutcome::Applied),
            "got {:?}",
            report.profiles[0].outcome
        );
        let env = std::fs::read_to_string(user_dir(&home, "prof-1111").join(".env")).unwrap();
        assert!(
            env.contains("farzan_service_account=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmYXJ6YW4ifQ.sig\n"),
            "bare value must map to the single referenced credential name"
        );
    }

    #[test]
    fn bare_secret_without_any_credential_name_fails_loud() {
        let home = tmp("bare0");
        let report = apply_profiles(&home, &fixture_payload("raw-token-no-equals"));
        match &report.profiles[0].outcome {
            ProfileOutcome::Failed { reason } => {
                assert!(reason.contains("bare value"), "reason: {reason}");
            }
            other => panic!("expected loud failure, got {other:?}"),
        }
        assert!(
            !user_dir(&home, "prof-1111").exists(),
            "a failed secrets resolution must not leave a half-materialized dir"
        );
    }

    #[test]
    fn bare_secret_with_multiple_credential_names_fails_loud() {
        let home = tmp("bare2");
        let mut payload = fixture_payload("raw-token");
        payload["profiles"][0]["mcp_servers"] = json!(
            "[mcp]\nenabled = true\n\n[[mcp.servers]]\nname = \"a\"\nurl = \"https://a.example\"\ncredentials = \"cred_a\"\n\n[[mcp.servers]]\nname = \"b\"\nurl = \"https://b.example\"\ncredentials = \"cred_b\"\n"
        );
        let report = apply_profiles(&home, &payload);
        assert!(matches!(
            &report.profiles[0].outcome,
            ProfileOutcome::Failed { .. }
        ));
    }

    #[test]
    fn json_object_secrets_flatten_to_env_entries() {
        let home = tmp("jsonsec");
        let payload =
            fixture_payload(r#"{"farzan_service_account":"tok-1","other_key":"val-2"}"#);
        let report = apply_profiles(&home, &payload);
        assert!(matches!(report.profiles[0].outcome, ProfileOutcome::Applied));
        let env = std::fs::read_to_string(user_dir(&home, "prof-1111").join(".env")).unwrap();
        assert!(env.contains("farzan_service_account=tok-1\n"));
        assert!(env.contains("other_key=val-2\n"));
    }

    #[test]
    fn changed_secrets_reapply_and_rotate_env() {
        let home = tmp("rotate");
        apply_profiles(&home, &fixture_payload("k=old-value"));
        let report = apply_profiles(&home, &fixture_payload("k=new-value"));
        assert_eq!(report.profiles[0].outcome, ProfileOutcome::Applied);
        let env = std::fs::read_to_string(user_dir(&home, "prof-1111").join(".env")).unwrap();
        assert!(env.contains("k=new-value") && !env.contains("old-value"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// THE PIN: secret material must never surface in the redacted payload
    /// (the only form the pull ever logs).
    #[test]
    fn redacted_payload_never_carries_secret_values() {
        let canary = "SUPER-SECRET-JWT-canary-9f2c";
        let payload = fixture_payload(&format!("farzan_service_account={canary}"));
        let red = crate::thq_register::redact_payload(&payload);
        let rendered = format!("{red}");
        assert!(
            !rendered.contains(canary),
            "SECRETS-IN-LOGS regression: the redacted payload must not carry the canary"
        );
        assert!(rendered.contains("<redacted:"));
        assert_eq!(red["profiles"][0]["profile_id"], "prof-1111");
        assert_eq!(red["profiles"][0]["model"], "GLM-5.3-Flash@glm-zai");
    }

    #[test]
    fn unparsable_mcp_is_a_loud_per_profile_failure() {
        let home = tmp("badmcp");
        let mut payload = fixture_payload("k=v");
        payload["profiles"][0]["mcp_servers"] = json!("not toml at all {{{");
        let report = apply_profiles(&home, &payload);
        assert!(matches!(
            report.profiles[0].outcome,
            ProfileOutcome::Failed { .. }
        ));
        assert!(report.observed_for("prof-1111", false).starts_with("failed:"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn empty_secrets_produce_an_empty_env() {
        let home = tmp("nosecrets");
        apply_profiles(&home, &fixture_payload(""));
        let env = std::fs::read_to_string(user_dir(&home, "prof-1111").join(".env")).unwrap();
        assert!(env.lines().filter(|l| !l.starts_with('#')).count() == 0);
        let _ = std::fs::remove_dir_all(&home);
    }

    // ── v0.19.1: desired_state gating + drain ───────────────────────────

    fn gated_payload(desired: &str) -> serde_json::Value {
        let mut p = fixture_payload("k=v");
        p["profiles"][0]["desired_state"] = json!(desired);
        p
    }

    #[test]
    fn stopped_profile_is_gated_not_materialized() {
        let home = tmp("gate-fresh");
        let report = apply_profiles(&home, &gated_payload("stopped"));
        assert_eq!(
            report.profiles[0].outcome,
            ProfileOutcome::Gated { desired: "stopped".into(), drained: false }
        );
        assert!(
            !user_dir(&home, "prof-1111").exists(),
            "a gated profile must not materialize"
        );
        assert_eq!(
            report.observed_for("prof-1111", false),
            "stopped: gated by desired_state (stopped)"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn paused_profile_is_gated_too() {
        let home = tmp("gate-paused");
        let report = apply_profiles(&home, &gated_payload("paused"));
        assert!(matches!(
            report.profiles[0].outcome,
            ProfileOutcome::Gated { .. }
        ));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn flip_running_to_stopped_drains_a_materialized_user() {
        let home = tmp("drain");
        apply_profiles(&home, &fixture_payload("k=v"));
        let ud = user_dir(&home, "prof-1111");
        assert!(ud.join("config").join("trustee.toml").exists());

        let report = apply_profiles(&home, &gated_payload("stopped"));
        assert_eq!(
            report.profiles[0].outcome,
            ProfileOutcome::Gated { desired: "stopped".into(), drained: true }
        );
        assert!(
            !ud.join("config").join("trustee.toml").exists(),
            "overlay removed on drain"
        );
        assert!(!ud.join(".env").exists(), "secrets removed on drain");
        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(ud.join("materialized.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(marker["materialized"], false);
        assert_eq!(marker["desired_state"], "stopped");

        // flip back to running → re-materializes (fingerprint miss)
        let report = apply_profiles(&home, &fixture_payload("k=v"));
        assert_eq!(report.profiles[0].outcome, ProfileOutcome::Applied);
        assert!(ud.join("config").join("trustee.toml").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    // ── launch-semantics contract (option b, refined) ───────────────────

    #[test]
    fn observed_ladder_maps_outcomes_truthfully() {
        let mut home = tmp("observed");
        let report = apply_profiles(&home, &fixture_payload("k=v"));
        // materialized + not busy → materialized (NOT "idle")
        assert_eq!(
            report.observed_for("prof-1111", false),
            "materialized: ready"
        );
        // process busy → running (the worker is active)
        assert_eq!(
            report.observed_for("prof-1111", true),
            "running: live session(s) in process"
        );
        // gated → stopped
        let gated = apply_profiles(&home, &gated_payload("stopped"));
        assert_eq!(
            gated.observed_for("prof-1111", false),
            "stopped: gated by desired_state (stopped, drained)",
            "the profile was previously materialized — the drain is part of the truth"
        );
        let _ = std::fs::remove_dir_all(&home);
        home = home; // no-op for symmetry
    }
}

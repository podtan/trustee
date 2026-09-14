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

/// Outcome of one apply pass over the pull payload.
#[derive(Debug, Default, Clone)]
pub struct ApplyReport {
    /// profile_ids freshly written or updated this pass.
    pub applied: Vec<String>,
    /// profile_ids whose content was unchanged (fingerprint match).
    pub unchanged: Vec<String>,
    /// (profile_id, reason) for profiles that could not be applied.
    pub failed: Vec<(String, String)>,
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
        match apply_one(trustee_home, p) {
            Ok(true) => report.applied.push(profile_id.to_string()),
            Ok(false) => report.unchanged.push(profile_id.to_string()),
            Err(e) => report.failed.push((profile_id.to_string(), e)),
        }
    }
    report
}

/// Apply one profile. `Ok(true)` = written, `Ok(false)` = unchanged.
fn apply_one(trustee_home: &Path, p: &serde_json::Value) -> Result<bool, String> {
    let profile_id = p
        .get("profile_id")
        .and_then(|v| v.as_str())
        .ok_or("no profile_id")?;
    let name = p
        .get("identity")
        .and_then(|i| i.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
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

    // --- secrets → .env (KEY=VALUE lines). Values NEVER logged. ---
    let env_str = parse_env_lines(secrets);

    // --- fingerprint: skip the rewrite when nothing changed ---
    let mut hasher = Sha256::new();
    hasher.update(overlay_str.as_bytes());
    hasher.update(b"\x00");
    hasher.update(env_str.as_bytes());
    let fingerprint = hex(&hasher.finalize());

    let user_dir = trustee_home
        .join("users")
        .join(trustee_core::user_hash(profile_id));
    let marker_path = user_dir.join("materialized.json");

    if let Ok(existing) = std::fs::read_to_string(&marker_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&existing) {
            if v.get("fingerprint").and_then(|f| f.as_str()) == Some(fingerprint.as_str()) {
                return Ok(false); // unchanged — skip the rewrite
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
        "name": name,
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
    Ok(true)
}

/// Parse a secrets string (`KEY=VALUE` lines, `#` comments skipped) into
/// `.env` file text. Empty input → empty file (the binding simply carries
/// no secrets). Values are NOT validated beyond the split — they are
/// opaque to the runtime.
fn parse_env_lines(secrets: &str) -> String {
    let mut out = String::from("# Materialized from THQ profile secrets — 0600, never logged.\n");
    for line in secrets.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            if !k.is_empty() {
                out.push_str(&format!("{k}={}\n", v.trim()));
            }
        }
    }
    out
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

    #[test]
    fn applies_profile_to_user_home() {
        let home = tmp("apply");
        let report = apply_profiles(&home, &fixture_payload("farzan_service_account=sec-ret-1"));
        assert_eq!(report.applied, vec!["prof-1111".to_string()]);

        let user_dir = home.join("users").join(trustee_core::user_hash("prof-1111"));
        let overlay = std::fs::read_to_string(user_dir.join("config").join("trustee.toml")).unwrap();
        assert!(overlay.contains("Agent: Farzan"), "identity name applied");
        assert!(
            overlay.contains("Be helpful, concise, Persian-friendly."),
            "persona applied: {overlay}"
        );
        assert!(overlay.contains("GLM-5.3-Flash@glm-zai"), "model pin applied");
        assert!(overlay.contains("[[mcp.servers]]"), "mcp fragment applied");
        assert!(overlay.contains("https://fame.example"));

        let env = std::fs::read_to_string(user_dir.join(".env")).unwrap();
        assert!(env.contains("farzan_service_account=sec-ret-1"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(user_dir.join(".env"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "secrets .env is owner-only");
        }

        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(user_dir.join("materialized.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(marker["profile_id"], "prof-1111");
        assert_eq!(marker["desired_state"], "running");
        assert_eq!(marker["mcp_applied"], true);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn second_pull_with_unchanged_content_skips_rewrite() {
        let home = tmp("idem");
        assert_eq!(apply_profiles(&home, &fixture_payload("k=v")).applied.len(), 1);
        let report = apply_profiles(&home, &fixture_payload("k=v"));
        assert!(report.applied.is_empty());
        assert_eq!(report.unchanged, vec!["prof-1111".to_string()]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn changed_secrets_reapply_and_rotate_env() {
        let home = tmp("rotate");
        apply_profiles(&home, &fixture_payload("k=old-value"));
        let report = apply_profiles(&home, &fixture_payload("k=new-value"));
        assert_eq!(report.applied, vec!["prof-1111".to_string()]);
        let env = std::fs::read_to_string(
            home.join("users")
                .join(trustee_core::user_hash("prof-1111"))
                .join(".env"),
        )
        .unwrap();
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
        assert!(
            rendered.contains("<redacted:"),
            "the redaction is honest about having removed something: {rendered}"
        );
        // and the redaction is lossless about everything else
        assert_eq!(red["profiles"][0]["profile_id"], "prof-1111");
        assert_eq!(red["profiles"][0]["model"], "GLM-5.3-Flash@glm-zai");
    }

    #[test]
    fn unparsable_mcp_is_a_loud_per_profile_failure() {
        let home = tmp("badmcp");
        let mut payload = fixture_payload("k=v");
        payload["profiles"][0]["mcp_servers"] = json!("not toml at all {{{");
        let report = apply_profiles(&home, &payload);
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].1.contains("not valid TOML"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn empty_secrets_produce_an_empty_env() {
        let home = tmp("nosecrets");
        let report = apply_profiles(&home, &fixture_payload(""));
        assert_eq!(report.applied.len(), 1);
        let env = std::fs::read_to_string(
            home.join("users")
                .join(trustee_core::user_hash("prof-1111"))
                .join(".env"),
        )
        .unwrap();
        assert!(env.lines().filter(|l| !l.starts_with('#')).count() == 0);
        let _ = std::fs::remove_dir_all(&home);
    }
}

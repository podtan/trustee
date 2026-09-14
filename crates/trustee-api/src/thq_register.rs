//! THQ v0.3 runtime enrollment — the RUNTIME lane.
//!
//! Rewrite of the torpi-era auto-registration (owner rulings 2026-09-13,
//! three-concept model): agent identities, runtimes, and profiles live in
//! THQ, server-side. THIS module is the runtime half — the trustee PROCESS
//! itself. Zero coupling to users/agents: no per-user loops, no
//! per-user enrollment, no `users/{hash}` anything in this lane. One loop
//! per process reports process-wide health (any live session → running,
//! else idle).
//!
//! The 16F dispatch lane (agents-as-users, `xagent` impersonation) is a
//! DIFFERENT concept and lives in [`crate::thq_dispatch`], unchanged.
//!
//! # The lane (contracts pinned from thq v0.3.2 source)
//!
//! 1. **REGISTER** (open, instance-gated — `X-Instance-Id` header, never
//!    in the path; `thq_url` is origin-only so it survives API prefix
//!    bumps): `POST {thq_url}/api/v1/runtime/register` with
//!    `{name, advertise_url}`. The runtime's identity is DETERMINISTIC:
//!    `runtime_key = sha256(advertise_url)` — stateless re-registration,
//!    no ids pasted anywhere. THQ mints a random secret, stores only its
//!    hash, and…
//! 2. …**CALLS BACK** `POST {advertise_url}/thq/enroll` with
//!    `{thq_url, runtime_id, instance_id, secret}`. This module SERVES
//!    that route ([`enroll_route`]). The callback's `thq_url` origin must
//!    equal the configured origin (rogue-THQ guard — a THQ that is not the
//!    one in the config must never hand us a secret), and `instance_id` /
//!    `runtime_id` must match. The secret is persisted to
//!    `~/.trustee/thq/runtime.json` (0700/0600) — PROCESS-level, its own
//!    `thq/` namespace, never under `users/`, firewalled from agent/user
//!    secrets. Self-signed TLS accepted both ways (10s timeouts).
//! 3. **PULL**: `GET {thq_url}/api/v1/runtime/profiles` with
//!    `X-Instance-Id` + `X-Runtime-Id` + `X-Runtime-Secret` — every profile
//!    bound to this runtime with its full identity payload. Scope of THIS
//!    release: the payload is LOGGED (wire-capture), never applied —
//!    materialization is the next dispatch.
//! 4. **STATE**: `POST {thq_url}/api/v1/runtime/profiles/{id}/state` with
//!    `{observed_state, detail}` per bound profile. Any report flips the
//!    runtime `pending → active` (verified thq runtime_api.rs).
//!
//! A **403 anywhere** ⇒ the secret rotated or was revoked ⇒ drop the
//! credential and re-register (self-heal). Never periodic re-registration:
//! the loop re-registers exactly when it has no valid credential.
//!
//! # Config ([thq], main process trustee.toml ONLY — per-user overlays no
//! longer drive this lane)
//!
//! ```toml
//! [thq]
//! thq_url = "https://thq.tanbal.ir"      # origin ONLY (no path/query)
//! instance_id = "<THQ leaf uuid>"         # the X-Instance-Id value
//! advertise_url = "https://10.99.0.11:3000"
//! runtime_name = "nox"                    # THQ entity "Runtime: nox"
//! heartbeat_interval = 30                 # pull/state cadence, seconds
//! ```
//!
//! Exactly these five keys. No compatibility code (no `torpi_url`, no
//! dead-key tolerance — configs are migrated, not translated): unknown or
//! missing keys fail LOUD at boot with the lane disabled.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use axum::response::IntoResponse;
use axum::Json;

/// sha256 hex of the advertise URL — the runtime's deterministic identity.
/// MUST match thq's `runtime_register::runtime_key` byte-for-byte.
pub fn runtime_key(advertise_url: &str) -> String {
    let mut h = Sha256::new();
    h.update(advertise_url.as_bytes());
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The ONLY form of the pull payload that may ever reach a log line: every
/// profile's `secrets` value is replaced by a length note. Secret material
/// travels to the credential-holder on the wire — never into logs
/// (v0.19, owner + Paydar: the 0.18 payload log printed the full secrets
/// JWT at info level).
pub fn redact_payload(payload: &serde_json::Value) -> serde_json::Value {
    let mut out = payload.clone();
    if let Some(profiles) = out.get_mut("profiles").and_then(|v| v.as_array_mut()) {
        for p in profiles.iter_mut() {
            if let Some(secret) = p.get("secrets").and_then(|v| v.as_str()) {
                let len = secret.len();
                p["secrets"] = serde_json::json!(if len > 0 {
                    format!("<redacted:{len} chars>")
                } else {
                    String::new()
                });
            }
        }
    }
    out
}

/// The origin (`scheme://host[:port]`) of a URL, or None if it does not
/// parse / has no host / is not http(s).
fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url.trim()).ok()?;
    let scheme = u.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = u.host_str()?;
    let host = if host.contains(':') {
        // bare IPv6 literal — re-bracket for a canonical origin string
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some(format!("{scheme}://{host}{port}"))
}

/// The process-level runtime credential: `~/.trustee/thq/runtime.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeCredential {
    /// sha256(advertise_url) — COMPUTED at boot, never pasted. Stored so a
    /// stale secret (advertise_url changed) is detectable without the
    /// config at read time.
    pub runtime_key: String,
    /// The enrollment secret, delivered out-of-band by THQ's callback.
    pub secret: String,
}

/// Default credential location: `~/.trustee/thq/runtime.json`. Process-
/// level namespace, deliberately NOT under `users/`.
pub fn default_credential_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".trustee")
        .join("thq")
        .join("runtime.json")
}

/// Write `bytes` to `path` with 0700 on the parent dir and 0600 on the
/// file (unix). The secret file is process-private, full stop.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Configuration for the enrollment lane, parsed from the MAIN process
/// `[thq]` section. Exactly five keys; anything else is a config error.
#[derive(Debug, Clone)]
pub struct ThqConfig {
    /// THQ's origin — scheme://host[:port], NOTHING else. (A `?instance=`
    /// here is the 405 bug that started this rewrite.)
    pub thq_url: String,
    /// The THQ leaf instance id (`X-Instance-Id` on every call).
    pub instance_id: String,
    /// This runtime's externally-reachable URL — the callback target and
    /// the identity source (`runtime_key = sha256(advertise_url)`).
    pub advertise_url: String,
    /// The runtime's name — vocabulary is RUNTIME (never agent): maps to
    /// the THQ register body `{name}` and the entity "Runtime: {name}".
    pub runtime_name: String,
    /// Pull/state cadence in seconds (default 30).
    pub heartbeat_interval: u64,
}

/// Exactly the keys the enrollment lane knows. Anything else in `[thq]` —
/// including registration-era leftovers (`torpi_url`, `agent_name`, …) —
/// is a LOUD config error: the no-compat ruling (configs are migrated, not
/// translated by code).
const KNOWN_KEYS: [&str; 5] = [
    "thq_url",
    "instance_id",
    "advertise_url",
    "runtime_name",
    "heartbeat_interval",
];

impl ThqConfig {
    /// Parse the main process `[thq]` section.
    ///
    /// - `Ok(None)` — no `[thq]` section: the lane is simply off.
    /// - `Err(msg)` — `[thq]` is present but malformed: loud at boot, lane
    ///   disabled (never a silent skip).
    /// - `Ok(Some(cfg))` — the five-key config, validated and normalized.
    pub fn from_toml(config_toml: &str) -> Result<Option<Self>, String> {
        let table: toml::Table = toml::from_str(config_toml)
            .map_err(|e| format!("config is not valid TOML: {e}"))?;
        let Some(thq) = table.get("thq").and_then(|v| v.as_table()) else {
            return Ok(None); // no [thq] — lane off
        };

        for k in thq.keys() {
            if !KNOWN_KEYS.contains(&k.as_str()) {
                return Err(format!(
                    "unknown key [thq].{k} — the enrollment config is exactly \
                     thq_url / instance_id / advertise_url / runtime_name / \
                     heartbeat_interval (the torpi-era registration keys were \
                     REMOVED in 0.15.0; migrate the config)"
                ));
            }
        }

        let req = |key: &str| -> Result<String, String> {
            let v = thq
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("[thq].{key} is required (string, non-empty)"))?;
            Ok(v)
        };

        // thq_url: an ORIGIN, nothing else — a path or query here is the
        // 405 incident (a ?instance= made every appended route land on the
        // dashboard). Fail loud instead of trimming it away silently.
        let raw_thq_url = req("thq_url")?;
        let trimmed = raw_thq_url.trim_end_matches('/');
        let normalized = origin_of(trimmed).ok_or_else(|| {
            format!(
                "[thq].thq_url must be an http(s) ORIGIN (scheme://host[:port]) — got \
                 {raw_thq_url:?}: no path, no query, no fragment (a ?instance= here \
                 is the 405 bug; the leaf goes in [thq].instance_id)"
            )
        })?;
        let parsed = url::Url::parse(trimmed).map_err(|e| format!("[thq].thq_url: {e}"))?;
        if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(format!(
                "[thq].thq_url must be an ORIGIN — got {raw_thq_url:?}: no path/query/fragment \
                 allowed (the THQ leaf belongs in [thq].instance_id)"
            ));
        }

        let instance_id = req("instance_id")?;
        let advertise_url_raw = req("advertise_url")?;
        if !advertise_url_raw.starts_with("http") {
            return Err(format!(
                "[thq].advertise_url must be an http(s) URL (THQ calls back to it) — got \
                 {advertise_url_raw:?}"
            ));
        }
        let advertise_url = advertise_url_raw.trim_end_matches('/').to_string();
        let runtime_name = req("runtime_name")?;

        let heartbeat_interval = match thq.get("heartbeat_interval") {
            None => 30,
            Some(v) => {
                let n = v
                    .as_integer()
                    .ok_or("[thq].heartbeat_interval must be an integer (seconds)")?;
                if n < 1 {
                    return Err("[thq].heartbeat_interval must be >= 1 second".to_string());
                }
                n as u64
            }
        };

        Ok(Some(Self {
            thq_url: normalized,
            instance_id,
            advertise_url,
            runtime_name,
            heartbeat_interval,
        }))
    }
}

/// Shared handle between the `/thq/enroll` route and the enrollment loop.
#[derive(Clone)]
pub struct Enrollment(Arc<EnrollmentInner>);

struct EnrollmentInner {
    config: ThqConfig,
    /// Where the runtime credential lives (injectable for tests).
    cred_path: PathBuf,
    /// The ~/.trustee root — the materialization applier writes bound
    /// profiles under `{home}/users/{user_hash(profile_id)}/`.
    home: PathBuf,
    /// Fired when the enroll handler persists a fresh secret — wakes the
    /// loop out of its post-register wait. Correctness does NOT depend on
    /// the notify (the loop re-reads the file each cycle); it only kills
    /// up-to-one-interval of latency.
    notify: tokio::sync::Notify,
}

impl Enrollment {
    /// Production handle: credential at [`default_credential_path`].
    pub fn new(config: ThqConfig) -> Self {
        Self::with_cred_path(config, default_credential_path())
    }

    /// Testable handle with an explicit credential path. The materialization
    /// home is derived as the credential's grandparent
    /// (`~/.trustee/thq/runtime.json` → `~/.trustee`).
    pub fn with_cred_path(config: ThqConfig, cred_path: PathBuf) -> Self {
        let home = cred_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Self(Arc::new(EnrollmentInner {
            config,
            cred_path,
            home,
            notify: tokio::sync::Notify::new(),
        }))
    }

    /// sha256(advertise_url) — OUR deterministic runtime identity.
    fn key(&self) -> String {
        runtime_key(&self.0.config.advertise_url)
    }

    /// Load the credential, tolerating absence/corruption (warn + None →
    /// the loop self-heals by re-registering).
    fn load_credential(&self) -> Option<RuntimeCredential> {
        let raw = std::fs::read_to_string(&self.0.cred_path).ok()?;
        match serde_json::from_str(&raw) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!(
                    "THQ credential {} is unreadable ({e}) — will re-register",
                    self.0.cred_path.display()
                );
                None
            }
        }
    }

    /// The credential valid for THIS advertise_url — a stored key that does
    /// not match means the advertise_url changed and the stale secret
    /// belongs to another runtime identity (re-register, do not use).
    fn valid_credential(&self) -> Option<RuntimeCredential> {
        let c = self.load_credential()?;
        if c.runtime_key == self.key() {
            Some(c)
        } else {
            tracing::warn!(
                "THQ credential at {} was minted for a different advertise_url \
                 (key mismatch) — re-registering",
                self.0.cred_path.display()
            );
            None
        }
    }

    fn forget_credential(&self) {
        if let Err(e) = std::fs::remove_file(&self.0.cred_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("THQ: cannot drop credential {}: {e}", self.0.cred_path.display());
            }
        }
    }

    fn save_credential(&self, cred: &RuntimeCredential) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(cred).expect("credential serializes");
        write_private(&self.0.cred_path, &bytes)
    }

    /// Spawn the enrollment loop. Call AFTER the HTTP listener is up: the
    /// register callback must land on a live `/thq/enroll` route.
    pub fn spawn(self, state: crate::state::ServerState) {
        let enrollment = self.clone();
        let client = build_http_client();
        tokio::spawn(async move {
            let cfg = enrollment.0.config.clone();
            let key = enrollment.key();
            tracing::info!(
                "THQ enrollment lane: runtime \"{}\" (key {}…) -> {} instance {} — \
                 credential at {}",
                cfg.runtime_name,
                &key[..16],
                cfg.thq_url,
                cfg.instance_id,
                enrollment.0.cred_path.display()
            );
            let interval = Duration::from_secs(cfg.heartbeat_interval.max(1));
            loop {
                match enrollment.valid_credential() {
                    Some(cred) => match pull(&client, &cfg, &cred).await {
                        PullOutcome::Ok(payload) => {
                            // v0.19: the payload is (a) LOGGED REDACTED — the
                            // 0.18 full-payload info line printed the secrets
                            // JWT, which is exactly what must never happen —
                            // and (b) APPLIED: every bound profile is
                            // materialized as an agent-user under
                            // ~/.trustee/users/{user_hash(profile_id)}.
                            let total = payload
                                .get("total")
                                .and_then(|v| v.as_u64())
                                .unwrap_or_default();
                            tracing::info!(
                                target: "thq",
                                "THQ pull: {total} profile(s) bound — payload (secrets REDACTED): {}",
                                redact_payload(&payload)
                            );
                            let report = crate::materialize::apply_profiles(&enrollment.0.home, &payload);
                            if !report.applied.is_empty() {
                                tracing::info!(target: "thq", "THQ apply: {} profile(s) materialized", report.applied.len());
                            }
                            for (id, reason) in &report.failed {
                                tracing::warn!(target: "thq", "THQ apply: profile {id} FAILED: {reason}");
                            }

                            let busy = process_busy(&state).await;
                            let profiles = payload
                                .get("profiles")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            for p in &profiles {
                                let Some(pid) = p.get("profile_id").and_then(|v| v.as_str())
                                else {
                                    continue;
                                };
                                match report_state(&client, &cfg, &cred, pid, busy).await {
                                    StateOutcome::Ok => {}
                                    StateOutcome::Forbidden => {
                                        tracing::warn!(
                                            "THQ state report 403 on profile {pid} — \
                                             credential rotated; re-registering (self-heal)"
                                        );
                                        enrollment.forget_credential();
                                        break;
                                    }
                                    StateOutcome::Failed(e) => {
                                        tracing::warn!("THQ state report failed ({pid}): {e}")
                                    }
                                }
                            }
                            tokio::time::sleep(interval).await;
                        }
                        PullOutcome::Forbidden => {
                            tracing::warn!(
                                "THQ pull 403 — secret rotated or revoked; re-registering \
                                 (self-heal)"
                            );
                            enrollment.forget_credential();
                            // no sleep: fall straight through to register
                        }
                        PullOutcome::Failed(e) => {
                            tracing::warn!("THQ pull failed (will retry): {e}");
                            tokio::time::sleep(interval).await;
                        }
                    },
                    None => {
                        // No usable credential (first boot, advertise_url
                        // change, corruption, or a 403 purge): REGISTER ONCE
                        // and wait for the /thq/enroll callback.
                        register_once(&client, &cfg).await;
                        let notified = enrollment.0.notify.notified();
                        tokio::select! {
                            _ = notified => {}
                            _ = tokio::time::sleep(interval) => {}
                        }
                    }
                }
            }
        });
    }
}

/// Build a reqwest client that accepts self-signed certs (THQ/advertise
/// endpoints may use them; ruling 2026-09-12) with a 10s timeout — the
/// enrollment lane must never hang the boot path.
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client for THQ enrollment")
}

// ---------------------------------------------------------------------------
// The /thq/enroll receiver
// ---------------------------------------------------------------------------

/// THQ's callback payload (pinned from thq `deliver_secret`):
/// `{thq_url, runtime_id, instance_id, secret}`.
#[derive(Debug, Clone, Deserialize)]
pub struct EnrollPayload {
    pub thq_url: String,
    pub runtime_id: String,
    pub instance_id: String,
    pub secret: String,
}

/// POST /thq/enroll — THQ hands the enrollment secret to whoever actually
/// serves the advertise URL (ACME-style proof of control).
///
/// Fail-closed checks, all 403: the callback must name THIS runtime
/// (`runtime_id`), come from the CONFIGURED THQ (origin of `thq_url` —
/// rogue-THQ guard), and carry the CONFIGURED instance. Only then is the
/// secret persisted (0700/0600) and the loop woken.
async fn handle_enroll(
    axum::Extension(enrollment): axum::Extension<Enrollment>,
    Json(payload): Json<EnrollPayload>,
) -> impl IntoResponse {
    use axum::http::StatusCode;

    let cfg = &enrollment.0.config;
    let short = payload.runtime_id.chars().take(16).collect::<String>();

    if payload.runtime_id != enrollment.key() {
        tracing::warn!(
            "THQ enroll REJECTED: runtime_id {short}… is not this process \
             (advertise_url key mismatch)"
        );
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "runtime_id mismatch — not this runtime"})),
        );
    }

    // Rogue-THQ guard: whatever origin the payload claims must be exactly
    // the origin in OUR config. THQ itself sends `scheme://host/?instance=`
    // — compare ORIGINS (path/query stripped), never raw strings.
    match origin_of(&payload.thq_url) {
        Some(o) if o == cfg.thq_url => {}
        other => {
            tracing::error!(
                "THQ enroll REJECTED (rogue-THQ guard): callback origin {other:?} \
                 is not the configured thq_url {:?}",
                cfg.thq_url
            );
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "thq_url origin mismatch — not my THQ"})),
            );
        }
    }

    if payload.instance_id != cfg.instance_id {
        tracing::error!(
            "THQ enroll REJECTED: instance {} is not the configured leaf {}",
            payload.instance_id,
            cfg.instance_id
        );
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "instance_id mismatch"})),
        );
    }

    let cred = RuntimeCredential {
        runtime_key: payload.runtime_id,
        secret: payload.secret,
    };
    if let Err(e) = enrollment.save_credential(&cred) {
        tracing::error!(
            "THQ enroll: cannot persist credential to {}: {e}",
            enrollment.0.cred_path.display()
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "credential persistence failed"})),
        );
    }
    enrollment.0.notify.notify_waiters();
    tracing::info!(
        target: "thq",
        "THQ enrollment secret received from {} — runtime {} enrolled (key {short}…)",
        cfg.thq_url,
        cfg.runtime_name
    );
    (StatusCode::OK, Json(json!({"status": "enrolled"})))
}

/// The enroll route, merged into the main router. Uses an `Extension`
/// layer (process-level handle), so it merges into any `Router<S>` — same
/// shape as the xagent merge. No `[thq]` config → no route (404).
pub fn enroll_route<S: Clone + Send + Sync + 'static>(
    enrollment: Enrollment,
) -> axum::Router<S> {
    axum::Router::new()
        .route("/thq/enroll", axum::routing::post(handle_enroll))
        .layer(axum::Extension(enrollment))
}

// ---------------------------------------------------------------------------
// THQ clients: register / pull / state
// ---------------------------------------------------------------------------

/// `POST {thq_url}/api/v1/runtime/register` — open route, instance-gated.
/// Response (201) carries `{runtime_id, asset_id, status, callback}` —
/// logged for the receipt trail; the SECRET itself only ever travels on
/// the callback, never in this response.
async fn register_once(client: &reqwest::Client, cfg: &ThqConfig) {
    let url = format!("{}/api/v1/runtime/register", cfg.thq_url);
    let body = json!({
        "name": cfg.runtime_name,
        "advertise_url": cfg.advertise_url,
        "version": env!("CARGO_PKG_VERSION"),
    });
    match client
        .post(&url)
        .header("X-Instance-Id", &cfg.instance_id)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                tracing::info!(target: "thq", "THQ register accepted ({status}): {text}");
            } else {
                tracing::warn!("THQ register rejected ({status}): {}", truncate(&text, 300));
            }
        }
        Err(e) => tracing::warn!("THQ register unreachable (will retry): {e}"),
    }
}

enum PullOutcome {
    /// 200 with the profiles payload (logged verbatim by the loop).
    Ok(serde_json::Value),
    /// 403 — credential bad (rotated/revoked): self-heal by re-register.
    Forbidden,
    /// Any other failure — transient, retry next tick.
    Failed(String),
}

/// `GET {thq_url}/api/v1/runtime/profiles` — runtime-credential-ONLY (the
/// unauthenticated `?runtime_id=` form is dead since thq v0.3.2).
async fn pull(client: &reqwest::Client, cfg: &ThqConfig, cred: &RuntimeCredential) -> PullOutcome {
    let url = format!("{}/api/v1/runtime/profiles", cfg.thq_url);
    match client
        .get(&url)
        .header("X-Instance-Id", &cfg.instance_id)
        .header("X-Runtime-Id", &cred.runtime_key)
        .header("X-Runtime-Secret", &cred.secret)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.as_u16() == 403 {
                return PullOutcome::Forbidden;
            }
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                return PullOutcome::Failed(format!("{status}: {}", truncate(&text, 300)));
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => PullOutcome::Ok(v),
                Err(e) => PullOutcome::Failed(format!("payload is not JSON: {e}")),
            }
        }
        Err(e) => PullOutcome::Failed(e.to_string()),
    }
}

enum StateOutcome {
    Ok,
    Forbidden,
    Failed(String),
}

/// `POST {thq_url}/api/v1/runtime/profiles/{id}/state` — report process
/// health for one bound profile. Values sent: `running` (≥1 live session
/// in the process) / `idle`. Any report flips the runtime pending→active.
async fn report_state(
    client: &reqwest::Client,
    cfg: &ThqConfig,
    cred: &RuntimeCredential,
    profile_id: &str,
    busy: bool,
) -> StateOutcome {
    let url = format!("{}/api/v1/runtime/profiles/{profile_id}/state", cfg.thq_url);
    let observed = if busy { "running" } else { "idle" };
    let body = json!({
        "observed_state": observed,
        "version": env!("CARGO_PKG_VERSION"),
        "detail": if busy {
            "process health: live session(s) present"
        } else {
            "process health: no live sessions"
        },
    });
    match client
        .post(&url)
        .header("X-Instance-Id", &cfg.instance_id)
        .header("X-Runtime-Id", &cred.runtime_key)
        .header("X-Runtime-Secret", &cred.secret)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.as_u16() == 403 {
                return StateOutcome::Forbidden;
            }
            if status.is_success() {
                StateOutcome::Ok
            } else {
                let text = resp.text().await.unwrap_or_default();
                StateOutcome::Failed(format!("{status}: {}", truncate(&text, 300)))
            }
        }
        Err(e) => StateOutcome::Failed(e.to_string()),
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Process-wide health: busy = ANY live session in ANY user bucket is
/// Running. The runtime IS the process — per-user state is irrelevant
/// here (that is the three-concept boundary).
///
/// DashMap guards are dropped before any await (house pattern).
async fn process_busy(state: &crate::state::ServerState) -> bool {
    let mut handles = Vec::new();
    for user in state.sessions.iter() {
        for entry in user.value().sessions.iter() {
            handles.push(entry.value().session.clone());
        }
    }
    for session in handles {
        if session.lock().await.workflow_state == trustee_core::types::WorkflowState::Running {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trustee-thq3-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn full_toml() -> String {
        r#"
[thq]
thq_url = "https://thq.tanbal.ir"
instance_id = "6f635fe4-4a06-42b0-8a62-276383488a9c"
advertise_url = "https://10.99.0.11:3000"
runtime_name = "nox"
heartbeat_interval = 30
"#
        .to_string()
    }

    // ── config ──────────────────────────────────────────────────────────

    #[test]
    fn parse_full_config() {
        let cfg = ThqConfig::from_toml(&full_toml())
            .expect("parse ok")
            .expect("section present");
        assert_eq!(cfg.thq_url, "https://thq.tanbal.ir");
        assert_eq!(cfg.instance_id, "6f635fe4-4a06-42b0-8a62-276383488a9c");
        assert_eq!(cfg.advertise_url, "https://10.99.0.11:3000");
        assert_eq!(cfg.runtime_name, "nox");
        assert_eq!(cfg.heartbeat_interval, 30);
    }

    #[test]
    fn parse_defaults_heartbeat_only() {
        let toml = r#"
[thq]
thq_url = "https://thq.example.com"
instance_id = "leaf-uuid"
advertise_url = "https://10.0.0.5:3000"
runtime_name = "edge"
"#;
        let cfg = ThqConfig::from_toml(toml).unwrap().unwrap();
        assert_eq!(cfg.heartbeat_interval, 30, "default cadence");
    }

    #[test]
    fn no_section_is_lane_off() {
        assert!(ThqConfig::from_toml("[oidc]\nissuer_url = \"x\"\n")
            .unwrap()
            .is_none());
    }

    #[test]
    fn missing_required_keys_are_loud() {
        for key in ["thq_url", "instance_id", "advertise_url", "runtime_name"] {
            // build a full config minus one line
            let broken: String = full_toml()
                .lines()
                .filter(|l| !l.starts_with(&format!("{key} =")))
                .collect::<Vec<_>>()
                .join("\n");
            let err = ThqConfig::from_toml(&broken).expect_err("must be loud");
            assert!(
                err.contains(key),
                "error must name the missing key {key}: {err}"
            );
        }
    }

    /// THE 405 INCIDENT: a `?instance=` in thq_url must never boot the
    /// lane silently — it is the exact shape that swallowed every appended
    /// path and landed on the dashboard.
    #[test]
    fn thq_url_with_instance_query_is_rejected() {
        let toml = full_toml().replacen(
            "thq_url = \"https://thq.tanbal.ir\"",
            "thq_url = \"https://thq.tanbal.ir/?instance=6f635fe4-4a06-42b0-8a62-276383488a9c\"",
            1,
        );
        let err = ThqConfig::from_toml(&toml).expect_err("query in thq_url must fail loud");
        assert!(err.contains("instance_id"), "error must redirect to instance_id: {err}");
    }

    #[test]
    fn thq_url_with_path_is_rejected() {
        let toml = full_toml().replacen(
            "thq_url = \"https://thq.tanbal.ir\"",
            "thq_url = \"https://thq.tanbal.ir/api/v2\"",
            1,
        );
        assert!(ThqConfig::from_toml(&toml).is_err());
    }

    #[test]
    fn thq_url_trailing_slash_is_fine() {
        let toml = full_toml().replacen(
            "thq_url = \"https://thq.tanbal.ir\"",
            "thq_url = \"https://thq.tanbal.ir/\"",
            1,
        );
        let cfg = ThqConfig::from_toml(&toml).unwrap().unwrap();
        assert_eq!(cfg.thq_url, "https://thq.tanbal.ir", "normalized to origin");
    }

    /// No-compat ruling: the torpi-era keys are not translated, not
    /// warned-about, not tolerated — they are ERRORS (the owner migrates
    /// configs himself; code that guesses is extra code for nothing).
    #[test]
    fn registration_era_keys_are_rejected() {
        // torpi_url instead of thq_url: missing-key error names thq_url.
        let old_style = r#"
[thq]
torpi_url = "https://torpi.tanbal.ir"
advertise_url = "https://10.0.0.5:3000"
agent_name = "nox"
"#;
        let err = ThqConfig::from_toml(old_style).expect_err("must be loud");
        assert!(err.contains("thq_url"), "must name thq_url: {err}");

        // New keys present AND a leftover torpi_url: unknown-key error.
        let leftover = format!("{}\ntorpi_url = \"https://x.example\"", full_toml());
        let err = ThqConfig::from_toml(&leftover).expect_err("leftovers must fail loud");
        assert!(err.contains("torpi_url"), "error must name the leftover key: {err}");

        // agent_name (the pre-rename vocabulary) is equally foreign here.
        let leftover_agent = format!("{}\nagent_name = \"nox\"", full_toml());
        assert!(ThqConfig::from_toml(&leftover_agent).is_err());
    }

    #[test]
    fn heartbeat_zero_is_rejected() {
        let toml = full_toml().replacen("heartbeat_interval = 30", "heartbeat_interval = 0", 1);
        assert!(ThqConfig::from_toml(&toml).is_err());
    }

    #[test]
    fn advertise_url_must_be_http() {
        let toml = full_toml().replacen(
            "advertise_url = \"https://10.99.0.11:3000\"",
            "advertise_url = \"tcp://10.99.0.11:3000\"",
            1,
        );
        assert!(ThqConfig::from_toml(&toml).is_err());
    }

    // ── identity / origin ───────────────────────────────────────────────

    #[test]
    fn runtime_key_matches_sha256() {
        // Known vector: sha256("abc")
        assert_eq!(
            runtime_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(runtime_key("").len(), 64, "hex sha256");
    }

    /// THQ sends `scheme://host/?instance=<leaf>` in the callback — the
    /// rogue-THQ guard must compare ORIGINS, never raw strings.
    #[test]
    fn origin_strips_instance_query_and_path() {
        assert_eq!(
            origin_of("https://thq.tanbal.ir/?instance=abc").as_deref(),
            Some("https://thq.tanbal.ir")
        );
        assert_eq!(
            origin_of("http://localhost:18710").as_deref(),
            Some("http://localhost:18710")
        );
        assert_eq!(
            origin_of("https://thq.tanbal.ir:8443/x?y=1").as_deref(),
            Some("https://thq.tanbal.ir:8443")
        );
        assert_eq!(origin_of("not a url").as_deref(), None);
        assert_eq!(origin_of("ftp://host/").as_deref(), None);
    }

    // ── credential store ────────────────────────────────────────────────

    fn cfg() -> ThqConfig {
        ThqConfig::from_toml(&full_toml()).unwrap().unwrap()
    }

    #[test]
    fn credential_roundtrip_private() {
        let dir = tmp("cred-round");
        let path = dir.join("thq").join("runtime.json");
        let enr = Enrollment::with_cred_path(cfg(), path.clone());
        let cred = RuntimeCredential {
            runtime_key: enr.key(),
            secret: "s3cret-value".to_string(),
        };
        enr.save_credential(&cred).unwrap();

        let loaded = enr.load_credential().expect("loads");
        assert_eq!(loaded, cred);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600, "secret file is owner-only");
            let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "thq/ namespace is owner-only");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_credential_yields_none() {
        let dir = tmp("cred-corrupt");
        let path = dir.join("runtime.json");
        std::fs::write(&path, "{not json").unwrap();
        let enr = Enrollment::with_cred_path(cfg(), path);
        assert!(enr.load_credential().is_none(), "corruption self-heals via re-register");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// advertise_url changed ⇒ stored key no longer matches ⇒ the stale
    /// secret is for ANOTHER runtime identity: invalid, must re-register.
    #[test]
    fn advertise_change_invalidates_stale_credential() {
        let dir = tmp("cred-stale");
        let path = dir.join("runtime.json");
        let enr = Enrollment::with_cred_path(cfg(), path.clone());
        let stale = RuntimeCredential {
            runtime_key: runtime_key("https://OLD-ADVERTISE:3000"),
            secret: "old".to_string(),
        };
        enr.save_credential(&stale).unwrap();
        assert!(
            enr.valid_credential().is_none(),
            "stale-key credential must be treated as absent"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forget_credential_is_tolerant() {
        let dir = tmp("cred-forget");
        let enr = Enrollment::with_cred_path(cfg(), dir.join("runtime.json"));
        enr.forget_credential(); // absent file — no panic
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── /thq/enroll receiver ────────────────────────────────────────────

    use axum::response::Response;

    async fn enroll(enr: &Enrollment, payload: &EnrollPayload) -> Response {
        handle_enroll(axum::Extension(enr.clone()), Json(payload.clone()))
            .await
            .into_response()
    }

    fn enroll_payload(enr: &Enrollment, thq_url: &str, instance: &str) -> EnrollPayload {
        EnrollPayload {
            thq_url: thq_url.to_string(),
            runtime_id: enr.key(),
            instance_id: instance.to_string(),
            secret: "fresh-secret".to_string(),
        }
    }

    #[tokio::test]
    async fn enroll_accepts_configured_thq_and_persists() {
        let dir = tmp("enroll-ok");
        let path = dir.join("runtime.json");
        let enr = Enrollment::with_cred_path(cfg(), path.clone());
        // THQ's live callback shape: origin + /?instance=<leaf>
        let payload = enroll_payload(
            &enr,
            "https://thq.tanbal.ir/?instance=6f635fe4-4a06-42b0-8a62-276383488a9c",
            "6f635fe4-4a06-42b0-8a62-276383488a9c",
        );
        let resp = enroll(&enr, &payload).await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let cred = enr.load_credential().expect("credential persisted");
        assert_eq!(cred.runtime_key, enr.key());
        assert_eq!(cred.secret, "fresh-secret");
        assert!(enr.valid_credential().is_some(), "immediately usable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn enroll_rejects_rogue_thq_origin() {
        let dir = tmp("enroll-rogue");
        let path = dir.join("runtime.json");
        let enr = Enrollment::with_cred_path(cfg(), path.clone());
        let payload = enroll_payload(&enr, "https://rogue.example/?instance=leaf", "6f635fe4-4a06-42b0-8a62-276383488a9c");
        let resp = enroll(&enr, &payload).await;
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(
            enr.load_credential().is_none(),
            "a rogue THQ must never persist anything"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn enroll_rejects_foreign_runtime_id() {
        let dir = tmp("enroll-wrongid");
        let enr = Enrollment::with_cred_path(cfg(), dir.join("runtime.json"));
        let mut payload = enroll_payload(
            &enr,
            "https://thq.tanbal.ir/?instance=6f635fe4-4a06-42b0-8a62-276383488a9c",
            "6f635fe4-4a06-42b0-8a62-276383488a9c",
        );
        payload.runtime_id = runtime_key("https://SOMEONE-ELSE:3000");
        let resp = enroll(&enr, &payload).await;
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(enr.load_credential().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn enroll_rejects_wrong_instance() {
        let dir = tmp("enroll-wronginst");
        let enr = Enrollment::with_cred_path(cfg(), dir.join("runtime.json"));
        let payload = enroll_payload(&enr, "https://thq.tanbal.ir/?instance=other-leaf", "other-leaf");
        let resp = enroll(&enr, &payload).await;
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(enr.load_credential().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── process health ──────────────────────────────────────────────────

    fn open_state() -> crate::state::ServerState {
        let (session, _rx) = trustee_core::session::Session::new();
        let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(16);
        crate::state::ServerState::new(session, ws_tx, None)
    }

    #[tokio::test]
    async fn fresh_process_is_not_busy() {
        let state = open_state();
        assert!(!process_busy(&state).await);
    }

    #[tokio::test]
    async fn any_running_session_makes_process_busy() {
        let state = open_state();
        // default bucket, first session — flip it to Running
        let session = {
            let user = state.sessions.get("default").unwrap();
            let entry = user.value().sessions.get("default").unwrap();
            entry.value().session.clone()
        };
        session.lock().await.workflow_state = trustee_core::types::WorkflowState::Running;
        assert!(process_busy(&state).await);
    }
}

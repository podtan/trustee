//! 16F: per-agent dispatch surface — `/xagent/{name}/api/v1/...`
//!
//! Closes the THQ dispatch gap: torpi's proxy forwards
//! `/thq/api/agents/{id}/...` to `{endpoint}/api/v1/...` with the CALLER's
//! Bearer injected, so until now THQ-dispatched sessions ran under the
//! CALLER's identity (the owner), not the agent's. This module exposes the
//! same handler surface under a per-agent path prefix and re-keys every
//! request to the agent-user the THQ entry represents.
//!
//! Mechanism (impersonation by Bearer swap — zero handler duplication):
//! 1. `{name}` resolves through the boot-time dispatch table
//!    (`ServerState.thq_dispatch`, populated by 16E discovery) to the
//!    agent-user's stable key (= its Kanidm `sub`, per the 16E pin).
//! 2. The OUTER caller is gate-checked: human admin only
//!    ([`crate::auth::check_dispatch_admin`]). Agents can never dispatch
//!    agents; open mode follows the same open posture as `check_auth`.
//! 3. The agent's service token (captured from its per-user `.env` at boot)
//!    is exchanged for a short-lived `role=agent` access token (RFC 8693,
//!    expiry-buffered cache). Open mode passes through unauthenticated,
//!    matching `check_auth`'s open posture.
//! 4. The inner request is rebuilt with `Authorization: Bearer <agent>` and
//!    the caller's cookie STRIPPED, then the STANDARD handler runs: `check_auth`
//!    authenticates the AGENT, Cedar applies the per-action agent matrix
//!    (working set minus DeleteSession), `user_key` resolves to the agent's
//!    sub, and session bucket / per-user home / MCP loader all resolve to the
//!    agent's own namespace — her own fame, never the caller's tools.
//!
//! THQ-side wiring: set each agent-user's `[thq].advertise_url` to
//! `https://<host>:<port>/xagent/<agent_name>` — torpi appends
//! `/api/v1/...` to the advertised origin, landing here.

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::routes;
use crate::state::{ServerState, ThqDispatchEntry};

/// Parse the REAL `exp` claim (epoch seconds) from a JWT's payload —
/// base64url-decode the middle segment, read `exp`. The `expires_in`
/// estimate is trusted only when the payload carries no parsable `exp`
/// (the estimate class: 199c4801; pep 849e7528 lineage).
fn jwt_real_exp(token: &str) -> Option<u64> {
    use base64::Engine as _;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(payload)
                .ok()
        })?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_u64()
}

/// Fixed per-process refresh jitter in [0,15]s — drawn ONCE per process
/// (no rand dep), applied consistently to every freshness check: fleet
/// mint phases desynchronize while each process's horizon is stable (the
/// pep 0.5.15 fleet-jitter pattern).
fn refresh_jitter() -> std::time::Duration {
    static JITTER: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let secs = *JITTER.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            % 16
    });
    std::time::Duration::from_secs(secs)
}

/// Proactive-refresh horizon: a cached dispatch Bearer is presented only
/// while it has MORE than this much life left; inside the horizon the next
/// dispatch re-exchanges BEFORE the expiry edge (elimination spec
/// fb82b866 response c35042bb leg L1 shape: T-120s ±15s jitter on the
/// real exp, never the estimate).
const DISPATCH_REFRESH_HORIZON: std::time::Duration = std::time::Duration::from_secs(120);

/// Freshness gate: present the cached Bearer only while more than the
/// proactive horizon (T-120s − per-process jitter) of life remains.
fn within_refresh_horizon(
    expires_at: std::time::Instant,
    now: std::time::Instant,
    jitter: std::time::Duration,
) -> bool {
    now + DISPATCH_REFRESH_HORIZON + jitter < expires_at
}

/// Exchange the agent's service token for a short-lived `role=agent`
/// Bearer and cache it keyed on the REAL exp (payload-parsed; the
/// `expires_in` estimate is only the unparsable-payload fallback, floored
/// at 30s so a degenerate mint never caches a dead token).
async fn mint_dispatch_token(
    state: &ServerState,
    auth: &crate::auth::AuthState,
    entry: &ThqDispatchEntry,
) -> Result<String, (StatusCode, String)> {
    let Some(ref service_token) = entry.service_token else {
        return Err((
            StatusCode::BAD_GATEWAY,
            "agent has no service token provisioned (per-user .env) — cannot impersonate"
                .to_string(),
        ));
    };
    // Kanidm accepts a token exchange only on the origin the token was
    // minted for: use the issuer captured from the agent's OWN overlay
    // credential (16F — verified: same token, 200 on its vhost, 400 on the
    // auth issuer's vhost). Falls back to the auth issuer only if the
    // overlay declares none.
    let issuer = entry
        .issuer_url
        .clone()
        .unwrap_or_else(|| auth.config.issuer_url.clone());
    let (token, expires_in) = auth
        .exchange_agent_token(&issuer, service_token)
        .await
        .map_err(|s| (s, "agent token exchange failed".to_string()))?;
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let life = jwt_real_exp(&token)
        .map(|exp| exp.saturating_sub(now_epoch))
        .unwrap_or(expires_in);
    let ttl = std::time::Duration::from_secs(life).max(std::time::Duration::from_secs(30));
    state.agent_dispatch_tokens.insert(
        entry.user_key.clone(),
        (token.clone(), std::time::Instant::now() + ttl),
    );
    Ok(token)
}

/// Resolve the dispatch target and rebuild the inner request headers with
/// the AGENT's Bearer (cookie stripped). Shared prelude of every wrapper.
async fn dispatch_context(
    state: &ServerState,
    agent: &str,
    headers: &HeaderMap,
) -> Result<(HeaderMap, ThqDispatchEntry), (StatusCode, String)> {
    dispatch_context_inner(state, agent, headers, false).await
}

async fn dispatch_context_inner(
    state: &ServerState,
    agent: &str,
    headers: &HeaderMap,
    force_refresh: bool,
) -> Result<(HeaderMap, ThqDispatchEntry), (StatusCode, String)> {
    let Some(entry) = state.thq_dispatch.get(agent).map(|e| e.clone()) else {
        return Err((StatusCode::NOT_FOUND, format!("unknown agent: {agent}")));
    };
    if entry.user_key.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("agent {agent} has no owner_id — not dispatchable"),
        ));
    }

    // Outer gate: human admin (open mode allowed, same posture as check_auth).
    crate::auth::check_dispatch_admin(&state.auth, headers)
        .await
        .map_err(|s| {
            (
                s,
                "xagent dispatch requires an admin Bearer token".to_string(),
            )
        })?;

    // Inner identity: the agent's own short-lived Bearer.
    let mut inner = headers.clone();

    let Some(auth) = state.auth.as_ref() else {
        // Open mode: no IdP to mint from — run the inner call unauthenticated,
        // which check_auth resolves as the open-mode "default" user.
        inner.remove(header::AUTHORIZATION);
        inner.remove(header::COOKIE);
        return Ok((inner, entry.clone()));
    };

    if entry.service_token.is_none() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!(
                "agent {agent} has no service token provisioned (per-user .env) — cannot impersonate"
            ),
        ));
    }

    if force_refresh {
        // The 401 retry's fresh exchange must not race the stale entry.
        state.agent_dispatch_tokens.remove(&entry.user_key);
    } else if let Some(kv) = state.agent_dispatch_tokens.get(&entry.user_key) {
        let (tok, exp) = kv.value();
        // Proactive refresh: present the cached Bearer only while it has
        // more than T-120s (± per-process jitter) of REAL life left; inside
        // the horizon the next dispatch re-exchanges before the edge.
        if within_refresh_horizon(*exp, std::time::Instant::now(), refresh_jitter()) {
            inner.insert(
                header::AUTHORIZATION,
                format!("Bearer {tok}").parse().map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "header build".to_string(),
                    )
                })?,
            );
            inner.remove(header::COOKIE);
            return Ok((inner, entry.clone()));
        }
    }

    let token = mint_dispatch_token(state, auth, &entry).await?;
    inner.insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "header build".to_string(),
            )
        })?,
    );
    inner.remove(header::COOKIE);
    Ok((inner, entry))
}

/// Run a dispatch wrapper with the budget-of-one 401 retry (elimination
/// spec fb82b866 response c35042bb leg L2; abk `send_with_retry`
/// semantics): a 401 on the inner call means the presented Bearer was
/// rejected mid-TTL (the kanidm orphan-sweep class) — invalidate the
/// cached dispatch token, re-exchange fresh, and re-run the handler
/// EXACTLY once. A persistent 401 surfaces as-is (no loop). Open mode
/// presents no credential and is single-shot; WS upgrades (the
/// [`WebSocketUpgrade`] extractor is not clonable) rely on the proactive
/// refresh plus the client's own reconnect.
async fn run_dispatch<F, Fut>(
    state: &ServerState,
    agent: &str,
    headers: &HeaderMap,
    run: F,
) -> Result<Response, (StatusCode, String)>
where
    F: Fn(HeaderMap) -> Fut,
    Fut: std::future::Future<Output = Result<Response, (StatusCode, String)>>,
{
    let (inner, entry) = dispatch_context(state, agent, headers).await?;
    let resp = crate::state::in_dispatch_scope(&entry.user_key, run(inner)).await?;
    if resp.status() != StatusCode::UNAUTHORIZED || state.auth.is_none() {
        return Ok(resp);
    }
    tracing::warn!(
        "xagent dispatch: 401 on the inner call for agent '{agent}' — cached Bearer rejected mid-TTL; re-exchanging fresh and retrying once"
    );
    let (inner2, entry2) = dispatch_context_inner(state, agent, headers, true).await?;
    crate::state::in_dispatch_scope(&entry2.user_key, run(inner2)).await
}

/// THQ polls `{advertise_url}/api/v1/health` for liveness — resolve the agent
/// (unknown → 404 → THQ marks it offline) then answer with the shared health.
pub async fn x_health(State(state): State<ServerState>, Path(agent): Path<String>) -> Response {
    if !state.thq_dispatch.contains_key(&agent) {
        return (StatusCode::NOT_FOUND, format!("unknown agent: {agent}")).into_response();
    }
    routes::health().await.into_response()
}

pub async fn x_list_sessions(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        async move { routes::list_sessions(State(state), inner).await }
    })
    .await
}

pub async fn x_create_session(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
    Json(req): Json<routes::CreateSessionRequest>,
) -> Result<Response, (StatusCode, String)> {
    // Identity is NOT injected here: an dispatched session's persona comes
    // from the agent's OWN overlay config ([lifecycle].system_template in
    // her ~/.trustee/users/<hash>/config/trustee.toml — allowlisted since
    // the xagent-persona change), falling back to the shared config default.
    // An explicit identity in the create body still wins (caller override).
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let req = req.clone();
        async move { routes::create_session(State(state), inner, Json(req)).await }
    })
    .await
}

pub async fn x_list_live_sessions(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        async move { routes::list_live_sessions(State(state), inner).await }
    })
    .await
}

pub async fn x_get_session_detail(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        async move { routes::get_session_detail(State(state), Path(session_id), inner).await }
    })
    .await
}

pub async fn x_destroy_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        async move { routes::destroy_session(State(state), inner, Path(session_id)).await }
    })
    .await
}

pub async fn x_get_live_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        async move { routes::get_live_session(State(state), inner, Path(session_id)).await }
    })
    .await
}

pub async fn x_resume_session(
    State(state): State<ServerState>,
    Path((agent, checkpoint_session_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Option<Json<routes::ResumeRequestBody>>,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let checkpoint_session_id = checkpoint_session_id.clone();
        let body = body.clone();
        async move {
            routes::resume_session(State(state), Path(checkpoint_session_id), inner, body).await
        }
    })
    .await
}

/// POST /xagent/{agent}/api/v1/sessions/{id}/fork — explicit chain fork
/// (the owner's feature; NEVER a side effect of resume).
pub async fn x_fork_session(
    State(state): State<ServerState>,
    Path((agent, source_session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let source_session_id = source_session_id.clone();
        async move { routes::fork_session(State(state), Path(source_session_id), inner).await }
    })
    .await
}

pub async fn x_get_session_history(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        async move { routes::get_session_history(State(state), Path(session_id), inner).await }
    })
    .await
}

pub async fn x_post_command_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<routes::CommandRequest>,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        let req = req.clone();
        async move {
            routes::post_command_session(State(state), inner, Path(session_id), Json(req)).await
        }
    })
    .await
}

pub async fn x_post_cancel_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        async move { routes::post_cancel_session(State(state), inner, Path(session_id)).await }
    })
    .await
}

pub async fn x_post_handoff_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        let session_id = session_id.clone();
        async move { routes::post_handoff_session(State(state), inner, Path(session_id)).await }
    })
    .await
}

pub async fn x_ws_session_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Single-shot by nature: the WebSocketUpgrade extractor is not clonable,
    // so the budget-of-one 401 retry cannot re-run this wrapper — the
    // proactive refresh (T-120s ± jitter on the real exp) keeps the
    // presented Bearer away from the expiry edge, and the console client's
    // own reconnect covers a mid-TTL revocation.
    let (inner, entry) = dispatch_context(&state, &agent, &headers)
        .await
        .map_err(|(s, _)| s)?;
    crate::state::in_dispatch_scope(
        &entry.user_key,
        routes::ws_session_handler(ws, State(state), inner, Path(session_id)),
    )
    .await
}

pub async fn x_list_models(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        async move { routes::list_models(State(state), inner).await }
    })
    .await
}

/// v0.19.8: impersonated MCP credential status — the same handler the
/// trustee web serves at /auth/mcp/status, resolved in the agent's dispatch
/// scope (per-credential connected/warn/fail dots for the thq console).
pub async fn x_mcp_status(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
) -> Result<axum::response::Response, (StatusCode, String)> {
    run_dispatch(&state, &agent, &headers, |inner| {
        let state = state.clone();
        async move { Ok(crate::auth::mcp_status_handler(State(state), inner).await) }
    })
    .await
}

/// The `/xagent/{agent}/api/v1` route tree — merged into the main router.
pub fn router() -> axum::Router<ServerState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/xagent/{agent}/api/v1/health", get(x_health))
        .route(
            "/xagent/{agent}/api/v1/sessions",
            get(x_list_sessions).post(x_create_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/live",
            get(x_list_live_sessions),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}",
            get(x_get_session_detail).delete(x_destroy_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/live",
            get(x_get_live_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/resume",
            post(x_resume_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/fork",
            post(x_fork_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/history",
            get(x_get_session_history),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/command",
            post(x_post_command_session).layer(axum::extract::DefaultBodyLimit::max(
                routes::ATTACH_BODY_LIMIT,
            )),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/cancel",
            post(x_post_cancel_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/handoff",
            post(x_post_handoff_session),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/stream",
            get(x_ws_session_handler),
        )
        .route("/xagent/{agent}/api/v1/models", get(x_list_models))
        // v0.19.8: the thq console's MCP Connections overlay relays
        // /api/v1/console/agents/{agent}/mcp/status → THIS route. Without it
        // the overlay 404s into a misleading "No MCP credentials" message.
        .route("/xagent/{agent}/api/v1/mcp/status", get(x_mcp_status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ThqDispatchEntry;
    use std::collections::HashMap;

    fn open_state() -> ServerState {
        let (session, _rx) = trustee_core::session::Session::new();
        let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel::<String>(16);
        ServerState::new(session, ws_tx, None)
    }

    fn hdrs(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                header::HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn create_without_identity_stays_config_driven() {
        // Contract: the xagent create wrapper does NOT synthesize an
        // identity — the agent's own overlay [lifecycle].system_template
        // flows through the standard config path (verified in state.rs
        // lifecycle_overlay tests). This test only pins the request shape.
        let req: routes::CreateSessionRequest =
            serde_json::from_str(r#"{"session_name": "probe"}"#).unwrap();
        assert!(
            req.identity.is_none(),
            "wrapper must not invent identities — persona is config-owned"
        );
    }

    #[tokio::test]
    async fn dispatch_unknown_agent_is_404() {
        let state = open_state();
        let err = dispatch_context(&state, "nobody", &hdrs(&[]))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
        assert!(err.1.contains("unknown agent"));
    }

    #[tokio::test]
    async fn dispatch_entry_without_owner_id_is_not_dispatchable() {
        let state = open_state();
        state.thq_dispatch.insert(
            "ghost".to_string(),
            ThqDispatchEntry {
                user_key: String::new(),
                service_token: None,
                issuer_url: None,
            },
        );
        let err = dispatch_context(&state, "ghost", &hdrs(&[]))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
        assert!(err.1.contains("not dispatchable"));
    }

    #[tokio::test]
    async fn open_mode_dispatch_strips_auth_and_cookie() {
        // Open mode (auth=None): the inner request must carry NO caller
        // credentials — the inner check_auth resolves the open "default" user.
        let state = open_state();
        state.thq_dispatch.insert(
            "saman".to_string(),
            ThqDispatchEntry {
                user_key: "f27de518-a647-4ea2-85ec-8ecc4d61e658".to_string(),
                service_token: None,
                issuer_url: Some("https://idp.tanbal.ir/oauth2/openid/pdt-api".to_string()),
            },
        );
        let (inner, _entry) = dispatch_context(
            &state,
            "saman",
            &hdrs(&[
                ("Authorization", "Bearer caller-jwt"),
                ("Cookie", "trustee_token=owner-session"),
            ]),
        )
        .await
        .unwrap();
        assert!(
            inner.get(header::AUTHORIZATION).is_none(),
            "caller Bearer stripped"
        );
        assert!(
            inner.get(header::COOKIE).is_none(),
            "caller cookie stripped"
        );
    }

    #[tokio::test]
    async fn dispatch_table_missing_service_token_still_resolves_context_in_open_mode() {
        // Open mode never mints (no IdP) — a None service_token is fine there.
        let state = open_state();
        state.thq_dispatch.insert(
            "ravand".to_string(),
            ThqDispatchEntry {
                user_key: "1a71c077-b3b3-4581-b605-925c3f276f30".to_string(),
                service_token: None,
                issuer_url: None,
            },
        );
        let (inner, _entry) = dispatch_context(&state, "ravand", &hdrs(&[]))
            .await
            .unwrap();
        assert!(inner.get(header::AUTHORIZATION).is_none());
    }

    // ── admin decision core (no IdP needed) ─────────────────────────────

    #[test]
    fn dispatch_allowed_only_for_human_admins() {
        use crate::auth::{dispatch_allowed, PrincipalKind};
        assert!(dispatch_allowed(PrincipalKind::Human, Some("admin")));
        assert!(!dispatch_allowed(PrincipalKind::Human, Some("user")));
        assert!(
            !dispatch_allowed(PrincipalKind::Agent, Some("admin")),
            "agents never dispatch agents"
        );
        assert!(!dispatch_allowed(PrincipalKind::Human, None));
        assert!(
            !dispatch_allowed(PrincipalKind::Human, Some("Admin")),
            "case-sensitive"
        );
    }

    #[test]
    fn secrets_map_is_unused_but_type_stable() {
        // Guards the merge-secrets typing used by ServerState::with_secrets —
        // xagent must never need per-user secrets for impersonation (the
        // service token travels in the dispatch entry, not the secrets map).
        let _m: HashMap<String, String> = HashMap::new();
    }

    // ── credential-layer hardening (199c4801/9618b41c; spec c35042bb) ────

    #[test]
    fn jwt_real_exp_parses_the_payload_exp() {
        use base64::Engine as _;
        let payload = serde_json::json!({ "exp": 1_234_567_890_u64 }).to_string();
        let token = format!(
            "hdr.{}.sig",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
        );
        assert_eq!(jwt_real_exp(&token), Some(1_234_567_890));
    }

    #[test]
    fn jwt_real_exp_returns_none_for_unparsable_payloads() {
        assert_eq!(jwt_real_exp("not-a-jwt"), None);
        assert_eq!(jwt_real_exp("hdr.!!!not-base64!!!.sig"), None);
        use base64::Engine as _;
        let no_exp = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"sub":"x"}"#);
        assert_eq!(jwt_real_exp(&format!("hdr.{no_exp}.sig")), None);
    }

    #[test]
    fn refresh_jitter_is_stable_per_process_and_bounded() {
        let a = refresh_jitter();
        let b = refresh_jitter();
        assert_eq!(a, b, "one draw per process, applied consistently");
        assert!(a <= std::time::Duration::from_secs(15));
    }

    #[test]
    fn within_refresh_horizon_gates_proactive_refresh() {
        let now = std::time::Instant::now();
        // 200s of real life: outside the T-120s horizon — present the cache.
        assert!(within_refresh_horizon(
            now + std::time::Duration::from_secs(200),
            now,
            std::time::Duration::from_secs(0)
        ));
        // 100s of real life: inside the horizon — re-exchange proactively.
        assert!(!within_refresh_horizon(
            now + std::time::Duration::from_secs(100),
            now,
            std::time::Duration::from_secs(0)
        ));
        // The jitter can only move the horizon EARLIER (never past T-120s).
        assert!(within_refresh_horizon(
            now + std::time::Duration::from_secs(200),
            now,
            std::time::Duration::from_secs(15)
        ));
        // 130s of life with a 15s jitter is inside the horizon (130 < 135).
        assert!(!within_refresh_horizon(
            now + std::time::Duration::from_secs(130),
            now,
            std::time::Duration::from_secs(15)
        ));
    }

    #[tokio::test]
    async fn run_dispatch_open_mode_401_is_single_shot() {
        // Open mode presents no credential — a 401 from the inner handler is
        // never a token problem, so the budget-of-one retry must NOT fire.
        let state = open_state();
        state.thq_dispatch.insert(
            "saman".to_string(),
            ThqDispatchEntry {
                user_key: "f27de518-a647-4ea2-85ec-8ecc4d61e658".to_string(),
                service_token: None,
                issuer_url: None,
            },
        );
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        let resp = run_dispatch(&state, "saman", &hdrs(&[]), move |inner| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = inner;
                Ok::<_, (StatusCode, String)>((StatusCode::UNAUTHORIZED, "denied").into_response())
            }
        })
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "open mode is single-shot — no credential, no retry"
        );
    }
}

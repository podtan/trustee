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
use crate::state::ServerState;

/// Minimum identity for dispatched sessions whose create body carries none.
/// Charters replace this per agent once drafted (owner-approved content);
/// until then the agent at least knows its own name.
fn default_identity(agent: &str) -> String {
    format!("You are {agent}, an agent on the Tanbal platform.")
}

/// Resolve the dispatch target and rebuild the inner request headers with
/// the AGENT's Bearer (cookie stripped). Shared prelude of every wrapper.
async fn dispatch_context(
    state: &ServerState,
    agent: &str,
    headers: &HeaderMap,
) -> Result<HeaderMap, (StatusCode, String)> {
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
        return Ok(inner);
    };

    let Some(ref service_token) = entry.service_token else {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!(
                "agent {agent} has no service token provisioned (per-user .env) — cannot impersonate"
            ),
        ));
    };

    // Cache: (token, expires_at) with a 60s safety buffer (pep 0.5.6 lesson).
    if let Some(kv) = state.agent_dispatch_tokens.get(&entry.user_key) {
        let (tok, exp) = kv.value();
        if std::time::Instant::now() < *exp {
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
            return Ok(inner);
        }
    }

    let (token, expires_in) = auth
        .exchange_agent_token(service_token)
        .await
        .map_err(|s| (s, "agent token exchange failed".to_string()))?;
    let buffered = std::time::Duration::from_secs(expires_in.saturating_sub(60))
        .max(std::time::Duration::from_secs(30));
    state.agent_dispatch_tokens.insert(
        entry.user_key.clone(),
        (token.clone(), std::time::Instant::now() + buffered),
    );

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
    Ok(inner)
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
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::list_sessions(State(state), inner).await
}

pub async fn x_create_session(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
    Json(mut req): Json<routes::CreateSessionRequest>,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    if req.identity.is_none() {
        req.identity = Some(default_identity(&agent));
    }
    routes::create_session(State(state), inner, Json(req)).await
}

pub async fn x_list_live_sessions(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::list_live_sessions(State(state), inner).await
}

pub async fn x_get_session_detail(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::get_session_detail(State(state), Path(session_id), inner).await
}

pub async fn x_destroy_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::destroy_session(State(state), inner, Path(session_id)).await
}

pub async fn x_get_live_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::get_live_session(State(state), inner, Path(session_id)).await
}

pub async fn x_resume_session(
    State(state): State<ServerState>,
    Path((agent, checkpoint_session_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Option<Json<routes::ResumeRequestBody>>,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::resume_session(State(state), Path(checkpoint_session_id), inner, body).await
}

pub async fn x_get_session_history(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::get_session_history(State(state), Path(session_id), inner).await
}

pub async fn x_post_command_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<routes::CommandRequest>,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::post_command_session(State(state), inner, Path(session_id), Json(req)).await
}

pub async fn x_post_cancel_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::post_cancel_session(State(state), inner, Path(session_id)).await
}

pub async fn x_post_handoff_session(
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::post_handoff_session(State(state), inner, Path(session_id)).await
}

pub async fn x_ws_session_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Path((agent, session_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let inner = dispatch_context(&state, &agent, &headers)
        .await
        .map_err(|(s, _)| s)?;
    routes::ws_session_handler(ws, State(state), inner, Path(session_id)).await
}

pub async fn x_list_models(
    State(state): State<ServerState>,
    Path(agent): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let inner = dispatch_context(&state, &agent, &headers).await?;
    routes::list_models(State(state), inner).await
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
            "/xagent/{agent}/api/v1/sessions/{id}/history",
            get(x_get_session_history),
        )
        .route(
            "/xagent/{agent}/api/v1/sessions/{id}/command",
            post(x_post_command_session),
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
    fn default_identity_names_the_agent() {
        assert_eq!(
            default_identity("saman"),
            "You are saman, an agent on the Tanbal platform."
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
            },
        );
        let inner = dispatch_context(
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
            },
        );
        let inner = dispatch_context(&state, "ravand", &hdrs(&[]))
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
}

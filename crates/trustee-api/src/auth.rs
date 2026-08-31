//! Authentication module for Trustee API.
//!
//! Uses PEP for OIDC/OAuth2:
//! - `ResourceServerClient` for JWT validation (offline, cached JWKS)
//! - `OidcClient` for authorization code + PKCE login flow
//! - `PkceCookieManager` for stateless PKCE state (HMAC-signed cookies)
//! - `DevConfig` for local development bypass
//!
//! Two deployment modes:
//! - **Standalone**: browser hits /auth/login → IdP redirect → /auth/callback → cookie
//! - **Centralized**: external auth app sends `Authorization: Bearer <token>` directly
//!
//! Token extraction order: `Authorization: Bearer` header → `trustee_token` cookie.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration as StdDuration;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use pep::oidc::pkce_cookie::PkceCookieManager;
use pep::oidc_client::OidcClient;
use pep::oidc_resource_server::ResourceServerClient;
use pep::session_manager::WebSessionManager;
use pep::{DevConfig, JwtClaims, JwtValidationOptions, OidcClientConfig};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::Duration as TimeDuration;

use cedar_policy::{Context, Entities, EntityUid, Request};
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Authentication configuration parsed from `[oidc]` and `[dev]` TOML sections.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// OIDC provider issuer URL
    pub issuer_url: String,
    /// OAuth2 client ID
    pub client_id: String,
    /// OAuth2 client secret (None → public client, PKCE only)
    pub client_secret: Option<String>,
    /// Redirect URI for OIDC callback
    pub redirect_uri: String,
    /// OAuth2 scopes
    pub scope: String,
    /// Token cookie name
    pub cookie_name: String,
    /// Development mode configuration
    pub dev_config: DevConfig,
    /// JWT validation options
    pub validation_options: JwtValidationOptions,
    /// Secret for signing PKCE state cookies
    pub pkce_cookie_secret: String,
}

impl AuthConfig {
    /// Parse auth config from the merged trustee TOML string.
    ///
    /// Reads `[oidc]` and `[dev]` sections. If neither is present, returns None
    /// (auth disabled — all endpoints open).
    pub fn from_toml(config_toml: &str) -> Option<Self> {
        let table: toml::Table = toml::from_str(config_toml).ok()?;

        // Check for dev mode
        let dev_config = table
            .get("dev")
            .and_then(|d| d.as_table())
            .map(|d| DevConfig {
                local_dev_mode: d
                    .get("local_dev_mode")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                local_dev_email: d
                    .get("local_dev_email")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                local_dev_name: d
                    .get("local_dev_name")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                local_dev_username: d
                    .get("local_dev_username")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });

        // Dev mode without OIDC — return early with dev-only config
        if let Some(ref dc) = dev_config {
            if dc.local_dev_mode {
                // Try to get OIDC config too (for login endpoint), but it's optional in dev mode
                let oidc = Self::parse_oidc_section(&table);
                return Some(Self {
                    issuer_url: oidc
                        .as_ref()
                        .map(|o| o.0.clone())
                        .unwrap_or_else(|| "https://auth.example.com".into()),
                    client_id: oidc
                        .as_ref()
                        .map(|o| o.1.clone())
                        .unwrap_or_else(|| "trustee".into()),
                    client_secret: oidc.as_ref().and_then(|o| o.2.clone()),
                    redirect_uri: oidc
                        .as_ref()
                        .map(|o| o.3.clone())
                        .unwrap_or_else(|| "http://localhost:3000/auth/callback".into()),
                    scope: oidc
                        .as_ref()
                        .map(|o| o.4.clone())
                        .unwrap_or_else(|| "openid profile email".into()),
                    cookie_name: "trustee_token".into(),
                    dev_config: dc.clone(),
                    validation_options: JwtValidationOptions::default(),
                    pkce_cookie_secret: oidc
                        .as_ref()
                        .map(|o| o.6.clone())
                        .unwrap_or_else(|| "trustee-default-pkce-secret-change-me".into()),
                });
            }
        }

        // Production mode — requires [oidc] section
        let (
            issuer_url,
            client_id,
            client_secret,
            redirect_uri,
            scope,
            validation_options,
            pkce_secret,
        ) = Self::parse_oidc_section(&table)?;

        Some(Self {
            issuer_url,
            client_id,
            client_secret,
            redirect_uri,
            scope,
            cookie_name: "trustee_token".into(),
            dev_config: dev_config.unwrap_or_default(),
            validation_options,
            pkce_cookie_secret: pkce_secret,
        })
    }

    /// Parse the `[oidc]` section from a TOML table.
    /// Returns (issuer_url, client_id, client_secret, redirect_uri, scope, validation_options, pkce_secret).
    fn parse_oidc_section(
        table: &toml::Table,
    ) -> Option<(
        String,
        String,
        Option<String>,
        String,
        String,
        JwtValidationOptions,
        String,
    )> {
        let oidc = table.get("oidc")?.as_table()?;

        let issuer_url = oidc.get("issuer_url")?.as_str()?.to_string();
        let client_id = oidc.get("client_id")?.as_str()?.to_string();
        let client_secret = oidc
            .get("client_secret")
            .and_then(|v| v.as_str())
            .map(String::from);
        let redirect_uri = oidc
            .get("redirect_uri")
            .or_else(|| oidc.get("redirect_url")) // backward compat
            .and_then(|v| v.as_str())
            .unwrap_or("http://localhost:3000/auth/callback")
            .to_string();
        let scope = oidc
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("openid profile email")
            .to_string();

        let mut validation_options = JwtValidationOptions::default();
        if let Some(skip) = oidc.get("skip_issuer_validation").and_then(|v| v.as_bool()) {
            validation_options.skip_issuer_validation = skip;
        }
        if let Some(skip) = oidc
            .get("skip_audience_validation")
            .and_then(|v| v.as_bool())
        {
            validation_options.skip_audience_validation = skip;
        }
        validation_options.expected_audience = oidc
            .get("expected_audience")
            .and_then(|v| v.as_str())
            .map(String::from);

        let pkce_secret = oidc
            .get("pkce_cookie_secret")
            .and_then(|v| v.as_str())
            .unwrap_or("trustee-default-pkce-secret-change-me")
            .to_string();

        Some((
            issuer_url,
            client_id,
            client_secret,
            redirect_uri,
            scope,
            validation_options,
            pkce_secret,
        ))
    }

    /// Build OIDC client configuration for PEP's OidcClient.
    pub fn oidc_client_config(&self) -> OidcClientConfig {
        OidcClientConfig {
            issuer_url: self.issuer_url.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            redirect_uri: self.redirect_uri.clone(),
            scope: self.scope.clone(),
            code_challenge_method: "S256".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Token validation cache (issue c41eb9d7)
// ---------------------------------------------------------------------------

/// Grace subtracted from a token's `exp` before its cached validation goes
/// stale. MUST exceed PEP's 60s clock-skew leeway, so a cached result is
/// never served for a token PEP itself would reject on skew.
const CACHE_EXP_GRACE_SECS: i64 = 120;

/// Upper bound on how long any cached validation stays fresh, regardless of
/// token lifetime. Bounds staleness of role/group enrichment changes.
const CACHE_TTL_SECS: i64 = 300;

/// Maximum cached tokens — bounded memory on a long-running daemon.
const CACHE_MAX_ENTRIES: usize = 1024;

/// Cache key: first 16 bytes of SHA-256(token). 128-bit truncation — no
/// practical collision, and the raw token is never stored or logged.
fn validation_cache_key(token: &str) -> [u8; 16] {
    let digest = Sha256::digest(token.as_bytes());
    let mut key = [0u8; 16];
    key.copy_from_slice(&digest[..16]);
    key
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// AUTHN-only token validation cache.
///
/// Keyed by [`validation_cache_key`], entry lifetime
/// `min(exp - CACHE_EXP_GRACE_SECS, now + CACHE_TTL_SECS)`. Never gates
/// authorization: callers MUST still run Cedar per request on the cached
/// principal (`check_auth` does). Critical sections are clone-and-insert
/// sized — the lock is never held across an await, and a concurrent
/// double-validation on a cold token is benign (one wasted validation,
/// correctness intact).
struct ValidationCache {
    inner: Mutex<ValidationCacheInner>,
}

#[derive(Default)]
struct ValidationCacheInner {
    /// token-hash → (claims, valid_until as unix seconds)
    entries: HashMap<[u8; 16], (JwtClaims, i64)>,
    /// (sub, issuer) pairs already announced once at INFO.
    first_sight: HashSet<(String, String)>,
}

impl ValidationCache {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ValidationCacheInner::default()),
        }
    }

    /// Cached claims if present and still fresh.
    fn get(&self, key: &[u8; 16], now: i64) -> Option<JwtClaims> {
        let inner = self.inner.lock().expect("validation cache poisoned");
        inner
            .entries
            .get(key)
            .filter(|(_, until)| *until > now)
            .map(|(claims, _)| claims.clone())
    }

    /// Store a fresh validation result with
    /// `valid_until = min(exp - grace, now + ttl)`. A near-expiry token is
    /// inserted harmlessly — `get` filters by time, so it is born stale.
    fn put(&self, key: [u8; 16], claims: JwtClaims, now: i64) {
        let valid_until = (claims.exp - CACHE_EXP_GRACE_SECS).min(now + CACHE_TTL_SECS);
        let mut inner = self.inner.lock().expect("validation cache poisoned");
        if inner.entries.len() >= CACHE_MAX_ENTRIES {
            // Prefer dropping expired entries; if the map is all-hot, clear
            // rather than random-evict live sessions. The cache is an
            // optimization — correctness never depends on a hit.
            inner.entries.retain(|_, (_, until)| *until > now);
            if inner.entries.len() >= CACHE_MAX_ENTRIES {
                inner.entries.clear();
            }
        }
        inner.entries.insert(key, (claims, valid_until));
    }

    /// True on first sight of (sub, issuer) — the one chance to log at INFO.
    fn mark_first_sight(&self, sub: &str, issuer: &str) -> bool {
        let mut inner = self.inner.lock().expect("validation cache poisoned");
        if inner.first_sight.len() >= CACHE_MAX_ENTRIES {
            inner.first_sight.clear();
        }
        inner
            .first_sight
            .insert((sub.to_string(), issuer.to_string()))
    }
}

/// Shared authentication state, stored in ServerState.
#[derive(Clone)]
pub struct AuthState {
    /// OIDC client for login flow (authorization code + PKCE)
    pub oidc_client: OidcClient,
    /// Resource server client for JWT validation (lazy-initialized)
    pub resource_server: ResourceServerClient,
    /// OIDC client configuration
    pub client_config: OidcClientConfig,
    /// Auth configuration
    pub config: AuthConfig,
    /// Stateless PKCE cookie manager
    pub pkce_manager: PkceCookieManager,
    /// Web session manager (cookie session_id → server-side token with auto-refresh)
    pub session_manager: Arc<WebSessionManager>,
    /// 16F: service-account issuer candidates (Kanidm vhosts agent tokens
    /// are minted on). Tokens carrying one of these fail primary validation
    /// (the `[oidc]` issuer is a different vhost) — `check_auth` falls back
    /// to validating against each candidate, never skipping issuer checks.
    /// Same IdP, same keys, no trust expansion.
    pub issuer_fallbacks: Vec<String>,
    /// Cedar authorizer for ABAC authorization (None = Cedar disabled)
    pub cedar_authorizer: Option<Arc<pep::cedar::CedarAuthorizer>>,
    /// AUTHN-only token-validation cache (issue c41eb9d7). NEVER gates Cedar:
    /// per-action authorization still runs per-request on the cached principal.
    validation_cache: Arc<ValidationCache>,
}

impl AuthState {
    /// Create new auth state from configuration.
    pub fn new(config: AuthConfig) -> Self {
        Self::with_cedar(config, None)
    }

    /// Create new auth state with optional Cedar authorizer.
    pub fn with_cedar(
        config: AuthConfig,
        cedar_authorizer: Option<Arc<pep::cedar::CedarAuthorizer>>,
    ) -> Self {
        let pkce_manager = PkceCookieManager::new(
            config.pkce_cookie_secret.as_bytes(),
            "trustee_pkce_state",
            StdDuration::from_secs(600),
        );

        let session_manager = Arc::new(WebSessionManager::new(
            OidcClient::new(),
            config.issuer_url.clone(),
            config.client_id.clone(),
            config.client_secret.clone(),
            config.scope.clone(),
        ));

        Self {
            oidc_client: OidcClient::new(),
            resource_server: ResourceServerClient::new(),
            client_config: config.oidc_client_config(),
            pkce_manager,
            session_manager,
            config,
            cedar_authorizer,
            issuer_fallbacks: Vec::new(),
            validation_cache: Arc::new(ValidationCache::new()),
        }
    }

    /// 16F: set the service-account issuer fallback candidates.
    pub fn with_issuer_fallbacks(mut self, issuers: Vec<String>) -> Self {
        self.issuer_fallbacks = issuers;
        self
    }

    /// Check if development mode is enabled.
    pub fn is_dev_mode(&self) -> bool {
        self.config.dev_config.local_dev_mode
    }

    /// Validate a JWT token using PEP's ResourceServerClient.
    pub async fn validate_token(&self, token: &str) -> anyhow::Result<JwtClaims> {
        self.validate_token_on(&self.config.issuer_url, token).await
    }

    /// 16F: primary validation, then the service issuer if that fails —
    /// cached per token (issue c41eb9d7).
    ///
    /// Kanidm stamps tokens with the vhost they were minted on: human logins
    /// carry the `[oidc]` issuer, agent tokens exchanged via the service
    /// credential carry the service issuer. Each validates only against its
    /// own — this tries both, never skipping issuer validation.
    ///
    /// Results are cached (SHA-256(token)[:16] → claims, valid
    /// `min(exp-120s, now+300s)`, cap 1024) so polling clients stop paying a
    /// double validation + log line per request. AUTHN only: callers MUST
    /// still run Cedar authorization per request (`check_auth` does).
    pub async fn validate_token_flexible(&self, token: &str) -> anyhow::Result<JwtClaims> {
        let key = validation_cache_key(token);
        let now = unix_now();
        if let Some(cached) = self.validation_cache.get(&key, now) {
            return Ok(cached);
        }
        let claims = self.validate_token_flexible_uncached(token).await?;
        self.validation_cache.put(key, claims.clone(), now);
        Ok(claims)
    }

    async fn validate_token_flexible_uncached(&self, token: &str) -> anyhow::Result<JwtClaims> {
        match self.validate_token(token).await {
            Ok(claims) => Ok(claims),
            Err(primary) => {
                let mut last = primary;
                for si in &self.issuer_fallbacks {
                    if *si == self.config.issuer_url {
                        continue; // primary already tried it
                    }
                    match self.validate_token_on(si, token).await {
                        Ok(claims) => {
                            // c41eb9d7: this used to log INFO per request — an
                            // idle-poll terminal flood. One INFO on first
                            // sight of (sub, issuer), DEBUG afterwards.
                            let first = self.validation_cache.mark_first_sight(&claims.sub, si);
                            let who = claims.preferred_username.as_deref().unwrap_or(&claims.sub);
                            if first {
                                tracing::info!(
                                    "service principal {} authenticating via service-issuer fallback (iss={})",
                                    who,
                                    si
                                );
                            } else {
                                tracing::debug!(
                                    "token validated via service-issuer fallback (iss={}) — principal {}",
                                    si,
                                    who
                                );
                            }
                            return Ok(claims);
                        }
                        Err(e) => last = e,
                    }
                }
                // Auth failure must SCREAM regardless of caller handling
                // (callers also warn on the returned error — belt and braces).
                tracing::warn!(
                    "token validation failed — primary: {last}; all service-issuer fallbacks exhausted"
                );
                Err(anyhow::anyhow!(
                    "primary: {last}; all service-issuer fallbacks exhausted"
                ))
            }
        }
    }

    /// Validate against a SPECIFIC issuer — validation AND enrichment must
    /// both run on the vhost that issued the token.
    pub async fn validate_token_on(
        &self,
        issuer_url: &str,
        token: &str,
    ) -> anyhow::Result<JwtClaims> {
        let mut claims = self
            .resource_server
            .validate_jwt_with_options(
                token,
                issuer_url,
                &self.config.client_id,
                &self.config.validation_options,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Token validation failed: {}", e))?;

        // Enrich with userinfo for role/groups (cached, no-op if already present)
        // 2de5d1eb: this used to be `let _ =` — a silent role-less principal
        // that Cedar then fail-closed DENIED with `matched policies: []` and
        // zero diagnosis. Enrichment failure must SCREAM.
        if let Err(e) = self
            .resource_server
            .enrich_claims_with_userinfo(&mut claims, token, issuer_url, None)
            .await
        {
            tracing::error!(
                "userinfo enrichment FAILED for sub {}: {} — principal carries NO role/groups; \
                 with Cedar enabled every request will be DENIED until enrichment succeeds",
                claims.sub,
                e
            );
        }

        // PEP only merges groups/role from userinfo. If name/email are missing
        // (Kanidm JWTs only contain sub), fetch them from userinfo directly.
        if claims.name.is_none() || claims.email.is_none() {
            self.fill_userinfo_fields(&mut claims, token, issuer_url)
                .await;
        }

        Ok(claims)
    }

    /// Fetch name/email/preferred_username from the OIDC userinfo endpoint
    /// and fill in any that are missing from the JWT claims.
    async fn fill_userinfo_fields(&self, claims: &mut JwtClaims, token: &str, issuer_url: &str) {
        // Derive userinfo URL from issuer
        // For Kanidm: issuer_url is the discovery endpoint,
        // userinfo is at {issuer_url}/userinfo — the token's OWN vhost
        // (16F: a token minted on the service vhost must enrich there too).
        let userinfo_url = format!("{}/userinfo", issuer_url.trim_end_matches('/'));

        let client = reqwest::Client::new();
        let resp = client
            .get(&userinfo_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await;

        let Ok(resp) = resp else {
            tracing::debug!("Userinfo request failed for name/email enrichment");
            return;
        };

        if !resp.status().is_success() {
            tracing::debug!(
                "Userinfo returned {} for name/email enrichment",
                resp.status()
            );
            return;
        }

        let Ok(userinfo): Result<serde_json::Map<String, serde_json::Value>, _> = resp.json().await
        else {
            return;
        };

        tracing::debug!("Userinfo keys: {:?}", userinfo.keys().collect::<Vec<_>>());

        if claims.name.is_none() {
            if let Some(name) = userinfo.get("name").and_then(|v| v.as_str()) {
                claims.name = Some(name.to_string());
            }
        }
        if claims.email.is_none() {
            if let Some(email) = userinfo.get("email").and_then(|v| v.as_str()) {
                claims.email = Some(email.to_string());
            }
        }
        if claims.preferred_username.is_none() {
            if let Some(uname) = userinfo.get("preferred_username").and_then(|v| v.as_str()) {
                claims.preferred_username = Some(uname.to_string());
            }
        }
    }

    /// Check Cedar authorization for the authenticated user.
    ///
    /// Returns Ok(()) if allowed (or if Cedar is not configured).
    /// Returns Err(()) if denied — caller should return 403 Forbidden.
    fn check_cedar_authorized(&self, claims: &JwtClaims, action: &str) -> Result<(), ()> {
        let Some(ref authorizer) = self.cedar_authorizer else {
            return Ok(()); // Cedar not configured — allow
        };

        // Build principal entity from JWT claims
        let principal_entity = match pep::cedar::build_principal_entity(claims) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("Cedar: failed to build principal entity: {}", e);
                return Err(());
            }
        };

        // Build entities set with principal + TrusteeApp resource
        let mut entities_vec = vec![principal_entity];

        // Add a TrusteeApp entity as the resource
        let app_uid = match EntityUid::from_str(r#"TrusteeApp::"default""#) {
            Ok(uid) => uid,
            Err(e) => {
                tracing::error!("Cedar: failed to build TrusteeApp uid: {}", e);
                return Err(());
            }
        };
        let app_entity = match cedar_policy::Entity::new(
            app_uid,
            std::collections::HashMap::new(),
            std::collections::HashSet::new(),
        ) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("Cedar: failed to build TrusteeApp entity: {}", e);
                return Err(());
            }
        };
        entities_vec.push(app_entity);

        let entities = match Entities::from_entities(entities_vec, None) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("Cedar: failed to build entities set: {}", e);
                return Err(());
            }
        };

        // Build the Cedar authorization request
        let principal_uid = match pep::cedar::build_principal_uid(claims) {
            Ok(uid) => uid,
            Err(e) => {
                tracing::error!("Cedar: failed to build principal uid: {}", e);
                return Err(());
            }
        };

        let action_uid = match EntityUid::from_str(&format!("Action::\"{action}\"")) {
            Ok(uid) => uid,
            Err(e) => {
                tracing::error!("Cedar: failed to build action uid: {}", e);
                return Err(());
            }
        };

        let resource_uid = match EntityUid::from_str(r#"TrusteeApp::"default""#) {
            Ok(uid) => uid,
            Err(e) => {
                tracing::error!("Cedar: failed to build resource uid: {}", e);
                return Err(());
            }
        };

        let request = match Request::new(
            principal_uid,
            action_uid,
            resource_uid,
            Context::empty(),
            None,
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Cedar: failed to build request: {}", e);
                return Err(());
            }
        };

        let response = authorizer.is_allowed_with_entities(&request, &entities);

        if response.allowed() {
            tracing::debug!(
                "Cedar: authorized user {} (sub={})",
                claims.email.as_deref().unwrap_or("unknown"),
                claims.sub
            );
            Ok(())
        } else {
            tracing::warn!(
                "Cedar: DENIED user {} (sub={}) — matched policies: {:?}, errors: {:?}",
                claims.email.as_deref().unwrap_or("unknown"),
                claims.sub,
                response.matched_policies(),
                response.errors()
            );
            Err(())
        }
    }
}

// ---------------------------------------------------------------------------
// Auth checking — called by protected route handlers
// ---------------------------------------------------------------------------

/// Cedar action names (P2, nghr 645809c3).
///
/// Keep in sync with `policies/trustee_schema.cedarschema` — the schema and
/// the embedded policy ship atomically with these constants; a filesystem
/// policy override referencing removed actions will deny everything (loud,
/// by design).
pub mod actions {
    pub const LIST_MODELS: &str = "ListModels";
    pub const LIST_SESSIONS: &str = "ListSessions";
    pub const VIEW_SESSION: &str = "ViewSession";
    pub const VIEW_HISTORY: &str = "ViewHistory";
    pub const CREATE_SESSION: &str = "CreateSession";
    pub const COMMAND_SESSION: &str = "CommandSession";
    pub const CANCEL_SESSION: &str = "CancelSession";
    pub const HANDOFF_SESSION: &str = "HandoffSession";
    pub const RESUME_SESSION: &str = "ResumeSession";
    pub const UPDATE_SESSION: &str = "UpdateSession";
    pub const DELETE_SESSION: &str = "DeleteSession";
    pub const VIEW_MCP_CREDENTIALS: &str = "ViewMcpCredentials";
    pub const UPDATE_MCP_CREDENTIALS: &str = "UpdateMcpCredentials";
}

/// Principal kind (16D). `Agent` iff the enriched `role` claim contains
/// "agent" — the exact value mapped by Kanidm's `pdt-api-agents` group
/// (role vocabulary: admin | user | service | agent, facts doc 42977cb7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    Human,
    Agent,
}

impl PrincipalKind {
    /// Classify from the primary role. Anything that is not exactly
    /// "agent" is Human — fail-toward-human keeps the default posture
    /// identical to pre-16D behavior.
    pub fn from_role(role: Option<&str>) -> Self {
        match role {
            Some("agent") => Self::Agent,
            _ => Self::Human,
        }
    }
}

/// Extract the primary role from enriched JWT claims.
///
/// PEP merges userinfo into `extra` (flattened claims). Kanidm delivers
/// `role` as a STRING or an ARRAY (pep 366e8ed lesson) — accept both,
/// first value wins.
fn claim_role(claims: &JwtClaims) -> Option<String> {
    match claims.extra.get("role") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .next()
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Authenticated user info extracted from the token.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub username: Option<String>,
    pub is_dev: bool,
    /// Primary role from enriched claims (16D).
    pub role: Option<String>,
    /// Principal classification (16D): Agent iff role == "agent".
    pub kind: PrincipalKind,
}

impl From<JwtClaims> for AuthUser {
    fn from(claims: JwtClaims) -> Self {
        let role = claim_role(&claims);
        let kind = PrincipalKind::from_role(role.as_deref());
        Self {
            sub: claims.sub,
            email: claims.email,
            name: claims.name,
            username: claims.preferred_username,
            is_dev: false,
            role,
            kind,
        }
    }
}

/// Cookie max-age for session cookies (1 hour, matching the server-side idle timeout).
const SESSION_COOKIE_MAX_AGE: StdDuration = StdDuration::from_secs(3600);

/// PINNED (16D) + AGENT CARVE-OUT (16E): user_key for JWT principals.
///
/// Humans: `preferred_username` first, falling back to `sub`. NEVER email —
/// email is rebindable in Kanidm and would silently re-home a principal's
/// namespace.
///
/// Agents: `sub` — ALWAYS. Exchanged agent tokens carry no
/// `preferred_username` today, but a future scope change (e.g. adding
/// `profile` to the exchange) would add it and silently re-home every
/// agent's namespace — the exact migration bug class 0.10→0.11 inflicted on
/// humans. Agents are pinned to `sub` for the lifetime of the namespace.
///
/// MIGRATION NOTE (humans): the 16D rule changed the key for existing JWT
/// humans (they were keyed by `sub` before 0.11.0). Kanidm humans have a
/// `preferred_username`, so their namespace hash changes on first login —
/// pre-web-production history under the old hash is orphaned, not deleted.
fn jwt_user_key(claims: &JwtClaims) -> String {
    if PrincipalKind::from_role(claim_role(claims).as_deref()) == PrincipalKind::Agent {
        return claims.sub.clone();
    }
    match claims.preferred_username.as_deref() {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            tracing::debug!(
                "user_key: preferred_username missing/empty for sub {} — falling back to sub",
                claims.sub
            );
            claims.sub.clone()
        }
    }
}

/// Extract a user key from a dev-mode token string.
///
/// Two formats (BOTH gated by `local_dev_mode` at every call site):
/// - `dev:agent:<name>` → `agent-{name}` (16D: agent namespace in dev —
///   unblocks per-user cache + THQ E2E without Kanidm accounts; distinct
///   prefix so dev agents can never collide with dev humans)
/// - `dev:email:name:username` → `dev:{email}` (dev humans, legacy)
fn dev_user_key(token: &str) -> Option<String> {
    if let Some(name) = token.strip_prefix("dev:agent:") {
        let name = name.trim();
        if name.is_empty() || name.contains(':') {
            return None;
        }
        return Some(format!("agent-{name}"));
    }
    let parts: Vec<&str> = token.splitn(4, ':').collect();
    if parts.len() >= 4 {
        Some(format!("dev:{}", parts[1]))
    } else {
        None
    }
}

/// Pure decision core of [`check_dispatch_admin`] — unit-testable without an IdP.
pub(crate) fn dispatch_allowed(kind: PrincipalKind, role: Option<&str>) -> bool {
    kind == PrincipalKind::Human && role == Some("admin")
}

/// 16F: admin gate for the per-agent dispatch surface (`/xagent/{name}`).
///
/// The OUTER dispatch must be performed by a HUMAN ADMIN: agents may never
/// dispatch agents, and `service` is read-only by design. The impersonated
/// INNER request is separately authenticated and Cedar-gated AS THE AGENT
/// (per-action matrix), so this gate answers exactly one question: "may this
/// principal dispatch agent-users at all".
///
/// Open mode (no auth configured) allows dispatch, consistent with the
/// posture check_auth already applies. Dev-mode human tokens carry no role
/// and are therefore rejected — dispatch testing requires a real admin JWT.
pub async fn check_dispatch_admin(
    auth: &Option<Arc<AuthState>>,
    headers: &axum::http::HeaderMap,
) -> Result<(), StatusCode> {
    let Some(auth) = auth.as_ref() else {
        return Ok(()); // open mode — same posture as check_auth
    };
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if token.starts_with("dev:") {
        tracing::warn!("xagent dispatch rejected: dev tokens cannot dispatch agents");
        return Err(StatusCode::FORBIDDEN);
    }
    let claims = auth.validate_token_flexible(&token).await.map_err(|e| {
        tracing::warn!("xagent dispatch: caller token validation failed: {}", e);
        StatusCode::UNAUTHORIZED
    })?;
    let user = AuthUser::from(claims);
    if !dispatch_allowed(user.kind, user.role.as_deref()) {
        tracing::warn!(
            "xagent dispatch rejected: principal kind={:?} role={:?} — admin required",
            user.kind,
            user.role
        );
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

impl AuthState {
    /// 16F: exchange an agent-user's Kanidm service token for a short-lived
    /// access token carrying `role=agent`, usable as the impersonated Bearer
    /// on the inner dispatch. Same grant the MCP credentials use (verified
    /// RFC 8693 shape); enrichment in `validate_token` resolves the role.
    ///
    /// `issuer_url` MUST be the service-account issuer: Kanidm binds token
    /// exchange to the client origin — the `[oidc]` auth issuer host serves
    /// logins but rejects exchange with `invalid_request`, while the
    /// `[mcp.credentials.*]` service issuer (see
    /// `ServerState::service_issuer`) accepts it (verified live 2026-08-30:
    /// same token, 200 vs 400).
    pub async fn exchange_agent_token(
        &self,
        issuer_url: &str,
        service_token: &str,
    ) -> Result<(String, u64), StatusCode> {
        let tr = self
            .oidc_client
            .exchange_token(
                issuer_url,
                &self.config.client_id,
                None, // public client — no secret (Kanidm rejects one)
                service_token,
                &self.config.client_id,
                Some("openid groups"),
            )
            .await
            .map_err(|e| {
                tracing::error!(
                    "xagent dispatch: agent token exchange FAILED at {issuer_url}: {} — impersonation unavailable",
                    e
                );
                StatusCode::BAD_GATEWAY
            })?;
        Ok((tr.access_token, tr.expires_in.unwrap_or(900)))
    }
}

/// Check authentication for a protected endpoint.
///
/// Returns `Ok((None, user_key))` if auth is not configured (open mode), or if
/// a valid token is present without needing cookie renewal. Returns
/// `Ok((Some(cookie), user_key))` if auth succeeded and the caller should
/// include the given `Set-Cookie` header value in the response (rolling session).
/// Returns `Err(StatusCode)` if auth is configured but no valid token is found.
///
/// The returned `user_key` is the identity string used for session isolation
/// (JWT principals: `preferred_username || sub` — PINNED, see [`jwt_user_key`];
/// `dev:{email}` for dev-mode humans, `agent-{name}` for `dev:agent:` tokens,
/// `"default"` when auth is not configured). This avoids the need for handlers
/// to call `resolve_user_key()` which would re-validate the JWT a second time.
///
/// Token sources (in order):
/// 1. `Authorization: Bearer <token>` header (raw JWT — validated directly)
/// 2. `trustee_token=<session_id>` cookie (looked up in WebSessionManager,
///    auto-refreshed if near expiry)
///
/// Dev mode tokens use the format `dev:email:name:username`.
pub async fn check_auth(
    auth: &Option<Arc<AuthState>>,
    headers: &axum::http::HeaderMap,
    action: &str,
) -> Result<(Option<String>, String), StatusCode> {
    let Some(auth) = auth.as_ref() else {
        return Ok((None, "default".to_string())); // Auth not configured — allow
    };

    // 1. Try Bearer header first (raw JWT — e.g. from API clients, Torpi proxy)
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        // Dev mode token — only accepted when dev mode is currently enabled
        if token.starts_with("dev:") {
            if !auth.config.dev_config.local_dev_mode {
                tracing::warn!("Dev token presented but dev mode is disabled — rejecting");
                return Err(StatusCode::UNAUTHORIZED);
            }
            return match dev_user_key(&token) {
                Some(key) => Ok((None, key)),
                None => Err(StatusCode::UNAUTHORIZED),
            };
        }

        return match auth.validate_token_flexible(&token).await {
            Ok(claims) => {
                if auth.check_cedar_authorized(&claims, action).is_err() {
                    return Err(StatusCode::FORBIDDEN);
                }
                Ok((None, jwt_user_key(&claims)))
            }
            Err(e) => {
                tracing::warn!("Bearer token validation failed: {}", e);
                Err(StatusCode::UNAUTHORIZED)
            }
        };
    }

    // 2. Try cookie (session_id → WebSessionManager → access token with auto-refresh)
    let session_id = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| extract_token_from_cookies(cookies, &auth.config.cookie_name));

    let Some(session_id) = session_id else {
        tracing::warn!("No auth token found in request");
        return Err(StatusCode::UNAUTHORIZED);
    };

    // Dev mode token in cookie — only accepted when dev mode is currently enabled
    if session_id.starts_with("dev:") {
        if !auth.config.dev_config.local_dev_mode {
            tracing::warn!("Dev cookie presented but dev mode is disabled — rejecting");
            return Err(StatusCode::UNAUTHORIZED);
        }
        return match dev_user_key(&session_id) {
            Some(key) => Ok((None, key)),
            None => Err(StatusCode::UNAUTHORIZED),
        };
    }

    // Session-based: look up via WebSessionManager (auto-refreshes)
    match auth.session_manager.get_token(&session_id).await {
        Ok(access_token) => match auth.validate_token(&access_token).await {
            Ok(claims) => {
                // Cedar authorization check
                if auth.check_cedar_authorized(&claims, action).is_err() {
                    return Err(StatusCode::FORBIDDEN);
                }
                // Roll the cookie — reset max-age so active users stay logged in
                let secure = auth.client_config.redirect_uri.starts_with("https");
                let cookie = create_auth_cookie(
                    &auth.config.cookie_name,
                    &session_id,
                    SESSION_COOKIE_MAX_AGE,
                    secure,
                );
                Ok((Some(cookie.to_string()), jwt_user_key(&claims)))
            }
            Err(e) => {
                // Token was returned but JWT validation failed (e.g. ExpiredSignature
                // due to clock skew). Force-refresh and retry once.
                tracing::warn!(
                    "Session token validation failed: {} — attempting force-refresh",
                    e
                );
                match auth.session_manager.force_refresh(&session_id).await {
                    Ok(new_token) => match auth.validate_token(&new_token).await {
                        Ok(claims) => {
                            // Cedar authorization check
                            if auth.check_cedar_authorized(&claims, action).is_err() {
                                return Err(StatusCode::FORBIDDEN);
                            }
                            let secure = auth.client_config.redirect_uri.starts_with("https");
                            let cookie = create_auth_cookie(
                                &auth.config.cookie_name,
                                &session_id,
                                SESSION_COOKIE_MAX_AGE,
                                secure,
                            );
                            Ok((Some(cookie.to_string()), jwt_user_key(&claims)))
                        }
                        Err(e2) => {
                            tracing::warn!(
                                "Session token still invalid after force-refresh: {}",
                                e2
                            );
                            Err(StatusCode::UNAUTHORIZED)
                        }
                    },
                    Err(e2) => {
                        tracing::warn!("Force-refresh failed: {}", e2);
                        Err(StatusCode::UNAUTHORIZED)
                    }
                }
            }
        },
        Err(e) => {
            tracing::warn!("Session lookup/refresh failed: {}", e);
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// Extract a valid access token from the request (for use by handlers that
/// need the token itself, not just auth checking).
///
/// Resolves session_id cookies to actual access tokens via WebSessionManager.
/// Bearer headers are returned as-is.
async fn resolve_access_token(
    auth: &AuthState,
    headers: &axum::http::HeaderMap,
) -> Result<String, StatusCode> {
    // Bearer header — return as-is
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        return Ok(token);
    }

    // Cookie — resolve session_id → access_token
    let session_id = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| extract_token_from_cookies(cookies, &auth.config.cookie_name));

    match session_id {
        Some(sid) if sid.starts_with("dev:") => {
            if !auth.config.dev_config.local_dev_mode {
                tracing::warn!(
                    "Dev cookie in resolve_access_token but dev mode is disabled — rejecting"
                );
                Err(StatusCode::UNAUTHORIZED)
            } else {
                Ok(sid)
            }
        }
        Some(sid) => auth.session_manager.get_token(&sid).await.map_err(|e| {
            tracing::warn!("Failed to resolve session token: {}", e);
            StatusCode::UNAUTHORIZED
        }),
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Extract token value from a cookie header string.
fn extract_token_from_cookies(cookie_header: &str, cookie_name: &str) -> Option<String> {
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix(&format!("{}=", cookie_name)) {
            return Some(value.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Auth routes: /auth/login, /auth/callback, /auth/me, /auth/logout
// ---------------------------------------------------------------------------

/// Build the auth routes as a nested Router.
pub fn auth_routes() -> axum::Router<crate::ServerState> {
    axum::Router::new()
        .route("/login", axum::routing::get(login_handler))
        .route("/callback", axum::routing::get(callback_handler))
        .route("/me", axum::routing::get(me_handler))
        .route("/logout", axum::routing::post(logout_handler))
        .route("/mcp/login", axum::routing::get(mcp_login_handler))
        .route("/mcp/callback", axum::routing::get(mcp_callback_handler))
        .route("/mcp/status", axum::routing::get(mcp_status_handler))
        .route("/mcp/logout", axum::routing::post(mcp_logout_handler))
}

/// Query parameters for OIDC callback.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// GET /auth/login — initiate OIDC login with PKCE, or create dev session.
async fn login_handler(State(state): State<crate::ServerState>) -> Result<Response, AuthError> {
    let auth = state.auth.as_ref().ok_or(AuthError::AuthNotConfigured)?;

    // Dev mode — create synthetic session
    if auth.is_dev_mode() {
        tracing::info!("Dev mode: creating dev session");
        let dev = &auth.config.dev_config;
        let dev_token = format!(
            "dev:{}:{}:{}",
            dev.local_dev_email.as_deref().unwrap_or("dev@localhost"),
            dev.local_dev_name.as_deref().unwrap_or("Dev User"),
            dev.local_dev_username.as_deref().unwrap_or("dev")
        );
        let cookie = create_auth_cookie(
            &auth.config.cookie_name,
            &dev_token,
            StdDuration::from_secs(86400),
            false,
        );
        return Ok(Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, "/")
            .header(header::SET_COOKIE, cookie.to_string())
            .body(Body::empty())
            .unwrap());
    }

    // Production — redirect to IdP with PKCE
    let pkce_session = auth.pkce_manager.create();
    let challenge = OidcClient::generate_code_challenge(&pkce_session.verifier);

    let auth_url = auth
        .oidc_client
        .build_authorization_url(&auth.client_config, &pkce_session.state, Some(&challenge))
        .await
        .map_err(|e| AuthError::OidcError(e.to_string()))?;

    // Set PKCE state cookie (HttpOnly, SameSite=Lax)
    // Secure flag follows the redirect_uri scheme — HTTP localhost/LAN must not
    // set Secure or the browser drops the cookie and PKCE state is lost.
    let secure = auth.client_config.redirect_uri.starts_with("https");
    let pkce_cookie = Cookie::build((
        auth.pkce_manager.cookie_name().to_string(),
        pkce_session.cookie_value,
    ))
    .path("/")
    .http_only(true)
    .same_site(SameSite::Lax)
    .secure(secure)
    .max_age(TimeDuration::seconds(
        auth.pkce_manager.ttl().as_secs() as i64
    ))
    .build();

    Ok(Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, &auth_url)
        .header(header::SET_COOKIE, pkce_cookie.to_string())
        .body(Body::empty())
        .unwrap())
}

/// GET /auth/callback — exchange authorization code for tokens, set cookie.
async fn callback_handler(
    State(state): State<crate::ServerState>,
    Query(query): Query<CallbackQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AuthError> {
    let auth = state.auth.as_ref().ok_or(AuthError::AuthNotConfigured)?;

    // Check for errors from IdP
    if let Some(error) = query.error {
        let desc = query.error_description.unwrap_or_default();
        tracing::error!("OIDC error: {} - {}", error, desc);
        return Ok(Redirect::temporary(&format!(
            "/?error={}&error_description={}",
            urlencoding::encode(&error),
            urlencoding::encode(&desc)
        ))
        .into_response());
    }

    let code = query.code.ok_or(AuthError::MissingCode)?;
    let oauth_state = query.state.ok_or(AuthError::MissingState)?;

    // Retrieve PKCE cookie
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let pkce_value = extract_token_from_cookies(cookie_header, auth.pkce_manager.cookie_name())
        .ok_or(AuthError::InvalidState)?;

    // Verify PKCE cookie (HMAC + expiry + state match)
    let verifier = auth
        .pkce_manager
        .verify(&pkce_value, &oauth_state)
        .ok_or(AuthError::InvalidState)?;

    // Exchange code for tokens
    tracing::info!("Exchanging authorization code for tokens");
    let token_response = auth
        .oidc_client
        .exchange_code_for_tokens(&auth.client_config, &code, Some(&verifier))
        .await
        .map_err(|e| AuthError::TokenExchangeFailed(e.to_string()))?;

    let session_id = auth
        .session_manager
        .create_session(&token_response)
        .await
        .map_err(|e| AuthError::TokenExchangeFailed(format!("Session creation failed: {}", e)))?;

    // Cookie lifetime matches server-side idle timeout (1 hour).
    // The cookie is rolled on every successful request via check_auth().
    let max_age = SESSION_COOKIE_MAX_AGE;

    // Set auth cookie — Secure only when redirect_uri is HTTPS
    let secure = auth.client_config.redirect_uri.starts_with("https");
    let cookie = create_auth_cookie(&auth.config.cookie_name, &session_id, max_age, secure);

    // Clear PKCE cookie (single-use)
    let clear_pkce = Cookie::build((auth.pkce_manager.cookie_name().to_string(), ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(TimeDuration::seconds(-1))
        .build();

    tracing::info!("Authentication successful, redirecting to /");

    Ok(Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/")
        .header(header::SET_COOKIE, cookie.to_string())
        .header(header::SET_COOKIE, clear_pkce.to_string())
        .body(Body::empty())
        .unwrap())
}

/// GET /auth/me — return current user info.
async fn me_handler(
    State(state): State<crate::ServerState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(ref auth) = state.auth else {
        // Auth not configured — always authenticated (no auth required)
        return axum::Json(serde_json::json!({
            "authenticated": true,
            "auth_enabled": false
        }))
        .into_response();
    };

    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Also try Authorization: Bearer header
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from);

    let token = bearer
        .clone()
        .or_else(|| extract_token_from_cookies(cookie_header, &auth.config.cookie_name));

    let Some(cookie_value) = token else {
        return axum::Json(serde_json::json!({
            "authenticated": false,
            "auth_enabled": true
        }))
        .into_response();
    };

    // Dev mode token (stored directly in cookie, no session manager)
    // Only report as authenticated when dev mode is currently enabled
    if cookie_value.starts_with("dev:") && auth.config.dev_config.local_dev_mode {
        let parts: Vec<&str> = cookie_value.splitn(4, ':').collect();
        if parts.len() >= 4 {
            return axum::Json(serde_json::json!({
                "authenticated": true,
                "auth_enabled": true,
                "email": parts[1],
                "name": parts[2],
                "username": parts[3],
                "dev_mode": true
            }))
            .into_response();
        }
    }

    // Bearer header = raw JWT; Cookie value = session_id → resolve to access token
    let access_token = if bearer.is_some() {
        // Already have the raw token from Bearer header
        cookie_value
    } else {
        // Cookie value is a session_id — resolve via WebSessionManager
        match auth.session_manager.get_token(&cookie_value).await {
            Ok(token) => token,
            Err(e) => {
                tracing::debug!("Session token resolution failed for /auth/me: {}", e);
                return axum::Json(serde_json::json!({
                    "authenticated": false,
                    "auth_enabled": true
                }))
                .into_response();
            }
        }
    };

    // Real JWT — validate and return claims
    match auth.validate_token(&access_token).await {
        Ok(claims) => axum::Json(serde_json::json!({
            "authenticated": true,
            "auth_enabled": true,
            "sub": claims.sub,
            "email": claims.email,
            "name": claims.name,
            "username": claims.preferred_username,
            "dev_mode": false
        }))
        .into_response(),
        Err(e) => {
            tracing::debug!("Token validation failed for /auth/me: {}", e);
            axum::Json(serde_json::json!({
                "authenticated": false,
                "auth_enabled": true
            }))
            .into_response()
        }
    }
}

/// POST /auth/logout — destroy session and clear auth cookie.
async fn logout_handler(
    State(state): State<crate::ServerState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let cookie_name = state
        .auth
        .as_ref()
        .map(|a| a.config.cookie_name.as_str())
        .unwrap_or("trustee_token");

    // Destroy the session on the server side
    if let Some(ref auth) = state.auth {
        if let Some(cookie_header) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
            if let Some(session_id) = extract_token_from_cookies(cookie_header, cookie_name) {
                if !session_id.starts_with("dev:") {
                    let _ = auth.session_manager.destroy_session(&session_id);
                }
            }
        }
    }

    let cookie = Cookie::build((cookie_name.to_string(), ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(TimeDuration::seconds(-1))
        .build();

    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/")
        .header(header::SET_COOKIE, cookie.to_string())
        .body(Body::empty())
        .unwrap()
}

// ---------------------------------------------------------------------------
// MCP auth routes: /auth/mcp/login, /callback, /status, /logout (C2)
// ---------------------------------------------------------------------------

/// Query parameters for MCP login initiation.
#[derive(Debug, Deserialize)]
pub struct McpLoginQuery {
    pub cred: String,
}

/// Query parameters for MCP OIDC callback.
#[derive(Debug, Deserialize)]
pub struct McpCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// GET /auth/mcp/login?cred=<name> — initiate per-server OIDC PKCE login.
///
/// Reads the credential config from the session's config_toml, verifies it's
/// `type = "web-interactive"`, then redirects to the OIDC provider.
async fn mcp_login_handler(
    State(state): State<crate::ServerState>,
    Query(query): Query<McpLoginQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AuthError> {
    // Require authentication — user must be logged into trustee-web
    let (_cookie, _user_key) = crate::auth::check_auth(
        &state.auth,
        &headers,
        crate::auth::actions::UPDATE_MCP_CREDENTIALS,
    )
    .await
    .map_err(|_| AuthError::AuthNotConfigured)?;

    let auth = state.auth.as_ref().ok_or(AuthError::AuthNotConfigured)?;

    // Parse MCP credential config from session's config_toml
    let cred_config = load_mcp_credential(&state, &query.cred).await?;

    let (issuer_url, client_id, client_secret, scope) = match &cred_config {
        McpCredentialInfo::WebInteractive {
            issuer_url,
            client_id,
            client_secret,
            scope,
        } => (
            issuer_url.clone(),
            client_id.clone(),
            client_secret.clone(),
            scope.clone(),
        ),
        _ => {
            return Ok(Redirect::temporary(&format!(
                "/?mcp_error={}",
                urlencoding::encode(&format!(
                    "Credential '{}' is not web-interactive type",
                    query.cred
                ))
            ))
            .into_response());
        }
    };

    // Build PKCE pair using a separate PkceCookieManager for MCP
    let oidc_client = OidcClient::new();
    let verifier = OidcClient::generate_code_verifier();
    let challenge = OidcClient::generate_code_challenge(&verifier);
    let oauth_state = OidcClient::generate_state();

    // Build OidcClientConfig for the MCP credential's OIDC client
    let mcp_redirect_uri = format!(
        "{}/auth/mcp/callback",
        auth.client_config
            .redirect_uri
            .trim_end_matches('/')
            .trim_end_matches("/auth/callback")
    );

    let mcp_client_config = OidcClientConfig {
        issuer_url: issuer_url.clone(),
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        redirect_uri: mcp_redirect_uri.clone(),
        scope: scope.clone(),
        code_challenge_method: "S256".to_string(),
    };

    // Build authorization URL
    let auth_url = oidc_client
        .build_authorization_url(&mcp_client_config, &oauth_state, Some(&challenge))
        .await
        .map_err(|e| AuthError::OidcError(e.to_string()))?;

    // Store PKCE state + credential name in the in-memory map
    mcp_pkce()
        .insert(oauth_state.clone(), verifier.clone(), query.cred.clone())
        .await;

    tracing::info!(
        "Initiating MCP browser login for credential '{}' (issuer={})",
        query.cred,
        issuer_url
    );

    Ok(Response::builder()
        .status(StatusCode::TEMPORARY_REDIRECT)
        .header(header::LOCATION, &auth_url)
        .body(Body::empty())
        .unwrap())
}

/// GET /auth/mcp/callback — handle MCP OIDC callback, store tokens.
async fn mcp_callback_handler(
    State(state): State<crate::ServerState>,
    Query(query): Query<McpCallbackQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AuthError> {
    let auth = state.auth.as_ref().ok_or(AuthError::AuthNotConfigured)?;

    // Check for errors from IdP
    if let Some(error) = query.error {
        let desc = query.error_description.unwrap_or_default();
        tracing::error!("MCP OIDC error: {} - {}", error, desc);
        return Ok(Redirect::temporary(&format!(
            "/?mcp_error={}&error_description={}",
            urlencoding::encode(&error),
            urlencoding::encode(&desc)
        ))
        .into_response());
    }

    let code = query.code.ok_or(AuthError::MissingCode)?;
    let oauth_state = query.state.ok_or(AuthError::MissingState)?;

    // Look up PKCE verifier + credential name from in-memory store
    let pkce_data = mcp_pkce()
        .take(&oauth_state)
        .await
        .ok_or(AuthError::InvalidState)?;

    let verifier = pkce_data.verifier;
    let cred_name = &pkce_data.cred_name;

    // Parse the MCP credential config to get OIDC settings for token exchange
    let cred_config = load_mcp_credential(&state, cred_name).await?;

    let (issuer_url, client_id, client_secret, scope) = match &cred_config {
        McpCredentialInfo::WebInteractive {
            issuer_url,
            client_id,
            client_secret,
            scope,
        } => (
            issuer_url.clone(),
            client_id.clone(),
            client_secret.clone(),
            scope.clone(),
        ),
        _ => {
            return Ok(Redirect::temporary(&format!(
                "/?mcp_error={}",
                urlencoding::encode("Credential is not web-interactive type")
            ))
            .into_response());
        }
    };

    // Build redirect URI (must match what was used in login)
    let mcp_redirect_uri = format!(
        "{}/auth/mcp/callback",
        auth.client_config
            .redirect_uri
            .trim_end_matches('/')
            .trim_end_matches("/auth/callback")
    );

    let mcp_client_config = OidcClientConfig {
        issuer_url: issuer_url.clone(),
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        redirect_uri: mcp_redirect_uri,
        scope: scope.clone(),
        code_challenge_method: "S256".to_string(),
    };

    // Exchange code for tokens
    tracing::info!(
        "Exchanging MCP authorization code for tokens (credential={})",
        cred_name
    );
    let oidc_client = OidcClient::new();
    let token_response = oidc_client
        .exchange_code_for_tokens(&mcp_client_config, &code, Some(&verifier))
        .await
        .map_err(|e| AuthError::TokenExchangeFailed(e.to_string()))?;

    // Compute expires_at
    let expires_at = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_epoch = now + token_response.expires_in.unwrap_or(900);
        let days = expires_epoch / 86400;
        let rem = expires_epoch % 86400;
        let h = rem / 3600;
        let m = (rem % 3600) / 60;
        let s = rem % 60;
        let z = days as i64 + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mon = if mp < 10 { mp + 3 } else { mp - 9 };
        let yr = if mon <= 2 { y + 1 } else { y };
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yr, mon, d, h, m, s)
    };

    // Store via FileTokenStore (same as `trustee mcp auth`)
    use pep::{FileTokenStore, StoredToken, TokenStore};

    let stored = StoredToken::new(
        &token_response.access_token,
        token_response.refresh_token.clone(),
        "Bearer",
        &expires_at,
        token_response.scope.clone(),
    );

    let agent_name = state
        .config_toml
        .as_ref()
        .and_then(|t| {
            toml::from_str::<toml::Value>(t).ok().and_then(|v| {
                v.get("agent")
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .map(String::from)
            })
        })
        .unwrap_or_else(|| "trustee".to_string());
    let token_store = FileTokenStore::new(&agent_name);

    if let Err(e) = token_store.save(cred_name, &stored) {
        tracing::error!("Failed to store MCP token: {}", e);
        return Ok(Redirect::temporary(&format!(
            "/?mcp_error={}",
            urlencoding::encode(&format!("Failed to store token: {}", e))
        ))
        .into_response());
    }

    tracing::info!(
        "MCP authentication successful for credential '{}' (expires {})",
        cred_name,
        expires_at
    );

    Ok(Response::builder()
        .status(StatusCode::FOUND)
        .header(
            header::LOCATION,
            format!("/?mcp_connected={}", urlencoding::encode(cred_name)),
        )
        .body(Body::empty())
        .unwrap())
}

/// GET /auth/mcp/status — return connection status for all MCP credentials.
async fn mcp_status_handler(
    State(state): State<crate::ServerState>,
    headers: axum::http::HeaderMap,
) -> Response {
    use pep::{FileTokenStore, TokenStore};

    // Require auth
    let (_cookie, user_key) = match crate::auth::check_auth(
        &state.auth,
        &headers,
        crate::auth::actions::VIEW_MCP_CREDENTIALS,
    )
    .await
    {
        Ok(result) => result,
        Err(code) => {
            return (code, Json(serde_json::json!({"error": "Unauthorized"}))).into_response()
        }
    };

    // Parse MCP config from session
    let config_toml = {
        let (_sid, session_arc, _, _) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        match &session.config_toml {
            Some(t) => t.clone(),
            None => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Config not loaded").into_response()
            }
        }
    };

    let mcp_config: toml::Value = match toml::from_str(&config_toml) {
        Ok(v) => v,
        Err(_) => return Json(serde_json::json!([])).into_response(),
    };

    let agent_name = {
        let (_sid, session_arc, _, _) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        session.agent_name.clone()
    };
    let token_store = FileTokenStore::new(&agent_name);

    // Build server → credential mapping
    let servers = mcp_config
        .get("mcp")
        .and_then(|m| m.get("servers"))
        .and_then(|s| s.as_array());
    let credentials = mcp_config
        .get("mcp")
        .and_then(|m| m.get("credentials"))
        .and_then(|c| c.as_table());

    let mut cred_servers: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    if let Some(servers) = servers {
        for server in servers {
            let name = server.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let cred_ref = server
                .get("credentials")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if !cred_ref.is_empty() {
                cred_servers
                    .entry(cred_ref.to_string())
                    .or_default()
                    .push(name.to_string());
            }
        }
    }

    let mut result = Vec::new();

    if let Some(creds) = credentials {
        for (cred_name, cred_config) in creds {
            let cred_type = cred_config
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let servers_using = cred_servers.get(cred_name).cloned().unwrap_or_default();

            if cred_type == "web-session" {
                // Session credentials are always "connected" if auth is enabled
                let connected = state.auth.is_some();
                result.push(serde_json::json!({
                    "credential": cred_name,
                    "type": cred_type,
                    "connected": connected,
                    "servers": servers_using,
                }));
            } else if cred_type == "service-account" {
                // Long-lived service token, exchanged lazily (RFC 8693) by the
                // agent at runtime. "Connected" = the service_token resolved to
                // a non-empty value in this (already ${VAR}-substituted) config.
                let token = cred_config
                    .get("service_token")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                result.push(serde_json::json!({
                    "credential": cred_name,
                    "type": cred_type,
                    "connected": !token.is_empty(),
                    "servers": servers_using,
                }));
            } else if cred_type == "static" {
                // Static token — connected when it resolved non-empty.
                let token = cred_config
                    .get("token")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                result.push(serde_json::json!({
                    "credential": cred_name,
                    "type": cred_type,
                    "connected": !token.is_empty(),
                    "servers": servers_using,
                }));
            } else if cred_type == "web-interactive" || cred_type == "interactive" {
                // Check token store
                let status = match token_store.load(cred_name) {
                    Ok(Some(token)) => {
                        let expired = token.is_expired();
                        serde_json::json!({
                            "credential": cred_name,
                            "type": cred_type,
                            "connected": !expired,
                            "expires_at": token.expires_at,
                            "servers": servers_using,
                        })
                    }
                    _ => serde_json::json!({
                        "credential": cred_name,
                        "type": cred_type,
                        "connected": false,
                        "servers": servers_using,
                    }),
                };
                result.push(status);
            }
        }
    }

    Json(serde_json::Value::Array(result)).into_response()
}

/// POST /auth/mcp/logout?cred=<name> — remove stored MCP tokens.
async fn mcp_logout_handler(
    State(state): State<crate::ServerState>,
    Query(query): Query<McpLoginQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    use pep::{FileTokenStore, TokenStore};

    // Require auth
    let (_cookie, user_key) = match crate::auth::check_auth(
        &state.auth,
        &headers,
        crate::auth::actions::UPDATE_MCP_CREDENTIALS,
    )
    .await
    {
        Ok(result) => result,
        Err(code) => {
            return (code, Json(serde_json::json!({"error": "Unauthorized"}))).into_response()
        }
    };

    let agent_name = {
        let (_sid, session_arc, _, _) = state.ensure_active_session(&user_key).await;
        let session = session_arc.lock().await;
        session.agent_name.clone()
    };
    let token_store = FileTokenStore::new(&agent_name);

    match token_store.delete(&query.cred) {
        Ok(()) => {
            tracing::info!("Removed MCP credentials for '{}'", query.cred);
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to remove MCP credentials: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// MCP auth helpers
// ---------------------------------------------------------------------------

/// In-memory store for MCP PKCE state (state token → verifier + credential name).
/// Entries expire after 10 minutes. Not persisted across restarts.
struct McpPkceStore {
    entries: tokio::sync::Mutex<std::collections::HashMap<String, McpPkceEntry>>,
}

struct McpPkceEntry {
    verifier: String,
    cred_name: String,
    created_at: std::time::Instant,
}

impl McpPkceStore {
    fn new() -> Self {
        Self {
            entries: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Insert a PKCE entry. Cleans up entries older than 10 minutes.
    async fn insert(&self, state: String, verifier: String, cred_name: String) {
        let mut map = self.entries.lock().await;
        // Cleanup expired entries (older than 10 min)
        let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(600);
        map.retain(|_, v| v.created_at > cutoff);
        map.insert(
            state,
            McpPkceEntry {
                verifier,
                cred_name,
                created_at: std::time::Instant::now(),
            },
        );
    }

    /// Take and remove a PKCE entry (single-use).
    async fn take(&self, state: &str) -> Option<McpPkceEntry> {
        let mut map = self.entries.lock().await;
        map.remove(state)
    }
}

/// Global singleton PKCE store for MCP browser logins.
static MCP_PKCE: std::sync::OnceLock<McpPkceStore> = std::sync::OnceLock::new();

/// Get or initialize the global MCP PKCE store.
fn mcp_pkce() -> &'static McpPkceStore {
    MCP_PKCE.get_or_init(McpPkceStore::new)
}

/// Simplified MCP credential info (parsed from TOML).
enum McpCredentialInfo {
    WebInteractive {
        issuer_url: String,
        client_id: String,
        client_secret: Option<String>,
        scope: String,
    },
    Other(String),
}

/// Load a specific MCP credential from the session's config_toml.
async fn load_mcp_credential(
    state: &crate::ServerState,
    cred_name: &str,
) -> Result<McpCredentialInfo, AuthError> {
    let config_toml = state
        .config_toml
        .clone()
        .ok_or(AuthError::AuthNotConfigured)?;

    let config: toml::Value = toml::from_str(&config_toml)
        .map_err(|e| AuthError::OidcError(format!("Config parse error: {}", e)))?;

    let cred = config
        .get("mcp")
        .and_then(|m| m.get("credentials"))
        .and_then(|c| c.as_table())
        .and_then(|c| c.get(cred_name))
        .ok_or_else(|| AuthError::OidcError(format!("Credential '{}' not found", cred_name)))?;

    let cred_type = cred
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown");

    match cred_type {
        "web-interactive" => {
            let issuer_url = cred
                .get("issuer_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AuthError::OidcError("Missing issuer_url".into()))?
                .to_string();
            let client_id = cred
                .get("client_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AuthError::OidcError("Missing client_id".into()))?
                .to_string();
            let client_secret = cred
                .get("client_secret")
                .and_then(|v| v.as_str())
                .map(String::from);
            let scope = cred
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("openid profile email")
                .to_string();

            Ok(McpCredentialInfo::WebInteractive {
                issuer_url,
                client_id,
                client_secret,
                scope,
            })
        }
        other => Ok(McpCredentialInfo::Other(other.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an HttpOnly auth cookie.
fn create_auth_cookie(
    name: &str,
    value: &str,
    max_age: StdDuration,
    secure: bool,
) -> Cookie<'static> {
    Cookie::build((name.to_string(), value.to_string()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(TimeDuration::seconds(max_age.as_secs() as i64))
        .build()
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

/// Authentication errors.
#[derive(Debug)]
pub enum AuthError {
    MissingCode,
    MissingState,
    InvalidState,
    OidcError(String),
    TokenExchangeFailed(String),
    AuthNotConfigured,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (_status, msg) = match self {
            AuthError::MissingCode => (StatusCode::BAD_REQUEST, "Missing authorization code"),
            AuthError::MissingState => (StatusCode::BAD_REQUEST, "Missing state parameter"),
            AuthError::InvalidState => (StatusCode::BAD_REQUEST, "Invalid or expired state"),
            AuthError::OidcError(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Authentication service error",
            ),
            AuthError::TokenExchangeFailed(_) => (StatusCode::BAD_REQUEST, "Token exchange failed"),
            AuthError::AuthNotConfigured => {
                (StatusCode::NOT_IMPLEMENTED, "Authentication not configured")
            }
        };
        Redirect::temporary(&format!("/?error={}", urlencoding::encode(msg))).into_response()
    }
}

#[cfg(test)]
mod principal_tests {
    use super::*;
    use pep::oidc::types::JwtClaims;
    use std::collections::HashMap;

    fn claims_with_role(role: serde_json::Value) -> JwtClaims {
        let mut c = JwtClaims::default();
        c.sub = "sub-uuid".to_string();
        c.preferred_username = Some("farzan".to_string());
        c.extra.insert("role".to_string(), role);
        c
    }

    // -- dev_user_key (16D dev:agent namespace) ------------------------------

    #[test]
    fn dev_agent_token_yields_agent_namespaced_key() {
        assert_eq!(
            dev_user_key("dev:agent:farzan"),
            Some("agent-farzan".to_string())
        );
        assert_eq!(
            dev_user_key("dev:agent:paydar"),
            Some("agent-paydar".to_string())
        );
    }

    #[test]
    fn dev_agent_token_rejects_empty_and_colon_names() {
        assert_eq!(dev_user_key("dev:agent:"), None);
        assert_eq!(dev_user_key("dev:agent:  "), None);
        assert_eq!(
            dev_user_key("dev:agent:a:b"),
            None,
            "name must not contain ':'"
        );
    }

    #[test]
    fn dev_human_token_format_unchanged() {
        assert_eq!(
            dev_user_key("dev:a@b.c:Some Name:someuser"),
            Some("dev:a@b.c".to_string())
        );
        assert_eq!(dev_user_key("dev:only:two"), None);
    }

    // -- claim_role / PrincipalKind (string OR array role) --------------------

    #[test]
    fn role_as_string_classifies_agent() {
        let c = claims_with_role(serde_json::json!("agent"));
        assert_eq!(claim_role(&c).as_deref(), Some("agent"));
        assert_eq!(AuthUser::from(c).kind, PrincipalKind::Agent);
    }

    #[test]
    fn role_as_array_takes_first_value() {
        // Kanidm may deliver role as an array (pep 366e8ed lesson).
        let c = claims_with_role(serde_json::json!(["agent", "other"]));
        assert_eq!(claim_role(&c).as_deref(), Some("agent"));
        assert_eq!(AuthUser::from(c).kind, PrincipalKind::Agent);
    }

    #[test]
    fn non_agent_roles_classify_human() {
        for role in ["user", "admin", "service"] {
            let c = claims_with_role(serde_json::json!(role));
            assert_eq!(claim_role(&c).as_deref(), Some(role));
            assert_eq!(AuthUser::from(c).kind, PrincipalKind::Human, "role={role}");
        }
    }

    #[test]
    fn missing_or_nonstring_role_classifies_human() {
        let mut c = JwtClaims::default();
        c.sub = "sub-uuid".to_string();
        assert_eq!(claim_role(&c), None);
        assert_eq!(AuthUser::from(c.clone()).kind, PrincipalKind::Human);
        c.extra.insert("role".to_string(), serde_json::json!(42));
        assert_eq!(claim_role(&c), None, "non-string non-array role ignored");
    }

    // -- jwt_user_key (PINNED: preferred_username || sub, never email) --------

    #[test]
    fn user_key_prefers_preferred_username() {
        let mut c = JwtClaims::default();
        c.sub = "sub-uuid".to_string();
        c.preferred_username = Some("farzan".to_string());
        c.email = Some("rebindable@example.com".to_string());
        assert_eq!(jwt_user_key(&c), "farzan", "email must never be the key");
    }

    #[test]
    fn user_key_falls_back_to_sub_on_blank_username() {
        let mut c = JwtClaims::default();
        c.sub = "sub-uuid".to_string();
        c.preferred_username = Some("   ".to_string());
        assert_eq!(jwt_user_key(&c), "sub-uuid");
        c.preferred_username = None;
        assert_eq!(jwt_user_key(&c), "sub-uuid");
    }

    #[test]
    fn user_key_agent_pinned_to_sub_even_with_username() {
        // 16E: agent principals NEVER use preferred_username — a future
        // token-scope change must not re-home agent namespaces.
        let mut c = claims_with_role(serde_json::json!("agent"));
        c.sub = "agent-sub-uuid".to_string();
        c.preferred_username = Some("farzan".to_string());
        assert_eq!(jwt_user_key(&c), "agent-sub-uuid");
    }

    #[test]
    fn authuser_carries_role_and_kind() {
        let c = claims_with_role(serde_json::json!("agent"));
        let u = AuthUser::from(c);
        assert_eq!(u.role.as_deref(), Some("agent"));
        assert_eq!(u.kind, PrincipalKind::Agent);
        assert_eq!(u.username.as_deref(), Some("farzan"));
    }
}

#[cfg(test)]
mod cedar_p2_tests {
    use super::*;
    use cedar_policy::{Context, Entities, EntityUid, Request};
    use pep::cedar::{CedarAuthorizer, CedarConfig};
    use std::collections::HashMap;

    const POLICY: &str = include_str!("../policies/trustee_default.cedar");
    const SCHEMA: &str = include_str!("../policies/trustee_schema.cedarschema");

    async fn authorizer() -> CedarAuthorizer {
        // Unique per call: tests run concurrently and must not share files.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("trustee-cedar-p2-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let policy_path = dir.join("trustee_default.cedar");
        let schema_path = dir.join("trustee_schema.cedarschema");
        std::fs::write(&policy_path, POLICY).expect("write policy");
        std::fs::write(&schema_path, SCHEMA).expect("write schema");
        let cfg = CedarConfig {
            policy_path,
            schema_path: Some(schema_path),
            entities_path: None,
            default_decision: pep::cedar::DefaultDecision::Deny,
            validate_on_load: true,
            policy_store_url: None,
            policy_store_token: None,
            embedded_policy: Some(POLICY),
            embedded_schema: Some(SCHEMA),
        };
        CedarAuthorizer::new_with_policy_store(cfg)
            .await
            .expect("Cedar init from shipped sources")
    }

    fn claims_with_role(role: Option<&str>) -> JwtClaims {
        let mut c = JwtClaims::default();
        c.sub = "test-sub".to_string();
        if let Some(r) = role {
            c.extra.insert("role".to_string(), serde_json::json!(r));
        }
        c
    }

    /// Mirrors check_cedar_authorized's request construction exactly.
    async fn allowed(auth: &CedarAuthorizer, role: Option<&str>, action: &str) -> bool {
        let claims = claims_with_role(role);
        let principal_entity = pep::cedar::build_principal_entity(&claims).unwrap();
        let app_entity = cedar_policy::Entity::new(
            EntityUid::from_str(r#"TrusteeApp::"default""#).unwrap(),
            HashMap::new(),
            std::collections::HashSet::new(),
        )
        .unwrap();
        let entities = Entities::from_entities(vec![principal_entity, app_entity], None).unwrap();
        let request = Request::new(
            pep::cedar::build_principal_uid(&claims).unwrap(),
            EntityUid::from_str(&format!("Action::\"{action}\"")).unwrap(),
            EntityUid::from_str(r#"TrusteeApp::"default""#).unwrap(),
            Context::empty(),
            None,
        )
        .unwrap();
        auth.is_allowed_with_entities(&request, &entities).allowed()
    }

    #[tokio::test]
    async fn admin_allowed_including_destructive() {
        let auth = authorizer().await;
        for action in [
            actions::VIEW_SESSION,
            actions::COMMAND_SESSION,
            actions::DELETE_SESSION,
            actions::UPDATE_MCP_CREDENTIALS,
        ] {
            assert!(
                allowed(&auth, Some("admin"), action).await,
                "admin {action}"
            );
        }
    }

    #[tokio::test]
    async fn user_full_session_management() {
        let auth = authorizer().await;
        for action in [
            actions::CREATE_SESSION,
            actions::COMMAND_SESSION,
            actions::DELETE_SESSION,
            actions::VIEW_HISTORY,
            actions::UPDATE_MCP_CREDENTIALS,
        ] {
            assert!(allowed(&auth, Some("user"), action).await, "user {action}");
        }
    }

    #[tokio::test]
    async fn agent_working_set_but_delete_denied() {
        let auth = authorizer().await;
        for action in [
            actions::CREATE_SESSION,
            actions::COMMAND_SESSION,
            actions::CANCEL_SESSION,
            actions::RESUME_SESSION,
            actions::VIEW_HISTORY,
            actions::UPDATE_MCP_CREDENTIALS,
        ] {
            assert!(
                allowed(&auth, Some("agent"), action).await,
                "agent {action}"
            );
        }
        assert!(
            !allowed(&auth, Some("agent"), actions::DELETE_SESSION).await,
            "agent must NOT delete sessions (fail-closed start; revisit at task F)"
        );
    }

    #[tokio::test]
    async fn service_read_only() {
        let auth = authorizer().await;
        for action in [
            actions::VIEW_SESSION,
            actions::LIST_SESSIONS,
            actions::VIEW_HISTORY,
        ] {
            assert!(
                allowed(&auth, Some("service"), action).await,
                "service {action}"
            );
        }
        for action in [actions::COMMAND_SESSION, actions::DELETE_SESSION] {
            assert!(
                !allowed(&auth, Some("service"), action).await,
                "service {action} denied"
            );
        }
    }

    #[tokio::test]
    async fn missing_or_unknown_role_denied_everything() {
        let auth = authorizer().await;
        for role in [None, Some("intern"), Some("Admin")] {
            assert!(
                !allowed(&auth, role, actions::VIEW_SESSION).await,
                "role={role:?} must be denied (fail-closed default)"
            );
        }
    }

    #[test]
    fn boot_decision_is_fail_closed() {
        assert!(crate::cedar_boot_decision(true, false, false).is_err());
        assert!(crate::cedar_boot_decision(true, false, true).is_ok());
        assert!(crate::cedar_boot_decision(true, true, false).is_ok());
        assert!(crate::cedar_boot_decision(false, false, false).is_ok());
    }
}

/// Token validation cache (issue c41eb9d7) — pure-logic tests against a
/// standalone [`ValidationCache`]; no IdP, no network.
#[cfg(test)]
mod validation_cache_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn claims_with(exp_in: i64, sub: &str) -> JwtClaims {
        let mut c = JwtClaims::default();
        c.exp = unix_now() + exp_in;
        c.sub = sub.to_string();
        c.preferred_username = Some("farzan".to_string());
        c
    }

    /// ACCEPTANCE c41eb9d7: repeated polls = 1 validation, not 2.
    #[test]
    fn cache_hit_two_polls_one_validation() {
        let cache = ValidationCache::new();
        let validations = Arc::new(AtomicUsize::new(0));
        let key = validation_cache_key("token-A");

        for _ in 0..2 {
            if cache.get(&key, unix_now()).is_some() {
                continue; // cache hit — poll served without validation
            }
            // stub: this block is the "real validation"
            validations.fetch_add(1, Ordering::SeqCst);
            cache.put(key, claims_with(3600, "sub-A"), unix_now());
        }

        assert_eq!(
            validations.load(Ordering::SeqCst),
            1,
            "second poll must hit the cache, not re-validate"
        );
    }

    /// Expiry: after valid_until the entry is a miss → re-validation.
    #[test]
    fn expired_entry_revalidates() {
        let cache = ValidationCache::new();
        let key = validation_cache_key("token-B");
        let now = unix_now();
        cache.put(key, claims_with(3600, "sub-B"), now);

        assert!(cache.get(&key, now).is_some(), "fresh entry must hit");
        assert!(
            cache.get(&key, now + CACHE_TTL_SECS + 1).is_none(),
            "entry past valid_until must miss and force re-validation"
        );
    }

    /// exp-grace: a token inside its last 120s is born stale — get() must
    /// never serve it, mirroring PEP's 60s skew leeway with margin.
    #[test]
    fn near_expiry_token_never_served_from_cache() {
        let cache = ValidationCache::new();
        let key = validation_cache_key("token-C");
        let now = unix_now();
        // exp in 60s → valid_until = exp - 120 < now
        cache.put(key, claims_with(60, "sub-C"), now);
        assert!(
            cache.get(&key, now).is_none(),
            "entry valid_until <= now must be rejected immediately"
        );
    }

    /// Cap: at CACHE_MAX_ENTRIES the cache evicts instead of growing —
    /// correctness never depends on a hit (dropped entries re-validate).
    #[test]
    fn cap_evicts_instead_of_growing_unbounded() {
        let cache = ValidationCache::new();
        let now = unix_now();
        for i in 0..CACHE_MAX_ENTRIES {
            cache.put(
                validation_cache_key(&format!("tok-{i}")),
                claims_with(3600, "s"),
                now,
            );
        }
        // Map is all-hot → overflow clears rather than random-evicts.
        let overflow = validation_cache_key("tok-overflow");
        cache.put(overflow, claims_with(3600, "s"), now);

        let inner = cache.inner.lock().unwrap();
        assert!(
            inner.entries.len() <= CACHE_MAX_ENTRIES,
            "cache must stay bounded"
        );
        assert!(
            inner.entries.contains_key(&overflow),
            "newest entry must survive"
        );
        assert!(
            !inner.entries.contains_key(&validation_cache_key("tok-0")),
            "pre-overflow entries were reset, not served stale forever"
        );
    }

    /// INFO-vs-DEBUG gate: first sight per (sub, issuer), never again.
    #[test]
    fn first_sight_fires_once_per_sub_issuer_pair() {
        let cache = ValidationCache::new();
        assert!(cache.mark_first_sight("sub-A", "https://idp.tanbal.ir"));
        assert!(!cache.mark_first_sight("sub-A", "https://idp.tanbal.ir"));
        assert!(
            cache.mark_first_sight("sub-A", "https://other.tanbal.ir"),
            "different issuer = first sight"
        );
        assert!(
            cache.mark_first_sight("sub-B", "https://idp.tanbal.ir"),
            "different sub = first sight"
        );
    }

    /// 128-bit truncated SHA-256: distinct tokens must not collide.
    #[test]
    fn distinct_tokens_distinct_keys() {
        assert_ne!(validation_cache_key("tok-1"), validation_cache_key("tok-2"));
        // Raw token material must not leak into the stored key
        assert_eq!(
            validation_cache_key("secret"),
            validation_cache_key("secret")
        );
    }
}

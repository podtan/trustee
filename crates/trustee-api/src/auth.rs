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

use std::sync::Arc;
use std::time::Duration as StdDuration;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use pep::oidc_client::OidcClient;
use pep::oidc_resource_server::ResourceServerClient;
use pep::oidc::pkce_cookie::PkceCookieManager;
use pep::session_manager::WebSessionManager;
use pep::{DevConfig, JwtClaims, JwtValidationOptions, OidcClientConfig};
use serde::Deserialize;
use time::Duration as TimeDuration;

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
        let dev_config = table.get("dev").and_then(|d| d.as_table()).map(|d| {
            DevConfig {
                local_dev_mode: d.get("local_dev_mode").and_then(|v| v.as_bool()).unwrap_or(false),
                local_dev_email: d.get("local_dev_email").and_then(|v| v.as_str()).map(String::from),
                local_dev_name: d.get("local_dev_name").and_then(|v| v.as_str()).map(String::from),
                local_dev_username: d.get("local_dev_username").and_then(|v| v.as_str()).map(String::from),
            }
        });

        // Dev mode without OIDC — return early with dev-only config
        if let Some(ref dc) = dev_config {
            if dc.local_dev_mode {
                // Try to get OIDC config too (for login endpoint), but it's optional in dev mode
                let oidc = Self::parse_oidc_section(&table);
                return Some(Self {
                    issuer_url: oidc.as_ref().map(|o| o.0.clone()).unwrap_or_else(|| "https://auth.example.com".into()),
                    client_id: oidc.as_ref().map(|o| o.1.clone()).unwrap_or_else(|| "trustee".into()),
                    client_secret: oidc.as_ref().and_then(|o| o.2.clone()),
                    redirect_uri: oidc.as_ref().map(|o| o.3.clone()).unwrap_or_else(|| "http://localhost:3000/auth/callback".into()),
                    scope: oidc.as_ref().map(|o| o.4.clone()).unwrap_or_else(|| "openid profile email".into()),
                    cookie_name: "trustee_token".into(),
                    dev_config: dc.clone(),
                    validation_options: JwtValidationOptions::default(),
                    pkce_cookie_secret: oidc.as_ref().map(|o| o.6.clone()).unwrap_or_else(|| "trustee-default-pkce-secret-change-me".into()),
                });
            }
        }

        // Production mode — requires [oidc] section
        let (issuer_url, client_id, client_secret, redirect_uri, scope, validation_options, pkce_secret) =
            Self::parse_oidc_section(&table)?;

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
    ) -> Option<(String, String, Option<String>, String, String, JwtValidationOptions, String)> {
        let oidc = table.get("oidc")?.as_table()?;

        let issuer_url = oidc.get("issuer_url")?.as_str()?.to_string();
        let client_id = oidc.get("client_id")?.as_str()?.to_string();
        let client_secret = oidc.get("client_secret").and_then(|v| v.as_str()).map(String::from);
        let redirect_uri = oidc
            .get("redirect_url")
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
        if let Some(skip) = oidc.get("skip_audience_validation").and_then(|v| v.as_bool()) {
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

        Some((issuer_url, client_id, client_secret, redirect_uri, scope, validation_options, pkce_secret))
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
}

impl AuthState {
    /// Create new auth state from configuration.
    pub fn new(config: AuthConfig) -> Self {
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
        }
    }

    /// Check if development mode is enabled.
    pub fn is_dev_mode(&self) -> bool {
        self.config.dev_config.local_dev_mode
    }

    /// Validate a JWT token using PEP's ResourceServerClient.
    pub async fn validate_token(&self, token: &str) -> anyhow::Result<JwtClaims> {
        let mut claims = self
            .resource_server
            .validate_jwt_with_options(
                token,
                &self.config.issuer_url,
                &self.config.client_id,
                &self.config.validation_options,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Token validation failed: {}", e))?;

        // Enrich with userinfo for role/groups (cached, no-op if already present)
        let _ = self
            .resource_server
            .enrich_claims_with_userinfo(&mut claims, token, &self.config.issuer_url, None)
            .await;

        // PEP only merges groups/role from userinfo. If name/email are missing
        // (Kanidm JWTs only contain sub), fetch them from userinfo directly.
        if claims.name.is_none() || claims.email.is_none() {
            self.fill_userinfo_fields(&mut claims, token).await;
        }

        Ok(claims)
    }

    /// Fetch name/email/preferred_username from the OIDC userinfo endpoint
    /// and fill in any that are missing from the JWT claims.
    async fn fill_userinfo_fields(&self, claims: &mut JwtClaims, token: &str) {
        // Derive userinfo URL from issuer
        // For Kanidm: issuer_url is the discovery endpoint,
        // userinfo is at {issuer_url}/userinfo
        let userinfo_url = format!("{}/userinfo", self.config.issuer_url.trim_end_matches('/'));

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
            tracing::debug!("Userinfo returned {} for name/email enrichment", resp.status());
            return;
        }

        let Ok(userinfo): Result<serde_json::Map<String, serde_json::Value>, _> = resp.json().await else {
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
}

// ---------------------------------------------------------------------------
// Auth checking — called by protected route handlers
// ---------------------------------------------------------------------------

/// Authenticated user info extracted from the token.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub username: Option<String>,
    pub is_dev: bool,
}

impl From<JwtClaims> for AuthUser {
    fn from(claims: JwtClaims) -> Self {
        Self {
            sub: claims.sub,
            email: claims.email,
            name: claims.name,
            username: claims.preferred_username,
            is_dev: false,
        }
    }
}

/// Cookie max-age for session cookies (1 hour, matching the server-side idle timeout).
const SESSION_COOKIE_MAX_AGE: StdDuration = StdDuration::from_secs(3600);

/// Check authentication for a protected endpoint.
///
/// Returns `Ok(None)` if auth is not configured (open mode), or if a valid
/// token is present without needing cookie renewal. Returns `Ok(Some(cookie))`
/// if auth succeeded and the caller should include the given `Set-Cookie`
/// header value in the response (rolling session). Returns `Err(StatusCode)`
/// if auth is configured but no valid token is found.
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
) -> Result<Option<String>, StatusCode> {
    let Some(auth) = auth.as_ref() else {
        return Ok(None); // Auth not configured — allow
    };

    // 1. Try Bearer header first (raw JWT — e.g. from API clients, Torpi proxy)
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        // Dev mode token
        if token.starts_with("dev:") {
            let parts: Vec<&str> = token.splitn(4, ':').collect();
            return if parts.len() >= 4 {
                Ok(None)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            };
        }

        return match auth.validate_token(&token).await {
            Ok(_) => Ok(None),
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

    // Dev mode token in cookie
    if session_id.starts_with("dev:") {
        let parts: Vec<&str> = session_id.splitn(4, ':').collect();
        return if parts.len() >= 4 {
            Ok(None)
        } else {
            Err(StatusCode::UNAUTHORIZED)
        };
    }

    // Session-based: look up via WebSessionManager (auto-refreshes)
    match auth.session_manager.get_token(&session_id).await {
        Ok(access_token) => match auth.validate_token(&access_token).await {
            Ok(_) => {
                // Roll the cookie — reset max-age so active users stay logged in
                let secure = auth.client_config.redirect_uri.starts_with("https");
                let cookie = create_auth_cookie(
                    &auth.config.cookie_name,
                    &session_id,
                    SESSION_COOKIE_MAX_AGE,
                    secure,
                );
                Ok(Some(cookie.to_string()))
            }
            Err(e) => {
                // Token was returned but JWT validation failed (e.g. ExpiredSignature
                // due to clock skew). Force-refresh and retry once.
                tracing::warn!("Session token validation failed: {} — attempting force-refresh", e);
                match auth.session_manager.force_refresh(&session_id).await {
                    Ok(new_token) => match auth.validate_token(&new_token).await {
                        Ok(_) => {
                            let secure = auth.client_config.redirect_uri.starts_with("https");
                            let cookie = create_auth_cookie(
                                &auth.config.cookie_name,
                                &session_id,
                                SESSION_COOKIE_MAX_AGE,
                                secure,
                            );
                            Ok(Some(cookie.to_string()))
                        }
                        Err(e2) => {
                            tracing::warn!("Session token still invalid after force-refresh: {}", e2);
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
        Some(sid) if sid.starts_with("dev:") => Ok(sid),
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
async fn login_handler(
    State(state): State<crate::ServerState>,
) -> Result<Response, AuthError> {
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
        let cookie = create_auth_cookie(&auth.config.cookie_name, &dev_token, StdDuration::from_secs(86400), false);
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
        .max_age(TimeDuration::seconds(auth.pkce_manager.ttl().as_secs() as i64))
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

    let token = bearer.clone().or_else(|| extract_token_from_cookies(cookie_header, &auth.config.cookie_name));

    let Some(cookie_value) = token else {
        return axum::Json(serde_json::json!({
            "authenticated": false,
            "auth_enabled": true
        }))
        .into_response();
    };

    // Dev mode token (stored directly in cookie, no session manager)
    if cookie_value.starts_with("dev:") {
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
// Helpers
// ---------------------------------------------------------------------------

/// Create an HttpOnly auth cookie.
fn create_auth_cookie(name: &str, value: &str, max_age: StdDuration, secure: bool) -> Cookie<'static> {
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
            AuthError::OidcError(_) => (StatusCode::SERVICE_UNAVAILABLE, "Authentication service error"),
            AuthError::TokenExchangeFailed(_) => (StatusCode::BAD_REQUEST, "Token exchange failed"),
            AuthError::AuthNotConfigured => (StatusCode::NOT_IMPLEMENTED, "Authentication not configured"),
        };
        Redirect::temporary(&format!("/?error={}", urlencoding::encode(msg))).into_response()
    }
}

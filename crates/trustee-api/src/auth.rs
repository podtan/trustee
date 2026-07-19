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
}

impl AuthState {
    /// Create new auth state from configuration.
    pub fn new(config: AuthConfig) -> Self {
        let pkce_manager = PkceCookieManager::new(
            config.pkce_cookie_secret.as_bytes(),
            "trustee_pkce_state",
            StdDuration::from_secs(600),
        );

        Self {
            oidc_client: OidcClient::new(),
            resource_server: ResourceServerClient::new(),
            client_config: config.oidc_client_config(),
            pkce_manager,
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

        Ok(claims)
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

/// Check authentication for a protected endpoint.
///
/// Returns `Ok(())` if auth is not configured (open mode), or if a valid
/// token is present. Returns `Err(StatusCode::UNAUTHORIZED)` if auth is
/// configured but no valid token is found.
///
/// Token sources (in order):
/// 1. `Authorization: Bearer <token>` header
/// 2. `trustee_token=<token>` cookie
///
/// Dev mode tokens use the format `dev:email:name:username`.
pub async fn check_auth(
    auth: &Option<Arc<AuthState>>,
    headers: &axum::http::HeaderMap,
) -> Result<(), StatusCode> {
    let Some(auth) = auth.as_ref() else {
        return Ok(()); // Auth not configured — allow
    };

    // Extract token
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| extract_token_from_cookies(cookies, &auth.config.cookie_name))
        });

    let Some(token) = token else {
        tracing::warn!("No auth token found in request");
        return Err(StatusCode::UNAUTHORIZED);
    };

    // Dev mode token
    if token.starts_with("dev:") {
        let parts: Vec<&str> = token.splitn(4, ':').collect();
        if parts.len() >= 4 {
            return Ok(());
        }
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Real JWT — validate via PEP ResourceServerClient
    match auth.validate_token(&token).await {
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!("Token validation failed: {}", e);
            Err(StatusCode::UNAUTHORIZED)
        }
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
    let pkce_cookie = Cookie::build((
        auth.pkce_manager.cookie_name().to_string(),
        pkce_session.cookie_value,
    ))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(!auth.is_dev_mode())
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

    let auth_token = token_response.access_token;
    let max_age = token_response
        .expires_in
        .map(StdDuration::from_secs)
        .unwrap_or(StdDuration::from_secs(3600));

    // Set auth cookie
    let cookie = create_auth_cookie(&auth.config.cookie_name, &auth_token, max_age, !auth.is_dev_mode());

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

    let token = bearer.or_else(|| extract_token_from_cookies(cookie_header, &auth.config.cookie_name));

    let Some(token) = token else {
        return axum::Json(serde_json::json!({
            "authenticated": false,
            "auth_enabled": true
        }))
        .into_response();
    };

    // Dev mode token
    if token.starts_with("dev:") {
        let parts: Vec<&str> = token.splitn(4, ':').collect();
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

    // Real JWT — validate and return claims
    match auth.validate_token(&token).await {
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

/// POST /auth/logout — clear auth cookie.
async fn logout_handler(
    State(state): State<crate::ServerState>,
) -> Response {
    let cookie_name = state
        .auth
        .as_ref()
        .map(|a| a.config.cookie_name.as_str())
        .unwrap_or("trustee_token");

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

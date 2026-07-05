use anyhow::Result;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::state::{AuthSession, CloudApiState};

pub(crate) const DEFAULT_DEV_TOKEN: &str = "dev-token";
pub(crate) const DEFAULT_ADMIN_USERNAME: &str = "admin";
pub(crate) const DEFAULT_ADMIN_PASSWORD: &str = "dev-password";
pub(crate) const DEFAULT_AUTH_SESSION_TTL_SECONDS: u64 = 24 * 60 * 60;
pub(crate) const INTERNAL_API_TOKEN_HEADER: &str = "x-rust-watcher-token";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudLoginRequest {
    pub(crate) username: String,
    pub(crate) password: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudLoginResponse {
    pub(crate) session_token: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudMeResponse {
    pub(crate) authenticated: bool,
    pub(crate) username: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudLogoutResponse {
    pub(crate) authenticated: bool,
}

pub(crate) async fn cloud_login(
    State(state): State<CloudApiState>,
    Json(request): Json<CloudLoginRequest>,
) -> impl IntoResponse {
    let valid = state
        .auth_users
        .get(&request.username)
        .is_some_and(|password| password == &request.password);
    if !valid {
        return (StatusCode::UNAUTHORIZED, "invalid username or password").into_response();
    }
    let session_token = create_auth_session(&state, request.username);
    Json(CloudLoginResponse { session_token }).into_response()
}

pub(crate) async fn cloud_logout(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };
    match remove_valid_session(&state, token) {
        Ok(()) => Json(CloudLogoutResponse {
            authenticated: false,
        })
        .into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn cloud_me(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match require_cloud_auth(&state, &headers) {
        Ok(username) => Json(CloudMeResponse {
            authenticated: true,
            username,
        })
        .into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) fn require_cloud_auth(
    state: &CloudApiState,
    headers: &HeaderMap,
) -> Result<String, ApiError> {
    let token = bearer_token(headers)?;
    session_owner_for_token(state, token)
}
pub(crate) fn agent_owner_for_token(state: &CloudApiState, token: &str) -> Option<String> {
    if token == state.dev_token.as_str() {
        return Some(state.default_owner_username.to_string());
    }
    session_owner_for_token(state, token).ok()
}

pub(crate) fn require_internal_api_token(
    state: &CloudApiState,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let expected = state
        .internal_api_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| ApiError::Forbidden("internal API is disabled".into()))?;
    let supplied = bearer_token(headers)
        .ok()
        .or_else(|| {
            headers
                .get(INTERNAL_API_TOKEN_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| ApiError::Forbidden("missing internal API token".into()))?;
    if supplied != expected {
        return Err(ApiError::Forbidden("invalid internal API token".into()));
    }
    Ok(())
}

pub(crate) fn create_auth_session(state: &CloudApiState, username: String) -> String {
    let session_token = Uuid::new_v4().to_string();
    let now = unix_timestamp();
    let expires_at = now.saturating_add(state.auth_session_ttl_seconds);
    state.auth_sessions.write().insert(
        session_token.clone(),
        AuthSession {
            username,
            expires_at,
        },
    );
    session_token
}

pub(crate) fn validate_auth_defaults(
    users: &str,
    admin_username: &str,
    admin_password: &str,
    dev_token: &str,
    allow_insecure_dev_auth: bool,
) -> Result<()> {
    if allow_insecure_dev_auth {
        return Ok(());
    }
    let uses_default_admin = if users.trim().is_empty() {
        admin_username == DEFAULT_ADMIN_USERNAME && admin_password == DEFAULT_ADMIN_PASSWORD
    } else {
        users.split(',').map(str::trim).any(|entry| {
            entry.split_once(':').is_some_and(|(username, password)| {
                username.trim() == DEFAULT_ADMIN_USERNAME
                    && password.trim() == DEFAULT_ADMIN_PASSWORD
            })
        })
    };
    let uses_default_dev_token = dev_token == DEFAULT_DEV_TOKEN;
    if uses_default_admin || uses_default_dev_token {
        anyhow::bail!(
            "refusing to start with default development auth; set non-default credentials or RUST_WATCHER_ALLOW_INSECURE_DEV_AUTH=true"
        );
    }
    Ok(())
}

pub(crate) fn parse_auth_users(
    users: &str,
    admin_username: &str,
    admin_password: &str,
) -> Result<HashMap<String, String>> {
    let mut parsed = HashMap::new();
    for raw_entry in users
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let Some((username, password)) = raw_entry.split_once(':') else {
            anyhow::bail!("invalid RUST_WATCHER_USERS entry; expected username:password");
        };
        let username = username.trim();
        let password = password.trim();
        if username.is_empty() || password.is_empty() {
            anyhow::bail!("invalid RUST_WATCHER_USERS entry; username and password are required");
        }
        parsed.insert(username.to_string(), password.to_string());
    }
    if parsed.is_empty() {
        parsed.insert(admin_username.to_string(), admin_password.to_string());
    }
    Ok(parsed)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".into()))
}

fn session_owner_for_token(state: &CloudApiState, token: &str) -> Result<String, ApiError> {
    let now = unix_timestamp();
    let mut sessions = state.auth_sessions.write();
    let Some(session) = sessions.get(token) else {
        return Err(ApiError::Unauthorized("invalid or expired session".into()));
    };
    if session.expires_at <= now {
        sessions.remove(token);
        return Err(ApiError::Unauthorized("invalid or expired session".into()));
    }
    Ok(session.username.clone())
}

fn remove_valid_session(state: &CloudApiState, token: &str) -> Result<(), ApiError> {
    let now = unix_timestamp();
    let mut sessions = state.auth_sessions.write();
    let Some(session) = sessions.get(token) else {
        return Err(ApiError::Unauthorized("invalid or expired session".into()));
    };
    if session.expires_at <= now {
        sessions.remove(token);
        return Err(ApiError::Unauthorized("invalid or expired session".into()));
    }
    sessions.remove(token);
    Ok(())
}

pub(crate) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

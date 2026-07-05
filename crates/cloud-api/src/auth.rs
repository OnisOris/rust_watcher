use anyhow::Result;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::state::CloudApiState;

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
    let session_token = Uuid::new_v4().to_string();
    state
        .auth_sessions
        .write()
        .insert(session_token.clone(), request.username);
    Json(CloudLoginResponse { session_token }).into_response()
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
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".into()))?;
    state
        .auth_sessions
        .read()
        .get(token)
        .cloned()
        .ok_or_else(|| ApiError::Unauthorized("invalid or expired session".into()))
}
pub(crate) fn agent_owner_for_token(state: &CloudApiState, token: &str) -> Option<String> {
    if token == state.dev_token.as_str() {
        return Some(state.default_owner_username.to_string());
    }
    state.auth_sessions.read().get(token).cloned()
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

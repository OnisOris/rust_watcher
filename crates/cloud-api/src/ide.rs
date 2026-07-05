use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use graph_core::WorkspaceFileEntry;
use serde::{Deserialize, Serialize};

use crate::auth::require_cloud_auth;
use crate::state::CloudApiState;
use crate::workspaces::CloudWorkspaceFilesResponse;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudWorkspaceFileContentResponse {
    pub(crate) workspace_id: String,
    pub(crate) revision_id: String,
    pub(crate) file: WorkspaceFileEntry,
    pub(crate) content: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudWorkspaceFileContentQuery {
    pub(crate) path: String,
    pub(crate) revision_id: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveWorkspaceFileRequest {
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) base_revision: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveWorkspaceFileResponse {
    pub(crate) workspace_id: String,
    pub(crate) revision_id: String,
    pub(crate) file: WorkspaceFileEntry,
    pub(crate) files_count: u32,
    pub(crate) total_bytes: u64,
}

pub(crate) async fn cloud_workspace_files(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.current_revision(&id) {
        Ok(mut revision) => {
            revision
                .files
                .sort_by(|left, right| left.path.cmp(&right.path));
            Json(CloudWorkspaceFilesResponse {
                workspace_id: id,
                revision_id: revision.id,
                files: revision.files,
            })
            .into_response()
        }
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn cloud_workspace_file_content(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<CloudWorkspaceFileContentQuery>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.workspace_file_content(&id, &query.path, query.revision_id.as_deref()) {
        Ok(response) => Json(response).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn cloud_save_workspace_file(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SaveWorkspaceFileRequest>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.save_workspace_file(&id, request) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}

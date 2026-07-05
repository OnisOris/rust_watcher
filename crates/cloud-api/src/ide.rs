use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use graph_core::WorkspaceFileEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::auth::require_cloud_auth;
use crate::errors::ApiError;
use crate::state::CloudApiState;
use crate::workspaces::{validate_relative_path, CloudWorkspaceFilesResponse};

const MAX_UNIFIED_DIFF_FILE_BYTES: u64 = 512 * 1024;
const MAX_UNIFIED_DIFF_BYTES: usize = 256 * 1024;

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
pub(crate) struct WorkspaceRevisionDiffQuery {
    pub(crate) base_revision_id: String,
    pub(crate) head_revision_id: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceFileDiffQuery {
    pub(crate) path: String,
    pub(crate) base_revision_id: String,
    pub(crate) head_revision_id: Option<String>,
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceRevisionFileDiffEntry {
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old_content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new_content_hash: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceRevisionDiffResponse {
    pub(crate) workspace_id: String,
    pub(crate) base_revision_id: String,
    pub(crate) head_revision_id: String,
    pub(crate) added_files: Vec<WorkspaceRevisionFileDiffEntry>,
    pub(crate) removed_files: Vec<WorkspaceRevisionFileDiffEntry>,
    pub(crate) modified_files: Vec<WorkspaceRevisionFileDiffEntry>,
    pub(crate) unchanged_count: u32,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceFileDiffResponse {
    pub(crate) workspace_id: String,
    pub(crate) path: String,
    pub(crate) base_revision_id: String,
    pub(crate) head_revision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old_content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) new_content_hash: Option<String>,
    pub(crate) truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unified_diff: Option<String>,
}

impl CloudApiState {
    pub(crate) fn workspace_revision_diff(
        &self,
        workspace_id: &str,
        base_revision_id: &str,
        head_revision_id: Option<&str>,
    ) -> Result<WorkspaceRevisionDiffResponse, ApiError> {
        let base = self
            .get_revision(workspace_id, base_revision_id)
            .ok_or_else(|| ApiError::NotFound("base revision not found".into()))?;
        let head = match head_revision_id {
            Some(head_revision_id) => self
                .get_revision(workspace_id, head_revision_id)
                .ok_or_else(|| ApiError::NotFound("head revision not found".into()))?,
            None => self.current_revision(workspace_id)?,
        };
        let base_files = base
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect::<HashMap<_, _>>();
        let head_files = head
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect::<HashMap<_, _>>();
        let mut added_files = Vec::new();
        let mut removed_files = Vec::new();
        let mut modified_files = Vec::new();
        let mut unchanged_count = 0u32;

        for head_file in &head.files {
            match base_files.get(head_file.path.as_str()) {
                Some(base_file) if base_file.content_hash == head_file.content_hash => {
                    unchanged_count = unchanged_count.saturating_add(1);
                }
                Some(base_file) => modified_files.push(file_diff_entry(
                    head_file.path.clone(),
                    Some(base_file),
                    Some(head_file),
                )),
                None => added_files.push(file_diff_entry(
                    head_file.path.clone(),
                    None,
                    Some(head_file),
                )),
            }
        }
        for base_file in &base.files {
            if !head_files.contains_key(base_file.path.as_str()) {
                removed_files.push(file_diff_entry(
                    base_file.path.clone(),
                    Some(base_file),
                    None,
                ));
            }
        }
        sort_diff_entries(&mut added_files);
        sort_diff_entries(&mut removed_files);
        sort_diff_entries(&mut modified_files);

        Ok(WorkspaceRevisionDiffResponse {
            workspace_id: workspace_id.to_string(),
            base_revision_id: base.id,
            head_revision_id: head.id,
            added_files,
            removed_files,
            modified_files,
            unchanged_count,
        })
    }

    pub(crate) fn workspace_file_diff(
        &self,
        workspace_id: &str,
        path: &str,
        base_revision_id: &str,
        head_revision_id: Option<&str>,
    ) -> Result<WorkspaceFileDiffResponse, ApiError> {
        let path = validate_relative_path(path)?;
        let base = self
            .get_revision(workspace_id, base_revision_id)
            .ok_or_else(|| ApiError::NotFound("base revision not found".into()))?;
        let head = match head_revision_id {
            Some(head_revision_id) => self
                .get_revision(workspace_id, head_revision_id)
                .ok_or_else(|| ApiError::NotFound("head revision not found".into()))?,
            None => self.current_revision(workspace_id)?,
        };
        let old_file = base.files.iter().find(|file| file.path == path);
        let new_file = head.files.iter().find(|file| file.path == path);
        if old_file.is_none() && new_file.is_none() {
            return Err(ApiError::NotFound(
                "file not found in either revision".into(),
            ));
        }
        let too_large = old_file.is_some_and(|file| file.size_bytes > MAX_UNIFIED_DIFF_FILE_BYTES)
            || new_file.is_some_and(|file| file.size_bytes > MAX_UNIFIED_DIFF_FILE_BYTES);
        let (unified_diff, truncated) = if too_large {
            (None, true)
        } else {
            let old_content = read_optional_text_blob(self, old_file)?;
            let new_content = read_optional_text_blob(self, new_file)?;
            let (diff, truncated) = unified_diff_for_texts(
                &path,
                old_file.map(|file| file.path.as_str()),
                new_file.map(|file| file.path.as_str()),
                old_content.as_deref().unwrap_or_default(),
                new_content.as_deref().unwrap_or_default(),
            );
            (Some(diff), truncated)
        };

        Ok(WorkspaceFileDiffResponse {
            workspace_id: workspace_id.to_string(),
            path,
            base_revision_id: base.id,
            head_revision_id: head.id,
            old_size_bytes: old_file.map(|file| file.size_bytes),
            new_size_bytes: new_file.map(|file| file.size_bytes),
            old_content_hash: old_file.map(|file| file.content_hash.clone()),
            new_content_hash: new_file.map(|file| file.content_hash.clone()),
            truncated,
            unified_diff,
        })
    }
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
pub(crate) async fn cloud_workspace_diff(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorkspaceRevisionDiffQuery>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.workspace_revision_diff(
        &id,
        &query.base_revision_id,
        query.head_revision_id.as_deref(),
    ) {
        Ok(response) => Json(response).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn cloud_workspace_file_diff(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorkspaceFileDiffQuery>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state.workspace_file_diff(
        &id,
        &query.path,
        &query.base_revision_id,
        query.head_revision_id.as_deref(),
    ) {
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

fn file_diff_entry(
    path: String,
    old_file: Option<&WorkspaceFileEntry>,
    new_file: Option<&WorkspaceFileEntry>,
) -> WorkspaceRevisionFileDiffEntry {
    WorkspaceRevisionFileDiffEntry {
        path,
        old_size_bytes: old_file.map(|file| file.size_bytes),
        new_size_bytes: new_file.map(|file| file.size_bytes),
        old_content_hash: old_file.map(|file| file.content_hash.clone()),
        new_content_hash: new_file.map(|file| file.content_hash.clone()),
    }
}

fn sort_diff_entries(entries: &mut [WorkspaceRevisionFileDiffEntry]) {
    entries.sort_by(|left, right| left.path.cmp(&right.path));
}

fn read_optional_text_blob(
    state: &CloudApiState,
    file: Option<&WorkspaceFileEntry>,
) -> Result<Option<String>, ApiError> {
    let Some(file) = file else {
        return Ok(None);
    };
    let bytes = state.read_blob(&file.content_hash)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| ApiError::BadRequest("file is not valid UTF-8".into()))
}

fn unified_diff_for_texts(
    path: &str,
    old_label: Option<&str>,
    new_label: Option<&str>,
    old_content: &str,
    new_content: &str,
) -> (String, bool) {
    let old_lines = old_content.lines().collect::<Vec<_>>();
    let new_lines = new_content.lines().collect::<Vec<_>>();
    let mut diff = String::new();
    let mut truncated = false;
    push_diff_line(
        &mut diff,
        &mut truncated,
        format_args!("--- {}\n", old_label.unwrap_or("/dev/null")),
    );
    push_diff_line(
        &mut diff,
        &mut truncated,
        format_args!("+++ {}\n", new_label.unwrap_or("/dev/null")),
    );
    push_diff_line(
        &mut diff,
        &mut truncated,
        format_args!(
            "@@ -1,{} +1,{} @@ {}\n",
            old_lines.len(),
            new_lines.len(),
            path
        ),
    );
    let max_len = old_lines.len().max(new_lines.len());
    for idx in 0..max_len {
        match (old_lines.get(idx), new_lines.get(idx)) {
            (Some(old), Some(new)) if old == new => {
                push_diff_line(&mut diff, &mut truncated, format_args!(" {old}\n"));
            }
            (Some(old), Some(new)) => {
                push_diff_line(&mut diff, &mut truncated, format_args!("-{old}\n"));
                push_diff_line(&mut diff, &mut truncated, format_args!("+{new}\n"));
            }
            (Some(old), None) => {
                push_diff_line(&mut diff, &mut truncated, format_args!("-{old}\n"));
            }
            (None, Some(new)) => {
                push_diff_line(&mut diff, &mut truncated, format_args!("+{new}\n"));
            }
            (None, None) => {}
        }
        if truncated {
            break;
        }
    }
    (diff, truncated)
}

fn push_diff_line(diff: &mut String, truncated: &mut bool, args: std::fmt::Arguments<'_>) {
    if *truncated {
        return;
    }
    let line = args.to_string();
    if diff.len().saturating_add(line.len()) > MAX_UNIFIED_DIFF_BYTES {
        diff.push_str("...\n");
        *truncated = true;
    } else {
        diff.push_str(&line);
    }
}

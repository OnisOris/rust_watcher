use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use base64::Engine as _;
use graph_core::{
    AnalysisJobSource, CreateAnalysisJobRequest, CreateWorkspaceRevisionRequest, LanguageId,
    WorkspaceFileEntry,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

use crate::auth::{agent_owner_for_token, create_auth_session};
use crate::errors::ApiError;
use crate::imports::CloudStartResponse;
use crate::state::{AgentSession, CloudApiState};
use crate::workspaces::{
    language_for_path, requested_analyzers_for_workspace_files, sha256_content_hash,
    validate_relative_path, CreateWorkspaceRequest,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSessionRequest {
    pub(crate) project_name: String,
    pub(crate) token: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSessionResponse {
    pub(crate) session_id: String,
    pub(crate) workspace_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentFilesRequest {
    pub(crate) files: Vec<AgentFileRequest>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentFileRequest {
    pub(crate) path: String,
    pub(crate) language: Option<LanguageId>,
    pub(crate) content_base64: String,
}

pub(crate) async fn agent_create_session(
    State(state): State<CloudApiState>,
    Json(request): Json<AgentSessionRequest>,
) -> impl IntoResponse {
    let owner_username = match agent_owner_for_token(&state, &request.token) {
        Some(username) => username,
        None => return (StatusCode::UNAUTHORIZED, "invalid agent token").into_response(),
    };
    let workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: request.project_name.clone(),
        owner_username: Some(owner_username.clone()),
        source: Some(AnalysisJobSource {
            kind: graph_core::AnalysisJobSourceKind::LocalPath,
            display_name: Some(request.project_name.clone()),
            path: None,
            repository_url: None,
            git_ref: None,
            commit_sha: None,
        }),
    });
    let session_id = Uuid::new_v4().to_string();
    state.agent_sessions.write().insert(
        session_id.clone(),
        AgentSession {
            workspace_id: workspace.id.clone(),
            owner_username,
            project_name: request.project_name,
            files: HashMap::new(),
        },
    );
    Json(AgentSessionResponse {
        session_id,
        workspace_id: workspace.id,
    })
    .into_response()
}
pub(crate) async fn agent_upload_files(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<AgentFilesRequest>,
) -> impl IntoResponse {
    match store_agent_files(&state, &id, request) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn agent_analyze_session(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match analyze_agent_session(&state, &id) {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) fn store_agent_files(
    state: &CloudApiState,
    session_id: &str,
    request: AgentFilesRequest,
) -> Result<(), ApiError> {
    let workspace_id = state
        .agent_sessions
        .read()
        .get(session_id)
        .map(|session| session.workspace_id.clone())
        .ok_or_else(|| ApiError::NotFound("agent session not found".into()))?;
    for file in request.files {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.content_base64)
            .map_err(|error| {
                ApiError::BadRequest(format!("invalid base64 file payload: {error}"))
            })?;
        if bytes.len() as u64 > state.limits.max_file_bytes {
            return Err(ApiError::BadRequest(format!(
                "file exceeds max size: {}",
                file.path
            )));
        }
        let path = validate_relative_path(&file.path)?;
        let content_hash = sha256_content_hash(&bytes);
        state.upload_blob(&workspace_id, &content_hash, &bytes)?;
        let entry = WorkspaceFileEntry {
            path: path.clone(),
            content_hash,
            size_bytes: bytes.len() as u64,
            language: file
                .language
                .or_else(|| language_for_path(Path::new(&path))),
        };
        let mut sessions = state.agent_sessions.write();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| ApiError::NotFound("agent session not found".into()))?;
        session.files.insert(path, entry);
    }
    Ok(())
}
pub(crate) fn analyze_agent_session(
    state: &CloudApiState,
    session_id: &str,
) -> Result<CloudStartResponse, ApiError> {
    let session = state
        .agent_sessions
        .read()
        .get(session_id)
        .cloned()
        .ok_or_else(|| ApiError::NotFound("agent session not found".into()))?;
    let mut files = session.files.values().cloned().collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.is_empty() {
        return Err(ApiError::BadRequest(
            "agent session has no uploaded files".into(),
        ));
    }
    let requested_analyzers = requested_analyzers_for_workspace_files(&files);
    let revision_response = state.create_revision(
        &session.workspace_id,
        CreateWorkspaceRevisionRequest {
            base_revision: None,
            files,
        },
    )?;
    let job = state.create_job_for_request(CreateAnalysisJobRequest {
        source: None,
        requested_analyzers,
        project_name: Some(session.project_name),
        workspace_id: Some(revision_response.workspace.id.clone()),
        revision_id: Some(revision_response.revision.id),
        incremental: false,
        base_revision_id: None,
    })?;
    let session_token = create_auth_session(state, session.owner_username);
    Ok(CloudStartResponse {
        workspace_id: revision_response.workspace.id,
        job_id: job.id,
        status: "queued".into(),
        session_token: Some(session_token),
    })
}

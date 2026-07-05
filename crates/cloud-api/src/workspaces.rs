use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use graph_core::{
    AnalysisJob, AnalysisJobSource, AnalyzerEngine, CloudWorkspace, CreateWorkspaceRevisionRequest,
    CreateWorkspaceRevisionResponse, GraphMode, LanguageId, WorkspaceFileEntry, WorkspaceRevision,
    WorkspaceSyncPlanRequest, WorkspaceSyncPlanResponse,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use tracing::warn;
use uuid::Uuid;

use crate::auth::require_internal_api_token;
use crate::errors::ApiError;
use crate::ide::{
    CloudWorkspaceFileContentResponse, SaveWorkspaceFileRequest, SaveWorkspaceFileResponse,
};
use crate::imports::CollectedWorkspaceFile;
use crate::state::{CloudApiState, StoredBlob};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateWorkspaceRequest {
    pub(crate) display_name: String,
    #[serde(default)]
    pub(crate) owner_username: Option<String>,
    #[serde(default)]
    pub(crate) source: Option<AnalysisJobSource>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListWorkspacesResponse {
    pub(crate) workspaces: Vec<CloudWorkspace>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudWorkspaceStatusResponse {
    pub(crate) workspace_id: String,
    pub(crate) name: String,
    pub(crate) source: CloudWorkspaceSourceResponse,
    pub(crate) status: String,
    pub(crate) file_count: u32,
    pub(crate) current_revision: Option<String>,
    pub(crate) last_job_id: Option<String>,
    pub(crate) last_updated: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudWorkspaceFilesResponse {
    pub(crate) workspace_id: String,
    pub(crate) revision_id: String,
    pub(crate) files: Vec<WorkspaceFileEntry>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudWorkspaceListResponse {
    pub(crate) workspaces: Vec<CloudWorkspaceStatusResponse>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudWorkspaceSourceResponse {
    #[serde(rename = "type")]
    pub(crate) source_type: String,
    pub(crate) url: Option<String>,
    #[serde(rename = "ref")]
    pub(crate) git_ref: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct SnapshotQuery {
    pub(crate) mode: Option<GraphMode>,
}

impl CloudApiState {
    pub(crate) fn create_workspace(&self, request: CreateWorkspaceRequest) -> CloudWorkspace {
        let id = Uuid::new_v4().to_string();
        let now = timestamp();
        let workspace = CloudWorkspace {
            id: id.clone(),
            display_name: request.display_name,
            owner_username: request.owner_username,
            source: request.source,
            current_revision: None,
            files_count: 0,
            total_bytes: 0,
            created_at: Some(now.clone()),
            updated_at: Some(now),
        };
        self.workspaces.write().insert(id, workspace.clone());
        if let Err(error) = self.store.save_workspace(&workspace) {
            warn!(workspace_id = %workspace.id, %error, "failed to persist cloud workspace");
        }
        workspace
    }
    pub(crate) fn list_workspaces(&self) -> Vec<CloudWorkspace> {
        let mut workspaces = self.workspaces.read().values().cloned().collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.id.cmp(&left.id));
        workspaces
    }
    pub(crate) fn workspace_owner<'a>(&'a self, workspace: &'a CloudWorkspace) -> &'a str {
        workspace
            .owner_username
            .as_deref()
            .unwrap_or(self.default_owner_username.as_str())
    }
    pub(crate) fn workspace_belongs_to(&self, workspace: &CloudWorkspace, username: &str) -> bool {
        self.workspace_owner(workspace) == username
    }
    pub(crate) fn list_workspaces_for_user(&self, username: &str) -> Vec<CloudWorkspace> {
        let mut workspaces = self
            .workspaces
            .read()
            .values()
            .filter(|workspace| self.workspace_belongs_to(workspace, username))
            .cloned()
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.id.cmp(&left.id));
        workspaces
    }
    pub(crate) fn get_workspace(&self, id: &str) -> Option<CloudWorkspace> {
        self.workspaces.read().get(id).cloned()
    }
    pub(crate) fn get_workspace_for_user(
        &self,
        id: &str,
        username: &str,
    ) -> Option<CloudWorkspace> {
        self.get_workspace(id)
            .filter(|workspace| self.workspace_belongs_to(workspace, username))
    }
    pub(crate) fn can_access_job(&self, job: &AnalysisJob, username: &str) -> bool {
        let Some(target) = self.get_job_revision_target(&job.id) else {
            return true;
        };
        self.get_workspace_for_user(&target.workspace_id, username)
            .is_some()
    }
    pub(crate) fn sync_plan(
        &self,
        workspace_id: &str,
        request: WorkspaceSyncPlanRequest,
    ) -> Result<WorkspaceSyncPlanResponse, ApiError> {
        if !self.workspaces.read().contains_key(workspace_id) {
            return Err(ApiError::NotFound("workspace not found".into()));
        }
        let blobs = self.blobs.read();
        let mut missing_hashes = Vec::new();
        let mut known_hashes = Vec::new();
        for file in request.files {
            if blobs.contains_key(&file.content_hash) {
                known_hashes.push(file.content_hash);
            } else {
                missing_hashes.push(file.content_hash);
            }
        }
        Ok(WorkspaceSyncPlanResponse {
            missing_hashes,
            known_hashes,
        })
    }
    pub(crate) fn upload_blob(
        &self,
        workspace_id: &str,
        content_hash: &str,
        bytes: &[u8],
    ) -> Result<(StatusCode, StoredBlob), ApiError> {
        if !self.workspaces.read().contains_key(workspace_id) {
            return Err(ApiError::NotFound("workspace not found".into()));
        }
        validate_content_hash(content_hash)?;
        let computed_hash = sha256_content_hash(bytes);
        if computed_hash != content_hash.to_ascii_lowercase() {
            return Err(ApiError::BadRequest("content hash mismatch".into()));
        }
        if let Some(blob) = self.blobs.read().get(content_hash).cloned() {
            return Ok((StatusCode::OK, blob));
        }

        let storage_path = self.blobs_dir.join(storage_name_for_hash(content_hash));
        std::fs::write(&storage_path, bytes)
            .map_err(|error| ApiError::BadRequest(format!("failed to store blob: {error}")))?;
        let blob = StoredBlob {
            content_hash: content_hash.to_string(),
            size_bytes: bytes.len() as u64,
            storage_path: storage_path.display().to_string(),
            created_at: timestamp(),
        };
        self.blobs
            .write()
            .insert(content_hash.to_string(), blob.clone());
        if let Err(error) = self.store.save_blob(&blob) {
            warn!(content_hash = %blob.content_hash, %error, "failed to persist cloud blob metadata");
        }
        Ok((StatusCode::CREATED, blob))
    }
    pub(crate) fn create_revision(
        &self,
        workspace_id: &str,
        request: CreateWorkspaceRevisionRequest,
    ) -> Result<CreateWorkspaceRevisionResponse, ApiError> {
        {
            let workspaces = self.workspaces.read();
            if !workspaces.contains_key(workspace_id) {
                return Err(ApiError::NotFound("workspace not found".into()));
            }
        }
        let blobs = self.blobs.read();
        for file in &request.files {
            if !blobs.contains_key(&file.content_hash) {
                return Err(ApiError::BadRequest(format!(
                    "missing blob {}",
                    file.content_hash
                )));
            }
        }
        drop(blobs);

        let id = Uuid::new_v4().to_string();
        let files_count = request.files.len() as u32;
        let total_bytes = request.files.iter().map(|file| file.size_bytes).sum();
        let revision = WorkspaceRevision {
            id: id.clone(),
            workspace_id: workspace_id.to_string(),
            files: request.files,
            files_count,
            total_bytes,
            parent_revision: request.base_revision,
            created_at: Some(timestamp()),
        };
        self.revisions.write().insert(id.clone(), revision.clone());

        let mut workspaces = self.workspaces.write();
        let workspace = workspaces
            .get_mut(workspace_id)
            .ok_or_else(|| ApiError::NotFound("workspace not found".into()))?;
        workspace.current_revision = Some(id);
        workspace.files_count = files_count;
        workspace.total_bytes = total_bytes;
        workspace.updated_at = Some(timestamp());
        let workspace = workspace.clone();
        drop(workspaces);
        if let Err(error) = self.store.save_revision(&revision) {
            warn!(revision_id = %revision.id, %error, "failed to persist cloud revision");
        }
        if let Err(error) = self.store.save_workspace(&workspace) {
            warn!(workspace_id = %workspace.id, %error, "failed to persist cloud workspace");
        }
        Ok(CreateWorkspaceRevisionResponse {
            workspace,
            revision,
        })
    }
    pub(crate) fn get_revision(
        &self,
        workspace_id: &str,
        revision_id: &str,
    ) -> Option<WorkspaceRevision> {
        self.revisions
            .read()
            .get(revision_id)
            .filter(|revision| revision.workspace_id == workspace_id)
            .cloned()
    }
    pub(crate) fn current_revision(
        &self,
        workspace_id: &str,
    ) -> Result<WorkspaceRevision, ApiError> {
        let workspace = self
            .get_workspace(workspace_id)
            .ok_or_else(|| ApiError::NotFound("workspace not found".into()))?;
        let revision_id = workspace
            .current_revision
            .ok_or_else(|| ApiError::NotFound("workspace has no revision".into()))?;
        self.get_revision(workspace_id, &revision_id)
            .ok_or_else(|| ApiError::NotFound("revision not found".into()))
    }
    pub(crate) fn read_blob(&self, content_hash: &str) -> Result<Vec<u8>, ApiError> {
        let blob = self
            .blobs
            .read()
            .get(content_hash)
            .cloned()
            .ok_or_else(|| ApiError::NotFound("blob not found".into()))?;
        std::fs::read(&blob.storage_path)
            .map_err(|error| ApiError::BadRequest(format!("failed to read blob: {error}")))
    }
    pub(crate) fn workspace_file_content(
        &self,
        workspace_id: &str,
        path: &str,
        revision_id: Option<&str>,
    ) -> Result<CloudWorkspaceFileContentResponse, ApiError> {
        let path = validate_relative_path(path)?;
        let revision = match revision_id {
            Some(revision_id) => self
                .get_revision(workspace_id, revision_id)
                .ok_or_else(|| ApiError::NotFound("revision not found".into()))?,
            None => self.current_revision(workspace_id)?,
        };
        let file = revision
            .files
            .iter()
            .find(|file| file.path == path)
            .cloned()
            .ok_or_else(|| ApiError::NotFound("file not found".into()))?;
        let bytes = self.read_blob(&file.content_hash)?;
        let content = String::from_utf8(bytes)
            .map_err(|_| ApiError::BadRequest("file is not valid UTF-8".into()))?;
        Ok(CloudWorkspaceFileContentResponse {
            workspace_id: workspace_id.to_string(),
            revision_id: revision.id,
            file,
            content,
        })
    }
    pub(crate) fn save_workspace_file(
        &self,
        workspace_id: &str,
        request: SaveWorkspaceFileRequest,
    ) -> Result<SaveWorkspaceFileResponse, ApiError> {
        let path = validate_relative_path(&request.path)?;
        let current = self.current_revision(workspace_id)?;
        if let Some(base_revision) = &request.base_revision {
            if base_revision != &current.id {
                return Err(ApiError::Conflict("workspace revision changed".into()));
            }
        }
        let bytes = request.content.as_bytes();
        if bytes.len() as u64 > self.limits.max_file_bytes {
            return Err(ApiError::BadRequest("file exceeds maximum size".into()));
        }
        let content_hash = sha256_content_hash(bytes);
        self.upload_blob(workspace_id, &content_hash, bytes)?;
        let new_entry = WorkspaceFileEntry {
            path: path.clone(),
            content_hash,
            size_bytes: bytes.len() as u64,
            language: language_for_path(Path::new(&path)),
        };
        let mut files = current.files.clone();
        match files.iter_mut().find(|file| file.path == path) {
            Some(file) => *file = new_entry.clone(),
            None => files.push(new_entry.clone()),
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let response = self.create_revision(
            workspace_id,
            CreateWorkspaceRevisionRequest {
                base_revision: Some(current.id),
                files,
            },
        )?;
        Ok(SaveWorkspaceFileResponse {
            workspace_id: response.workspace.id,
            revision_id: response.revision.id,
            file: new_entry,
            files_count: response.revision.files_count,
            total_bytes: response.revision.total_bytes,
        })
    }
}

pub(crate) async fn create_workspace(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    (StatusCode::CREATED, Json(state.create_workspace(request))).into_response()
}
pub(crate) async fn list_workspaces(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    Json(ListWorkspacesResponse {
        workspaces: state.list_workspaces(),
    })
    .into_response()
}
pub(crate) async fn get_workspace(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.get_workspace(&id) {
        Some(workspace) => Json(workspace).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
pub(crate) async fn sync_plan(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<WorkspaceSyncPlanRequest>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.sync_plan(&id, request) {
        Ok(plan) => Json(plan).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn upload_blob(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath((id, content_hash)): AxumPath<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.upload_blob(&id, &content_hash, &body) {
        Ok((status, _blob)) => status.into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn create_revision(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<CreateWorkspaceRevisionRequest>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.create_revision(&id, request) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn get_revision(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath((workspace_id, revision_id)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.get_revision(&workspace_id, &revision_id) {
        Some(revision) => Json(revision).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) fn validate_relative_path(path: &str) -> Result<String, ApiError> {
    let candidate = Path::new(path);
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ApiError::BadRequest("path escapes workspace".into()))
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(ApiError::BadRequest("empty path".into()));
    }
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}
pub(crate) fn should_ignore_cloud_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if text.contains("/.git/")
        || text.contains("/target/")
        || text.contains("/node_modules/")
        || text.contains("/.venv/")
        || text.contains("/dist/")
        || text.contains("/build/")
        || text.contains("/.cache/")
        || text.contains("/.idea/")
        || text.contains("/.vscode/")
    {
        return true;
    }
    path.components().any(|component| match component {
        Component::Normal(name) => matches!(
            name.to_str(),
            Some(
                ".git"
                    | "target"
                    | "node_modules"
                    | ".venv"
                    | "dist"
                    | "build"
                    | ".cache"
                    | ".idea"
                    | ".vscode"
            )
        ),
        _ => false,
    })
}
pub(crate) fn is_sync_file_path(path: &Path) -> bool {
    language_for_path(path).is_some()
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                matches!(
                    name,
                    "Cargo.toml"
                        | "Cargo.lock"
                        | "rust-toolchain"
                        | "rust-toolchain.toml"
                        | "package.json"
                        | "pnpm-lock.yaml"
                        | "package-lock.json"
                        | "yarn.lock"
                        | "tsconfig.json"
                        | "pyproject.toml"
                        | "uv.lock"
                        | "requirements.txt"
                )
            })
}
pub(crate) fn language_for_path(path: &Path) -> Option<LanguageId> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => Some(LanguageId::Rust),
        Some("py") => Some(LanguageId::Python),
        Some("ts" | "tsx") => Some(LanguageId::TypeScript),
        Some("js" | "jsx") => Some(LanguageId::JavaScript),
        Some("qml") => Some(LanguageId::Qml),
        _ => None,
    }
}
pub(crate) fn requested_analyzers_for_collected_files(
    files: &[CollectedWorkspaceFile],
) -> Vec<AnalyzerEngine> {
    let entries = files
        .iter()
        .map(|file| file.entry.clone())
        .collect::<Vec<_>>();
    requested_analyzers_for_workspace_files(&entries)
}
pub(crate) fn requested_analyzers_for_workspace_files(
    files: &[WorkspaceFileEntry],
) -> Vec<AnalyzerEngine> {
    if files.iter().any(|file| file.path == "Cargo.toml") {
        vec![AnalyzerEngine::RustAnalyzer]
    } else {
        Vec::new()
    }
}
pub(crate) fn validate_content_hash(content_hash: &str) -> Result<(), ApiError> {
    let Some(hex) = content_hash.strip_prefix("sha256:") else {
        return Err(ApiError::BadRequest("invalid content hash format".into()));
    };
    if hex.len() != 64 || !hex.chars().all(|char| char.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid content hash format".into()));
    }
    Ok(())
}
pub(crate) fn sha256_content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("sha256:{hex}")
}
pub(crate) fn storage_name_for_hash(content_hash: &str) -> String {
    content_hash.replace(':', "_")
}
pub(crate) fn materialize_revision(
    state: &CloudApiState,
    workspace_id: &str,
    revision_id: &str,
) -> Result<PathBuf> {
    let revision = state
        .get_revision(workspace_id, revision_id)
        .ok_or_else(|| anyhow::anyhow!("revision not found"))?;
    if revision.workspace_id != workspace_id {
        anyhow::bail!("revision does not belong to workspace");
    }

    let workspace_root = state.workspaces_dir.join(workspace_id);
    let target_root = workspace_root.join(revision_id);
    if target_root.exists() {
        std::fs::remove_dir_all(&target_root)
            .with_context(|| format!("failed to clean {}", target_root.display()))?;
    }
    std::fs::create_dir_all(&target_root)
        .with_context(|| format!("failed to create {}", target_root.display()))?;

    for file in &revision.files {
        let target_path = materialized_child_path(&target_root, &file.path)
            .with_context(|| format!("invalid workspace path {}", file.path))?;
        let blob = state
            .blobs
            .read()
            .get(&file.content_hash)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing blob {}", file.content_hash))?;
        let bytes = std::fs::read(&blob.storage_path)
            .with_context(|| format!("failed to read blob {}", file.content_hash))?;
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&target_path, bytes)
            .with_context(|| format!("failed to write {}", target_path.display()))?;
    }

    Ok(target_root)
}
pub(crate) fn materialized_child_path(root: &Path, relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    let mut child = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => child.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("path escapes workspace")
            }
        }
    }
    if child.as_os_str().is_empty() {
        anyhow::bail!("empty workspace path");
    }
    let target = root.join(child);
    if !target.starts_with(root) {
        anyhow::bail!("path escapes workspace");
    }
    Ok(target)
}
pub(crate) fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

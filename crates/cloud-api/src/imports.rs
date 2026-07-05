use anyhow::Result;
use axum::extract::{Multipart, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use graph_core::{
    AnalysisJobSource, CreateAnalysisJobRequest, CreateWorkspaceRevisionRequest, WorkspaceFileEntry,
};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;
use tokio::process::Command;
use uuid::Uuid;

use crate::auth::require_cloud_auth;
use crate::errors::ApiError;
use crate::state::{CloudApiState, CloudLimits};
use crate::workspaces::{
    is_sync_file_path, language_for_path, materialized_child_path,
    requested_analyzers_for_collected_files, sha256_content_hash, should_ignore_cloud_path,
    CreateWorkspaceRequest,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubImportRequest {
    pub(crate) url: String,
    #[serde(rename = "ref")]
    pub(crate) git_ref: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudStartResponse {
    pub(crate) workspace_id: String,
    pub(crate) job_id: String,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_token: Option<String>,
}
#[derive(Debug)]
pub(crate) struct CollectedWorkspaceFile {
    pub(crate) entry: WorkspaceFileEntry,
    pub(crate) bytes: Vec<u8>,
}
#[derive(Debug)]
pub(crate) struct ParsedGithubUrl {
    pub(crate) repo: String,
    pub(crate) web_url: String,
    pub(crate) clone_url: String,
    pub(crate) git_ref: Option<String>,
}

pub(crate) async fn cloud_import_github(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    Json(request): Json<GithubImportRequest>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    match import_github_workspace(&state, &username, request).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn cloud_upload_zip(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    match upload_zip_workspace(&state, &username, &mut multipart).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn import_github_workspace(
    state: &CloudApiState,
    owner_username: &str,
    request: GithubImportRequest,
) -> Result<CloudStartResponse, ApiError> {
    let github = parse_github_url(&request.url)?;
    let git_ref = request
        .git_ref
        .or(github.git_ref)
        .filter(|value| !value.trim().is_empty());
    if let Some(git_ref) = &git_ref {
        validate_git_ref(git_ref)?;
    }
    let import_root = state.workspaces_dir.join("_imports");
    std::fs::create_dir_all(&import_root)
        .map_err(|error| ApiError::BadRequest(format!("failed to prepare import root: {error}")))?;
    let checkout_dir = import_root.join(Uuid::new_v4().to_string());
    let mut command = Command::new("git");
    command
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--no-tags");
    if let Some(git_ref) = &git_ref {
        command.arg("--branch").arg(git_ref);
    }
    command.arg(&github.clone_url).arg(&checkout_dir);
    let output = command
        .output()
        .await
        .map_err(|error| ApiError::BadRequest(format!("failed to start git: {error}")))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let _ = std::fs::remove_dir_all(&checkout_dir);
        return Err(ApiError::BadRequest(if message.is_empty() {
            "git clone failed".into()
        } else {
            format!("git clone failed: {message}")
        }));
    }
    import_project_directory(
        state,
        owner_username,
        &checkout_dir,
        github.repo.as_str(),
        Some(AnalysisJobSource {
            kind: graph_core::AnalysisJobSourceKind::GitRepository,
            display_name: Some(github.repo.clone()),
            path: None,
            repository_url: Some(github.web_url),
            git_ref,
            commit_sha: None,
        }),
    )
}
pub(crate) async fn upload_zip_workspace(
    state: &CloudApiState,
    owner_username: &str,
    multipart: &mut Multipart,
) -> Result<CloudStartResponse, ApiError> {
    let mut archive = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(format!("invalid multipart upload: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name != "file" && archive.is_some() {
            continue;
        }
        let file_name = field.file_name().map(str::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|error| ApiError::BadRequest(format!("failed to read upload: {error}")))?;
        if bytes.len() as u64 > state.limits.max_upload_bytes {
            return Err(ApiError::BadRequest(
                "archive exceeds max upload size".into(),
            ));
        }
        archive = Some((file_name, bytes));
    }
    let Some((file_name, bytes)) = archive else {
        return Err(ApiError::BadRequest(
            "multipart field 'file' is required".into(),
        ));
    };
    if let Some(file_name) = &file_name {
        if !file_name.to_ascii_lowercase().ends_with(".zip") {
            return Err(ApiError::BadRequest(
                "only .zip uploads are supported".into(),
            ));
        }
    }
    let import_root = state.workspaces_dir.join("_uploads");
    std::fs::create_dir_all(&import_root)
        .map_err(|error| ApiError::BadRequest(format!("failed to prepare upload root: {error}")))?;
    let unpack_dir = import_root.join(Uuid::new_v4().to_string());
    unpack_zip_bytes(state, &bytes, &unpack_dir)?;
    let display_name = file_name
        .as_deref()
        .map(zip_display_name)
        .unwrap_or_else(|| "uploaded-project".into());
    import_project_directory(
        state,
        owner_username,
        &unpack_dir,
        &display_name,
        Some(AnalysisJobSource {
            kind: graph_core::AnalysisJobSourceKind::UploadedArchive,
            display_name: Some(display_name.clone()),
            path: None,
            repository_url: None,
            git_ref: None,
            commit_sha: None,
        }),
    )
}
pub(crate) fn import_project_directory(
    state: &CloudApiState,
    owner_username: &str,
    root: &Path,
    display_name: &str,
    source: Option<AnalysisJobSource>,
) -> Result<CloudStartResponse, ApiError> {
    let workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: display_name.to_string(),
        owner_username: Some(owner_username.to_string()),
        source,
    });
    let files = collect_workspace_files(root, &state.limits)?;
    let requested_analyzers = requested_analyzers_for_collected_files(&files);
    store_workspace_files(state, &workspace.id, &files)?;
    let revision_files = files.into_iter().map(|file| file.entry).collect::<Vec<_>>();
    let revision_response = state.create_revision(
        &workspace.id,
        CreateWorkspaceRevisionRequest {
            base_revision: None,
            files: revision_files,
        },
    )?;
    let job = state.create_job_for_request(CreateAnalysisJobRequest {
        source: None,
        requested_analyzers,
        project_name: Some(workspace.display_name),
        workspace_id: Some(revision_response.workspace.id.clone()),
        revision_id: Some(revision_response.revision.id),
    })?;
    Ok(CloudStartResponse {
        workspace_id: revision_response.workspace.id,
        job_id: job.id,
        status: "queued".into(),
        session_token: None,
    })
}
pub(crate) fn store_workspace_files(
    state: &CloudApiState,
    workspace_id: &str,
    files: &[CollectedWorkspaceFile],
) -> Result<(), ApiError> {
    for file in files {
        state.upload_blob(workspace_id, &file.entry.content_hash, &file.bytes)?;
    }
    Ok(())
}
pub(crate) fn collect_workspace_files(
    root: &Path,
    limits: &CloudLimits,
) -> Result<Vec<CollectedWorkspaceFile>, ApiError> {
    let root = root.canonicalize().map_err(|error| {
        ApiError::BadRequest(format!("failed to canonicalize project: {error}"))
    })?;
    let mut files = Vec::new();
    collect_workspace_files_inner(&root, &root, limits, &mut files)?;
    files.sort_by(|left, right| left.entry.path.cmp(&right.entry.path));
    if files.len() > limits.max_file_count {
        return Err(ApiError::BadRequest("project has too many files".into()));
    }
    Ok(files)
}
pub(crate) fn collect_workspace_files_inner(
    root: &Path,
    current: &Path,
    limits: &CloudLimits,
    files: &mut Vec<CollectedWorkspaceFile>,
) -> Result<(), ApiError> {
    let entries = std::fs::read_dir(current)
        .map_err(|error| ApiError::BadRequest(format!("failed to read directory: {error}")))?;
    for entry in entries {
        let entry = entry.map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if should_ignore_cloud_path(relative) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        if metadata.is_dir() {
            collect_workspace_files_inner(root, &path, limits, files)?;
            continue;
        }
        if !metadata.is_file() || !is_sync_file_path(&path) {
            continue;
        }
        if metadata.len() > limits.max_file_bytes {
            return Err(ApiError::BadRequest(format!(
                "file exceeds max size: {}",
                relative.display()
            )));
        }
        if files.len() >= limits.max_file_count {
            return Err(ApiError::BadRequest("project has too many files".into()));
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| ApiError::BadRequest(format!("failed to read file: {error}")))?;
        let relative_path = project_indexer::relative_to(root, &path);
        files.push(CollectedWorkspaceFile {
            entry: WorkspaceFileEntry {
                path: relative_path,
                content_hash: sha256_content_hash(&bytes),
                size_bytes: bytes.len() as u64,
                language: language_for_path(&path),
            },
            bytes,
        });
    }
    Ok(())
}
pub(crate) fn parse_github_url(input: &str) -> Result<ParsedGithubUrl, ApiError> {
    let url =
        url::Url::parse(input).map_err(|_| ApiError::BadRequest("invalid GitHub URL".into()))?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return Err(ApiError::BadRequest(
            "unsupported repository host; only public github.com repositories are supported".into(),
        ));
    }
    let segments = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    if segments.len() < 2 {
        return Err(ApiError::BadRequest(
            "GitHub URL must include owner and repo".into(),
        ));
    }
    let owner = clean_github_component(segments[0], "owner")?;
    let repo = clean_github_component(segments[1].trim_end_matches(".git"), "repo")?;
    let git_ref = if segments.get(2) == Some(&"tree") && segments.len() >= 4 {
        let joined = segments[3..].join("/");
        validate_git_ref(&joined)?;
        Some(joined)
    } else {
        None
    };
    Ok(ParsedGithubUrl {
        repo: repo.clone(),
        web_url: format!("https://github.com/{owner}/{repo}"),
        clone_url: format!("https://github.com/{owner}/{repo}.git"),
        git_ref,
    })
}
pub(crate) fn clean_github_component(value: &str, label: &str) -> Result<String, ApiError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
    {
        return Err(ApiError::BadRequest(format!("invalid GitHub {label}")));
    }
    Ok(value.to_string())
}
pub(crate) fn validate_git_ref(value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 200
        || value.contains("..")
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.chars().any(|char| {
            char.is_control() || matches!(char, ' ' | '~' | '^' | ':' | '?' | '*' | '[')
        })
    {
        return Err(ApiError::BadRequest("invalid git ref".into()));
    }
    Ok(())
}
pub(crate) fn unpack_zip_bytes(
    state: &CloudApiState,
    bytes: &[u8],
    target_root: &Path,
) -> Result<(), ApiError> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|error| ApiError::BadRequest(format!("invalid zip archive: {error}")))?;
    if archive.len() > state.limits.max_file_count {
        return Err(ApiError::BadRequest("archive has too many entries".into()));
    }
    std::fs::create_dir_all(target_root).map_err(|error| {
        ApiError::BadRequest(format!("failed to create upload workspace: {error}"))
    })?;
    let mut unpacked_bytes = 0u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| ApiError::BadRequest(format!("failed to read zip entry: {error}")))?;
        if file.is_dir() {
            continue;
        }
        if file.enclosed_name().is_none() {
            return Err(ApiError::BadRequest("zip entry escapes workspace".into()));
        }
        let entry_name = file.name().to_string();
        let output = materialized_child_path(target_root, &entry_name)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let size = file.size();
        if size > state.limits.max_file_bytes {
            return Err(ApiError::BadRequest(format!(
                "zip entry exceeds max size: {entry_name}"
            )));
        }
        unpacked_bytes = unpacked_bytes.saturating_add(size);
        if unpacked_bytes > state.limits.max_unpacked_bytes {
            return Err(ApiError::BadRequest(
                "archive exceeds max unpacked size".into(),
            ));
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ApiError::BadRequest(format!("failed to create zip output directory: {error}"))
            })?;
        }
        let mut bytes = Vec::with_capacity(size.min(state.limits.max_file_bytes) as usize);
        file.read_to_end(&mut bytes).map_err(|error| {
            ApiError::BadRequest(format!("failed to extract zip entry: {error}"))
        })?;
        std::fs::write(&output, bytes)
            .map_err(|error| ApiError::BadRequest(format!("failed to write zip entry: {error}")))?;
    }
    Ok(())
}
pub(crate) fn zip_display_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("uploaded-project")
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
        .collect::<String>()
}

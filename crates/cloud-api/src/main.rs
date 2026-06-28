use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use graph_core::{
    AnalysisJob, AnalysisJobSource, AnalysisJobStatus, AnalyzerEngine, AnalyzerServiceStatus,
    CloudWorkspace, CreateWorkspaceRevisionRequest, CreateWorkspaceRevisionResponse,
    WorkspaceRevision, WorkspaceSyncPlanRequest, WorkspaceSyncPlanResponse,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "cloud-api")]
#[command(about = "Cloud API skeleton for asynchronous project analysis jobs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve(ServeArgs),
}

#[derive(Parser, Clone)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[arg(long, default_value = ".rust-watcher-cloud/blobs")]
    blobs_dir: PathBuf,
}

#[derive(Clone)]
struct CloudApiState {
    jobs: Arc<RwLock<HashMap<String, AnalysisJob>>>,
    workspaces: Arc<RwLock<HashMap<String, CloudWorkspace>>>,
    revisions: Arc<RwLock<HashMap<String, WorkspaceRevision>>>,
    blobs: Arc<RwLock<HashMap<String, StoredBlob>>>,
    blobs_dir: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct StoredBlob {
    content_hash: String,
    size_bytes: u64,
    storage_path: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApiError {
    NotFound(String),
    BadRequest(String),
}

impl CloudApiState {
    fn new(blobs_dir: PathBuf) -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            workspaces: Arc::new(RwLock::new(HashMap::new())),
            revisions: Arc::new(RwLock::new(HashMap::new())),
            blobs: Arc::new(RwLock::new(HashMap::new())),
            blobs_dir: Arc::new(blobs_dir),
        }
    }

    fn create_job(&self, request: CreateAnalysisJobRequest) -> Result<AnalysisJob, ApiError> {
        let (source, project_name, message) = match (&request.workspace_id, &request.revision_id) {
            (Some(workspace_id), Some(revision_id)) => {
                let workspace = self
                    .get_workspace(workspace_id)
                    .ok_or_else(|| ApiError::NotFound("workspace not found".into()))?;
                let _revision = self
                    .get_revision(workspace_id, revision_id)
                    .ok_or_else(|| ApiError::NotFound("revision not found".into()))?;
                (
                    workspace.source.clone().unwrap_or(AnalysisJobSource {
                        kind: graph_core::AnalysisJobSourceKind::LocalPath,
                        display_name: Some(workspace.display_name.clone()),
                        path: None,
                        repository_url: None,
                        git_ref: None,
                        commit_sha: None,
                    }),
                    request.project_name.or(Some(workspace.display_name)),
                    "Queued for cloud analysis",
                )
            }
            _ => (
                request
                    .source
                    .ok_or_else(|| ApiError::BadRequest("source is required".into()))?,
                request.project_name,
                "Queued for analysis",
            ),
        };
        let id = Uuid::new_v4().to_string();
        let job = AnalysisJob {
            id: id.clone(),
            status: AnalysisJobStatus::Queued,
            source,
            project_name,
            message: Some(message.into()),
            progress: Some(0),
            requested_analyzers: request.requested_analyzers,
            analyzer_statuses: Vec::<AnalyzerServiceStatus>::new(),
            created_at: None,
            started_at: None,
            finished_at: None,
            credits_estimated: None,
            credits_used: None,
            error: None,
        };
        self.jobs.write().insert(id, job.clone());
        Ok(job)
    }

    fn get_job(&self, id: &str) -> Option<AnalysisJob> {
        self.jobs.read().get(id).cloned()
    }

    fn list_jobs(&self) -> Vec<AnalysisJob> {
        let mut jobs = self.jobs.read().values().cloned().collect::<Vec<_>>();
        jobs.sort_by(|left, right| right.id.cmp(&left.id));
        jobs
    }

    fn cancel_job(&self, id: &str) -> Option<AnalysisJob> {
        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(id)?;
        if !is_terminal(job.status) {
            job.status = AnalysisJobStatus::Cancelled;
            job.message = Some("Cancelled".into());
        }
        Some(job.clone())
    }

    fn create_workspace(&self, request: CreateWorkspaceRequest) -> CloudWorkspace {
        let id = Uuid::new_v4().to_string();
        let now = timestamp();
        let workspace = CloudWorkspace {
            id: id.clone(),
            display_name: request.display_name,
            source: request.source,
            current_revision: None,
            files_count: 0,
            total_bytes: 0,
            created_at: Some(now.clone()),
            updated_at: Some(now),
        };
        self.workspaces.write().insert(id, workspace.clone());
        workspace
    }

    fn list_workspaces(&self) -> Vec<CloudWorkspace> {
        let mut workspaces = self.workspaces.read().values().cloned().collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.id.cmp(&left.id));
        workspaces
    }

    fn get_workspace(&self, id: &str) -> Option<CloudWorkspace> {
        self.workspaces.read().get(id).cloned()
    }

    fn sync_plan(
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

    fn upload_blob(
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
        Ok((StatusCode::CREATED, blob))
    }

    fn create_revision(
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
        Ok(CreateWorkspaceRevisionResponse {
            workspace: workspace.clone(),
            revision,
        })
    }

    fn get_revision(&self, workspace_id: &str, revision_id: &str) -> Option<WorkspaceRevision> {
        self.revisions
            .read()
            .get(revision_id)
            .filter(|revision| revision.workspace_id == workspace_id)
            .cloned()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAnalysisJobRequest {
    #[serde(default)]
    source: Option<AnalysisJobSource>,
    #[serde(default)]
    requested_analyzers: Vec<AnalyzerEngine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_id: Option<String>,
    project_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkspaceRequest {
    display_name: String,
    #[serde(default)]
    source: Option<AnalysisJobSource>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListWorkspacesResponse {
    workspaces: Vec<CloudWorkspace>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListAnalysisJobsResponse {
    jobs: Vec<AnalysisJob>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
    version: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cloud_api=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve(args) => serve(args).await,
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    std::fs::create_dir_all(&args.blobs_dir)
        .with_context(|| format!("failed to create {}", args.blobs_dir.display()))?;
    let state = CloudApiState::new(args.blobs_dir.clone());
    let app = Router::new()
        .route("/api/health", get(health))
        .route(
            "/api/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route("/api/workspaces/{id}", get(get_workspace))
        .route("/api/workspaces/{id}/sync-plan", post(sync_plan))
        .route(
            "/api/workspaces/{id}/blobs/{content_hash}",
            put(upload_blob),
        )
        .route("/api/workspaces/{id}/revisions", post(create_revision))
        .route(
            "/api/workspaces/{workspace_id}/revisions/{revision_id}",
            get(get_revision),
        )
        .route("/api/analysis/jobs", get(list_jobs).post(create_job))
        .route("/api/analysis/jobs/{id}", get(get_job))
        .route("/api/analysis/jobs/{id}/cancel", post(cancel_job))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from((args.host, args.port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    let local_addr = listener.local_addr().context("failed to read local addr")?;
    info!(%local_addr, "cloud-api listening");
    axum::serve(listener, app)
        .await
        .context("cloud-api server failed")
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "cloud-api",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn create_job(
    State(state): State<CloudApiState>,
    Json(request): Json<CreateAnalysisJobRequest>,
) -> impl IntoResponse {
    match state.create_job(request) {
        Ok(job) => (StatusCode::CREATED, Json(job)).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn get_job(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.get_job(&id) {
        Some(job) => (StatusCode::OK, Json(job)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn list_jobs(State(state): State<CloudApiState>) -> Json<ListAnalysisJobsResponse> {
    Json(ListAnalysisJobsResponse {
        jobs: state.list_jobs(),
    })
}

async fn cancel_job(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.cancel_job(&id) {
        Some(job) => Json(job).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn is_terminal(status: AnalysisJobStatus) -> bool {
    matches!(
        status,
        AnalysisJobStatus::Completed | AnalysisJobStatus::Failed | AnalysisJobStatus::Cancelled
    )
}

async fn create_workspace(
    State(state): State<CloudApiState>,
    Json(request): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    (StatusCode::CREATED, Json(state.create_workspace(request)))
}

async fn list_workspaces(State(state): State<CloudApiState>) -> Json<ListWorkspacesResponse> {
    Json(ListWorkspacesResponse {
        workspaces: state.list_workspaces(),
    })
}

async fn get_workspace(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.get_workspace(&id) {
        Some(workspace) => Json(workspace).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn sync_plan(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<WorkspaceSyncPlanRequest>,
) -> impl IntoResponse {
    match state.sync_plan(&id, request) {
        Ok(plan) => Json(plan).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn upload_blob(
    State(state): State<CloudApiState>,
    AxumPath((id, content_hash)): AxumPath<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    match state.upload_blob(&id, &content_hash, &body) {
        Ok((status, _blob)) => status.into_response(),
        Err(error) => error.into_response(),
    }
}

async fn create_revision(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<CreateWorkspaceRevisionRequest>,
) -> impl IntoResponse {
    match state.create_revision(&id, request) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn get_revision(
    State(state): State<CloudApiState>,
    AxumPath((workspace_id, revision_id)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    match state.get_revision(&workspace_id, &revision_id) {
        Some(revision) => Json(revision).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::NotFound(message) => (StatusCode::NOT_FOUND, message).into_response(),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
        }
    }
}

fn validate_content_hash(content_hash: &str) -> Result<(), ApiError> {
    let Some(hex) = content_hash.strip_prefix("sha256:") else {
        return Err(ApiError::BadRequest("invalid content hash format".into()));
    };
    if hex.len() != 64 || !hex.chars().all(|char| char.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid content hash format".into()));
    }
    Ok(())
}

fn sha256_content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("sha256:{hex}")
}

fn storage_name_for_hash(content_hash: &str) -> String {
    content_hash.replace(':', "_")
}

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{
        AnalysisJobSourceKind, AnalyzerProvider, LanguageId, WorkspaceFileEntry,
        WorkspaceSyncPlanRequest,
    };

    fn test_state() -> CloudApiState {
        let root = std::env::temp_dir().join(format!("rust-watcher-cloud-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        CloudApiState::new(root)
    }

    fn local_request() -> CreateAnalysisJobRequest {
        CreateAnalysisJobRequest {
            source: Some(AnalysisJobSource {
                kind: AnalysisJobSourceKind::LocalPath,
                display_name: Some("demo".into()),
                path: Some("/tmp/demo".into()),
                repository_url: None,
                git_ref: None,
                commit_sha: None,
            }),
            requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
            workspace_id: None,
            revision_id: None,
            project_name: Some("demo".into()),
        }
    }

    fn workspace_request() -> CreateWorkspaceRequest {
        CreateWorkspaceRequest {
            display_name: "demo".into(),
            source: None,
        }
    }

    fn file_entry(content: &[u8]) -> WorkspaceFileEntry {
        WorkspaceFileEntry {
            path: "src/main.rs".into(),
            content_hash: sha256_content_hash(content),
            size_bytes: content.len() as u64,
            language: Some(LanguageId::Rust),
        }
    }

    fn terminal_job(status: AnalysisJobStatus) -> AnalysisJob {
        AnalysisJob {
            id: Uuid::new_v4().to_string(),
            status,
            source: local_request().source.unwrap(),
            project_name: Some("demo".into()),
            message: Some("terminal".into()),
            progress: Some(100),
            requested_analyzers: Vec::new(),
            analyzer_statuses: vec![AnalyzerServiceStatus {
                id: "rust-analyzer".into(),
                kind: graph_core::AnalyzerKind::Rust,
                engine: AnalyzerEngine::RustAnalyzer,
                label: "rust-analyzer".into(),
                status: graph_core::AnalyzerStatus::Ready,
                mode: None,
                message: None,
                capabilities: Vec::new(),
                files_indexed: 1,
                last_updated: None,
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            }],
            created_at: None,
            started_at: None,
            finished_at: None,
            credits_estimated: None,
            credits_used: None,
            error: None,
        }
    }

    #[test]
    fn creating_job_stores_queued_job() {
        let state = test_state();
        let job = state.create_job(local_request()).unwrap();

        assert_eq!(job.status, AnalysisJobStatus::Queued);
        assert_eq!(job.message.as_deref(), Some("Queued for analysis"));
        assert_eq!(job.progress, Some(0));
        assert_eq!(job.requested_analyzers, vec![AnalyzerEngine::RustAnalyzer]);
        assert_eq!(state.get_job(&job.id).unwrap().id, job.id);
    }

    #[test]
    fn getting_known_job_returns_it() {
        let state = test_state();
        let job = state.create_job(local_request()).unwrap();

        assert_eq!(
            state.get_job(&job.id).unwrap().project_name.as_deref(),
            Some("demo")
        );
    }

    #[test]
    fn cancelling_queued_job_marks_cancelled() {
        let state = test_state();
        let job = state.create_job(local_request()).unwrap();
        let cancelled = state.cancel_job(&job.id).unwrap();

        assert_eq!(cancelled.status, AnalysisJobStatus::Cancelled);
        assert_eq!(cancelled.message.as_deref(), Some("Cancelled"));
    }

    #[test]
    fn cancelling_terminal_job_leaves_it_unchanged() {
        for status in [
            AnalysisJobStatus::Completed,
            AnalysisJobStatus::Failed,
            AnalysisJobStatus::Cancelled,
        ] {
            let state = test_state();
            let job = terminal_job(status);
            let id = job.id.clone();
            state.jobs.write().insert(id.clone(), job.clone());

            let after_cancel = state.cancel_job(&id).unwrap();

            assert_eq!(after_cancel.status, status);
            assert_eq!(after_cancel.message, job.message);
            assert_eq!(after_cancel.progress, job.progress);
        }
    }

    #[test]
    fn unknown_job_lookup_and_cancel_are_missing() {
        let state = test_state();

        assert!(state.get_job("missing").is_none());
        assert!(state.cancel_job("missing").is_none());
    }

    #[test]
    fn creating_workspace_stores_empty_workspace() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());

        assert_eq!(workspace.display_name, "demo");
        assert_eq!(workspace.current_revision, None);
        assert_eq!(workspace.files_count, 0);
        assert_eq!(state.get_workspace(&workspace.id).unwrap().id, workspace.id);
    }

    #[test]
    fn sync_plan_for_unknown_workspace_is_missing() {
        let state = test_state();
        let request = WorkspaceSyncPlanRequest {
            base_revision: None,
            files: Vec::new(),
        };

        assert_eq!(
            state.sync_plan("missing", request).unwrap_err(),
            ApiError::NotFound("workspace not found".into())
        );
    }

    #[test]
    fn sync_plan_reports_all_hashes_missing_when_blob_store_empty() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let file = file_entry(b"fn main() {}");
        let plan = state
            .sync_plan(
                &workspace.id,
                WorkspaceSyncPlanRequest {
                    base_revision: None,
                    files: vec![file.clone()],
                },
            )
            .unwrap();

        assert_eq!(plan.missing_hashes, vec![file.content_hash]);
        assert!(plan.known_hashes.is_empty());
    }

    #[test]
    fn uploading_blob_with_valid_sha256_stores_it() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let content = b"fn main() {}";
        let hash = sha256_content_hash(content);
        let (status, blob) = state.upload_blob(&workspace.id, &hash, content).unwrap();

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(blob.content_hash, hash);
        assert_eq!(blob.size_bytes, content.len() as u64);
        assert!(PathBuf::from(blob.storage_path).exists());
    }

    #[test]
    fn uploading_blob_with_mismatched_sha256_is_bad_request() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let wrong_hash = sha256_content_hash(b"different");

        assert_eq!(
            state
                .upload_blob(&workspace.id, &wrong_hash, b"actual")
                .unwrap_err(),
            ApiError::BadRequest("content hash mismatch".into())
        );
    }

    #[test]
    fn creating_revision_fails_when_file_references_missing_blob() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let file = file_entry(b"fn main() {}");

        let result = state.create_revision(
            &workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: None,
                files: vec![file.clone()],
            },
        );

        assert_eq!(
            result.unwrap_err(),
            ApiError::BadRequest(format!("missing blob {}", file.content_hash))
        );
    }

    #[test]
    fn creating_revision_succeeds_after_required_blobs_uploaded() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let content = b"fn main() {}";
        let file = file_entry(content);
        state
            .upload_blob(&workspace.id, &file.content_hash, content)
            .unwrap();

        let response = state
            .create_revision(
                &workspace.id,
                CreateWorkspaceRevisionRequest {
                    base_revision: None,
                    files: vec![file.clone()],
                },
            )
            .unwrap();

        assert_eq!(response.revision.workspace_id, workspace.id);
        assert_eq!(response.revision.files_count, 1);
        assert_eq!(response.revision.total_bytes, content.len() as u64);
        assert_eq!(
            response.workspace.current_revision,
            Some(response.revision.id)
        );
    }

    #[test]
    fn workspace_current_revision_updates_after_revision_creation() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let content = b"fn main() {}";
        let file = file_entry(content);
        state
            .upload_blob(&workspace.id, &file.content_hash, content)
            .unwrap();
        let response = state
            .create_revision(
                &workspace.id,
                CreateWorkspaceRevisionRequest {
                    base_revision: None,
                    files: vec![file],
                },
            )
            .unwrap();

        let updated = state.get_workspace(&workspace.id).unwrap();

        assert_eq!(updated.current_revision, Some(response.revision.id));
        assert_eq!(updated.files_count, 1);
    }

    #[test]
    fn creating_analysis_job_from_workspace_revision_succeeds() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let content = b"fn main() {}";
        let file = file_entry(content);
        state
            .upload_blob(&workspace.id, &file.content_hash, content)
            .unwrap();
        let revision = state
            .create_revision(
                &workspace.id,
                CreateWorkspaceRevisionRequest {
                    base_revision: None,
                    files: vec![file],
                },
            )
            .unwrap()
            .revision;

        let job = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();

        assert_eq!(job.status, AnalysisJobStatus::Queued);
        assert_eq!(job.message.as_deref(), Some("Queued for cloud analysis"));
        assert_eq!(job.progress, Some(0));
        assert_eq!(job.project_name.as_deref(), Some("demo"));
    }

    #[test]
    fn creating_analysis_job_from_missing_revision_fails() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());

        let error = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
                workspace_id: Some(workspace.id),
                revision_id: Some("missing".into()),
                project_name: None,
            })
            .unwrap_err();

        assert_eq!(error, ApiError::NotFound("revision not found".into()));
    }
}

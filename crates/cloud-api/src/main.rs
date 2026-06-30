use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Multipart, Path as AxumPath, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine as _;
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use graph_core::{
    estimate_cloud_analysis_credits, AnalysisJob, AnalysisJobSource, AnalysisJobStatus,
    AnalyzerCapability, AnalyzerEngine, AnalyzerKind, AnalyzerProvider, AnalyzerServiceStatus,
    AnalyzerStatus, AppState, AppStatus, CloudAnalysisUsage, CloudWorkspace,
    CreateAnalysisJobRequest, CreateWorkspaceRevisionRequest, CreateWorkspaceRevisionResponse,
    GraphMode, GraphSnapshot, LanguageId, WorkspaceFileEntry, WorkspaceRevision,
    WorkspaceSyncPlanRequest, WorkspaceSyncPlanResponse,
};
use parking_lot::RwLock;
use project_indexer::ProjectIndex;
use ra_client::{LspRuntime, LspRuntimeConfig, LspRuntimeMode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Read};
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{broadcast, Notify};
use tokio::time::timeout;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use uuid::Uuid;

mod storage;
use storage::{CloudMetadataStore, PersistedCloudState};

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
    #[arg(long, default_value = ".rust-watcher-cloud/workspaces")]
    workspaces_dir: PathBuf,
    #[arg(long, default_value = ".rust-watcher-cloud/cloud-api.sqlite")]
    db_path: PathBuf,
    #[arg(long, default_value = "frontend/dist")]
    frontend_dist: PathBuf,
    #[arg(long, default_value = "rust-analyzer")]
    rust_analyzer: PathBuf,
    #[arg(long, default_value_t = 120)]
    analysis_timeout_seconds: u64,
    #[arg(long, default_value_t = 3)]
    lsp_file_timeout_seconds: u64,
    #[arg(long, default_value_t = 2)]
    max_concurrent_jobs: usize,
    #[arg(long, default_value_t = 100)]
    max_queued_jobs: usize,
    #[arg(long, env = "RUST_WATCHER_DEV_TOKEN", default_value = "dev-token")]
    dev_token: String,
    #[arg(long, env = "RUST_WATCHER_ADMIN_USERNAME", default_value = "admin")]
    admin_username: String,
    #[arg(
        long,
        env = "RUST_WATCHER_ADMIN_PASSWORD",
        default_value = "dev-password"
    )]
    admin_password: String,
    #[arg(long, env = "RUST_WATCHER_MAX_UPLOAD_MB", default_value_t = 200)]
    max_upload_mb: u64,
    #[arg(long, env = "RUST_WATCHER_MAX_FILES", default_value_t = 20_000)]
    max_files: usize,
    #[arg(long, default_value_t = 20)]
    max_file_mb: u64,
}

#[derive(Clone)]
struct CloudApiState {
    jobs: Arc<RwLock<HashMap<String, AnalysisJob>>>,
    job_revision_targets: Arc<RwLock<HashMap<String, JobRevisionTarget>>>,
    workspaces: Arc<RwLock<HashMap<String, CloudWorkspace>>>,
    revisions: Arc<RwLock<HashMap<String, WorkspaceRevision>>>,
    blobs: Arc<RwLock<HashMap<String, StoredBlob>>>,
    analysis_results: Arc<RwLock<HashMap<String, CloudAnalysisResult>>>,
    analysis_usage: Arc<RwLock<HashMap<String, CloudAnalysisUsage>>>,
    blobs_dir: Arc<PathBuf>,
    workspaces_dir: Arc<PathBuf>,
    analysis_config: Arc<CloudAnalysisConfig>,
    limits: Arc<CloudLimits>,
    dev_token: Arc<String>,
    admin_username: Arc<String>,
    admin_password: Arc<String>,
    auth_sessions: Arc<RwLock<HashSet<String>>>,
    agent_sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
    ws_tx: broadcast::Sender<CloudEvent>,
    scheduler: JobScheduler,
    store: Arc<CloudMetadataStore>,
}

#[derive(Debug, Clone)]
struct CloudAnalysisConfig {
    rust_analyzer: PathBuf,
    analysis_timeout_seconds: u64,
    lsp_file_timeout_seconds: u64,
}

#[derive(Debug, Clone)]
struct CloudLimits {
    max_upload_bytes: u64,
    max_unpacked_bytes: u64,
    max_file_count: usize,
    max_file_bytes: u64,
}

#[derive(Debug, Clone)]
struct AgentSession {
    workspace_id: String,
    project_name: String,
    files: HashMap<String, WorkspaceFileEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct JobSchedulerConfig {
    max_concurrent_jobs: usize,
    max_queued_jobs: usize,
}

impl JobSchedulerConfig {
    fn new(max_concurrent_jobs: usize, max_queued_jobs: usize) -> Result<Self> {
        if max_concurrent_jobs == 0 {
            anyhow::bail!("max-concurrent-jobs must be at least 1");
        }
        if max_queued_jobs == 0 {
            anyhow::bail!("max-queued-jobs must be at least 1");
        }
        Ok(Self {
            max_concurrent_jobs,
            max_queued_jobs,
        })
    }
}

impl Default for JobSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_jobs: 2,
            max_queued_jobs: 100,
        }
    }
}

#[derive(Clone)]
struct JobScheduler {
    queue: Arc<RwLock<VecDeque<String>>>,
    running: Arc<RwLock<HashSet<String>>>,
    notify: Arc<Notify>,
    config: JobSchedulerConfig,
}

impl JobScheduler {
    fn new(config: JobSchedulerConfig) -> Self {
        Self {
            queue: Arc::new(RwLock::new(VecDeque::new())),
            running: Arc::new(RwLock::new(HashSet::new())),
            notify: Arc::new(Notify::new()),
            config,
        }
    }

    fn enqueue(&self, job_id: String) -> Result<(), ApiError> {
        {
            let running = self.running.read();
            if running.contains(&job_id) {
                return Ok(());
            }
        }
        let mut queue = self.queue.write();
        if queue.iter().any(|queued_id| queued_id == &job_id) {
            return Ok(());
        }
        if queue.len() >= self.config.max_queued_jobs {
            return Err(ApiError::TooManyRequests("analysis queue is full".into()));
        }
        queue.push_back(job_id);
        drop(queue);
        self.notify.notify_one();
        Ok(())
    }

    fn pop_next(&self) -> Option<String> {
        self.queue.write().pop_front()
    }

    fn mark_running(&self, job_id: String) {
        self.running.write().insert(job_id);
    }

    fn finish_running(&self, job_id: &str) {
        self.running.write().remove(job_id);
    }

    fn is_running(&self, job_id: &str) -> bool {
        self.running.read().contains(job_id)
    }

    fn status(&self) -> AnalysisQueueStatusResponse {
        let queued_job_ids = self.queue.read().iter().cloned().collect::<Vec<_>>();
        let mut running_job_ids = self.running.read().iter().cloned().collect::<Vec<_>>();
        running_job_ids.sort();
        AnalysisQueueStatusResponse {
            queued_jobs: queued_job_ids.len(),
            running_jobs: running_job_ids.len(),
            max_concurrent_jobs: self.config.max_concurrent_jobs,
            max_queued_jobs: self.config.max_queued_jobs,
            queued_job_ids,
            running_job_ids,
        }
    }
}

#[derive(Debug, Clone)]
struct JobRevisionTarget {
    workspace_id: String,
    revision_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudAnalysisResult {
    pub job_id: String,
    pub workspace_id: String,
    pub revision_id: String,
    pub snapshot: GraphSnapshot,
    pub created_at: String,
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
    Unauthorized(String),
    BadRequest(String),
    Conflict(String),
    TooManyRequests(String),
}

impl CloudApiState {
    fn from_persisted(
        blobs_dir: PathBuf,
        workspaces_dir: PathBuf,
        analysis_config: CloudAnalysisConfig,
        limits: CloudLimits,
        dev_token: String,
        admin_username: String,
        admin_password: String,
        store: CloudMetadataStore,
        scheduler_config: JobSchedulerConfig,
        persisted: PersistedCloudState,
    ) -> Result<Self> {
        let mut jobs = persisted.jobs;
        recover_running_jobs(&mut jobs);
        for (id, job) in &jobs {
            let target = persisted.job_revision_targets.get(id);
            store.save_job(
                job,
                target.map(|target| target.workspace_id.as_str()),
                target.map(|target| target.revision_id.as_str()),
            )?;
        }
        let state = Self {
            jobs: Arc::new(RwLock::new(jobs)),
            job_revision_targets: Arc::new(RwLock::new(persisted.job_revision_targets)),
            workspaces: Arc::new(RwLock::new(persisted.workspaces)),
            revisions: Arc::new(RwLock::new(persisted.revisions)),
            blobs: Arc::new(RwLock::new(persisted.blobs)),
            analysis_results: Arc::new(RwLock::new(persisted.analysis_results)),
            analysis_usage: Arc::new(RwLock::new(persisted.analysis_usage)),
            blobs_dir: Arc::new(blobs_dir),
            workspaces_dir: Arc::new(workspaces_dir),
            analysis_config: Arc::new(analysis_config),
            limits: Arc::new(limits),
            dev_token: Arc::new(dev_token),
            admin_username: Arc::new(admin_username),
            admin_password: Arc::new(admin_password),
            auth_sessions: Arc::new(RwLock::new(HashSet::new())),
            agent_sessions: Arc::new(RwLock::new(HashMap::new())),
            ws_tx: broadcast::channel(128).0,
            scheduler: JobScheduler::new(scheduler_config),
            store: Arc::new(store),
        };
        state.requeue_persisted_jobs();
        Ok(state)
    }

    fn create_job(&self, request: CreateAnalysisJobRequest) -> Result<AnalysisJob, ApiError> {
        let mut credits_estimated = None;
        let target = match (&request.workspace_id, &request.revision_id) {
            (Some(workspace_id), Some(revision_id)) => Some(JobRevisionTarget {
                workspace_id: workspace_id.clone(),
                revision_id: revision_id.clone(),
            }),
            _ => None,
        };
        let (source, project_name, message) = match (&request.workspace_id, &request.revision_id) {
            (Some(workspace_id), Some(revision_id)) => {
                let workspace = self
                    .get_workspace(workspace_id)
                    .ok_or_else(|| ApiError::NotFound("workspace not found".into()))?;
                let _revision = self
                    .get_revision(workspace_id, revision_id)
                    .ok_or_else(|| ApiError::NotFound("revision not found".into()))?;
                credits_estimated = Some(estimate_cloud_analysis_credits(
                    _revision.files_count,
                    _revision.total_bytes,
                    &request.requested_analyzers,
                ));
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
            created_at: Some(timestamp()),
            started_at: None,
            finished_at: None,
            credits_estimated,
            credits_used: None,
            error: None,
        };
        self.jobs.write().insert(id, job.clone());
        if let Some(target) = target {
            self.job_revision_targets
                .write()
                .insert(job.id.clone(), target);
        }
        self.persist_job(&job);
        Ok(job)
    }

    fn create_job_for_request(
        &self,
        request: CreateAnalysisJobRequest,
    ) -> Result<AnalysisJob, ApiError> {
        let should_queue = request.workspace_id.is_some() && request.revision_id.is_some();
        if !should_queue {
            let job = self.create_job(request)?;
            self.emit_job_event(&job, "jobStatus", job.message.clone());
            return Ok(job);
        }
        let mut queue = self.scheduler.queue.write();
        if queue.len() >= self.scheduler.config.max_queued_jobs {
            return Err(ApiError::TooManyRequests("analysis queue is full".into()));
        }
        let job = self.create_job(request)?;
        queue.push_back(job.id.clone());
        drop(queue);
        self.emit_job_event(&job, "jobStatus", job.message.clone());
        self.scheduler.notify.notify_one();
        Ok(job)
    }

    fn get_job(&self, id: &str) -> Option<AnalysisJob> {
        self.jobs.read().get(id).cloned()
    }

    fn get_job_revision_target(&self, id: &str) -> Option<JobRevisionTarget> {
        self.job_revision_targets.read().get(id).cloned()
    }

    fn has_valid_revision_target(&self, id: &str) -> bool {
        self.get_job_revision_target(id)
            .and_then(|target| self.get_revision(&target.workspace_id, &target.revision_id))
            .is_some()
    }

    fn enqueue_analysis_job(&self, id: &str) -> Result<(), ApiError> {
        self.scheduler.enqueue(id.to_string())
    }

    fn queue_status(&self) -> AnalysisQueueStatusResponse {
        self.scheduler.status()
    }

    fn requeue_persisted_jobs(&self) {
        let queued_job_ids = self
            .jobs
            .read()
            .values()
            .filter(|job| job.status == AnalysisJobStatus::Queued)
            .filter(|job| self.has_valid_revision_target(&job.id))
            .map(|job| job.id.clone())
            .collect::<Vec<_>>();
        for job_id in queued_job_ids {
            if let Err(error) = self.enqueue_analysis_job(&job_id) {
                warn!(%job_id, ?error, "failed to requeue persisted cloud analysis job");
            }
        }
    }

    fn should_run_dequeued_job(&self, id: &str) -> bool {
        let Some(job) = self.get_job(id) else {
            return false;
        };
        if job.status != AnalysisJobStatus::Queued {
            return false;
        }
        if self.get_job_revision_target(id).is_none() {
            self.fail_job(id, "job is not linked to a workspace revision");
            return false;
        }
        if !self.has_valid_revision_target(id) {
            self.fail_job(id, "workspace revision target not found");
            return false;
        }
        true
    }

    fn update_job_status(
        &self,
        id: &str,
        status: AnalysisJobStatus,
        message: &str,
        progress: Option<u8>,
    ) -> Option<AnalysisJob> {
        let job = {
            let mut jobs = self.jobs.write();
            let job = jobs.get_mut(id)?;
            job.status = status;
            job.message = Some(message.into());
            job.progress = progress;
            if job.started_at.is_none() {
                job.started_at = Some(timestamp());
            }
            job.clone()
        };
        self.persist_job(&job);
        self.emit_job_event(&job, "jobStatus", Some(message.to_string()));
        Some(job)
    }

    fn set_job_analyzer_statuses(
        &self,
        id: &str,
        analyzer_statuses: Vec<AnalyzerServiceStatus>,
    ) -> Option<AnalysisJob> {
        let job = {
            let mut jobs = self.jobs.write();
            let job = jobs.get_mut(id)?;
            job.analyzer_statuses = analyzer_statuses;
            job.clone()
        };
        self.persist_job(&job);
        self.emit_job_event(&job, "snapshotReady", job.message.clone());
        Some(job)
    }

    fn complete_job(
        &self,
        id: &str,
        snapshot: &GraphSnapshot,
        credits_used: u32,
    ) -> Option<AnalysisJob> {
        let job = {
            let mut jobs = self.jobs.write();
            let job = jobs.get_mut(id)?;
            job.status = AnalysisJobStatus::Completed;
            job.message = Some("Cloud analysis completed".into());
            job.progress = Some(100);
            job.finished_at = Some(timestamp());
            job.error = None;
            if job.analyzer_statuses.is_empty() {
                job.analyzer_statuses = parser_analyzer_statuses(snapshot);
            }
            job.credits_used = Some(credits_used);
            job.clone()
        };
        self.persist_job(&job);
        self.emit_job_event(&job, "error", job.error.clone());
        Some(job)
    }

    fn emit_job_event(&self, job: &AnalysisJob, event_type: &str, message: Option<String>) {
        let workspace_id = self
            .get_job_revision_target(&job.id)
            .map(|target| target.workspace_id);
        let _ = self.ws_tx.send(CloudEvent {
            event_type: event_type.into(),
            job_id: Some(job.id.clone()),
            workspace_id,
            status: Some(cloud_status_name(job.status).into()),
            progress: job.progress.map(|progress| f32::from(progress) / 100.0),
            message,
        });
    }

    fn fail_job(&self, id: &str, error: impl Into<String>) -> Option<AnalysisJob> {
        let error = error.into();
        let job = {
            let mut jobs = self.jobs.write();
            let job = jobs.get_mut(id)?;
            if requests_rust_analyzer(job) {
                job.analyzer_statuses = vec![rust_analyzer_status(
                    AnalyzerStatus::Error,
                    Some(error.clone()),
                    0,
                    None,
                )];
            }
            job.status = AnalysisJobStatus::Failed;
            job.message = Some("Cloud analysis failed".into());
            job.progress = None;
            job.finished_at = Some(timestamp());
            job.error = Some(error);
            job.clone()
        };
        self.persist_job(&job);
        Some(job)
    }

    fn persist_job(&self, job: &AnalysisJob) {
        let target = self.job_revision_targets.read().get(&job.id).cloned();
        if let Err(error) = self.store.save_job(
            job,
            target.as_ref().map(|target| target.workspace_id.as_str()),
            target.as_ref().map(|target| target.revision_id.as_str()),
        ) {
            warn!(job_id = %job.id, %error, "failed to persist cloud analysis job");
        }
    }

    fn usage_summary(&self) -> UsageSummaryResponse {
        let usage_records = self.analysis_usage.read();
        let jobs = self.jobs.read();
        UsageSummaryResponse {
            jobs_count: jobs.len() as u32,
            completed_jobs: jobs
                .values()
                .filter(|job| job.status == AnalysisJobStatus::Completed)
                .count() as u32,
            failed_jobs: jobs
                .values()
                .filter(|job| job.status == AnalysisJobStatus::Failed)
                .count() as u32,
            total_input_files: usage_records
                .values()
                .map(|usage| u64::from(usage.input_files))
                .sum(),
            total_input_bytes: usage_records.values().map(|usage| usage.input_bytes).sum(),
            total_wall_ms: usage_records
                .values()
                .map(|usage| usage.total_wall_ms)
                .sum(),
            total_credits_used: usage_records
                .values()
                .map(|usage| u64::from(usage.credits_used))
                .sum(),
        }
    }

    fn list_jobs(&self) -> Vec<AnalysisJob> {
        let mut jobs = self.jobs.read().values().cloned().collect::<Vec<_>>();
        jobs.sort_by(|left, right| right.id.cmp(&left.id));
        jobs
    }

    fn cancel_job(&self, id: &str) -> Result<Option<AnalysisJob>, ApiError> {
        if self.scheduler.is_running(id) {
            return Err(ApiError::Conflict(
                "running job cancellation is not supported yet".into(),
            ));
        }
        let job = {
            let mut jobs = self.jobs.write();
            let Some(job) = jobs.get_mut(id) else {
                return Ok(None);
            };
            if !is_terminal(job.status) {
                job.status = AnalysisJobStatus::Cancelled;
                job.message = Some("Cancelled".into());
                job.finished_at = Some(timestamp());
            }
            job.clone()
        };
        self.persist_job(&job);
        Ok(Some(job))
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
        if let Err(error) = self.store.save_workspace(&workspace) {
            warn!(workspace_id = %workspace.id, %error, "failed to persist cloud workspace");
        }
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
        if let Err(error) = self.store.save_blob(&blob) {
            warn!(content_hash = %blob.content_hash, %error, "failed to persist cloud blob metadata");
        }
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
#[serde(rename_all = "camelCase")]
struct AnalysisQueueStatusResponse {
    queued_jobs: usize,
    running_jobs: usize,
    max_concurrent_jobs: usize,
    max_queued_jobs: usize,
    queued_job_ids: Vec<String>,
    running_job_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSummaryResponse {
    pub jobs_count: u32,
    pub completed_jobs: u32,
    pub failed_jobs: u32,
    pub total_input_files: u64,
    pub total_input_bytes: u64,
    pub total_wall_ms: u64,
    pub total_credits_used: u64,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudHealthResponse {
    ok: bool,
    version: &'static str,
    mode: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudLoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudLoginResponse {
    session_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudMeResponse {
    authenticated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubImportRequest {
    url: String,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudStartResponse {
    workspace_id: String,
    job_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudJobResponse {
    job_id: String,
    workspace_id: Option<String>,
    status: AnalysisJobStatus,
    message: Option<String>,
    progress: Option<f32>,
    created_at: Option<String>,
    updated_at: Option<String>,
    credits_estimated: Option<u32>,
    credits_used: Option<u32>,
    analyzers: Vec<CloudJobAnalyzerResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudJobAnalyzerResponse {
    kind: String,
    status: AnalyzerStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudWorkspaceStatusResponse {
    workspace_id: String,
    name: String,
    source: CloudWorkspaceSourceResponse,
    status: String,
    file_count: u32,
    last_job_id: Option<String>,
    last_updated: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudWorkspaceListResponse {
    workspaces: Vec<CloudWorkspaceStatusResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudWorkspaceSourceResponse {
    #[serde(rename = "type")]
    source_type: String,
    url: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudUsageResponse {
    credits_remaining: u32,
    credits_used: u32,
    jobs: Vec<CloudUsageJobResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudUsageJobResponse {
    job_id: String,
    workspace_id: Option<String>,
    credits: u32,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSessionRequest {
    project_name: String,
    token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentSessionResponse {
    session_id: String,
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentFilesRequest {
    files: Vec<AgentFileRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentFileRequest {
    path: String,
    language: Option<LanguageId>,
    content_base64: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    mode: Option<GraphMode>,
}

#[derive(Debug)]
struct CollectedWorkspaceFile {
    entry: WorkspaceFileEntry,
    bytes: Vec<u8>,
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
    let scheduler_config = JobSchedulerConfig::new(args.max_concurrent_jobs, args.max_queued_jobs)?;
    std::fs::create_dir_all(&args.blobs_dir)
        .with_context(|| format!("failed to create {}", args.blobs_dir.display()))?;
    std::fs::create_dir_all(&args.workspaces_dir)
        .with_context(|| format!("failed to create {}", args.workspaces_dir.display()))?;
    let store = CloudMetadataStore::open(args.db_path.clone())?;
    store.init_schema()?;
    let persisted = store.load_all()?;
    let state = CloudApiState::from_persisted(
        args.blobs_dir.clone(),
        args.workspaces_dir.clone(),
        CloudAnalysisConfig {
            rust_analyzer: args.rust_analyzer.clone(),
            analysis_timeout_seconds: args.analysis_timeout_seconds,
            lsp_file_timeout_seconds: args.lsp_file_timeout_seconds,
        },
        CloudLimits {
            max_upload_bytes: args.max_upload_mb.saturating_mul(1024 * 1024),
            max_unpacked_bytes: args
                .max_upload_mb
                .saturating_mul(1024 * 1024)
                .saturating_mul(2),
            max_file_count: args.max_files,
            max_file_bytes: args.max_file_mb.saturating_mul(1024 * 1024),
        },
        args.dev_token.clone(),
        args.admin_username.clone(),
        args.admin_password.clone(),
        store,
        scheduler_config,
        persisted,
    )?;
    start_analysis_workers(state.clone());
    let app = Router::new()
        .route("/api/cloud/health", get(cloud_health))
        .route("/api/cloud/auth/login", post(cloud_login))
        .route("/api/cloud/auth/me", get(cloud_me))
        .route("/api/cloud/import/github", post(cloud_import_github))
        .route("/api/cloud/upload", post(cloud_upload_zip))
        .route("/api/cloud/jobs/{id}", get(cloud_get_job))
        .route("/api/cloud/workspaces", get(cloud_list_workspaces))
        .route(
            "/api/cloud/workspaces/{id}/status",
            get(cloud_workspace_status),
        )
        .route(
            "/api/cloud/workspaces/{id}/snapshot",
            get(cloud_workspace_snapshot),
        )
        .route("/api/cloud/usage", get(cloud_usage))
        .route("/api/cloud/ws", get(cloud_ws_handler))
        .route("/api/cloud/agent/sessions", post(agent_create_session))
        .route(
            "/api/cloud/agent/sessions/{id}/files",
            post(agent_upload_files),
        )
        .route(
            "/api/cloud/agent/sessions/{id}/changes",
            post(agent_upload_files),
        )
        .route(
            "/api/cloud/agent/sessions/{id}/analyze",
            post(agent_analyze_session),
        )
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
        .route("/api/analysis/jobs/{id}/snapshot", get(get_job_snapshot))
        .route("/api/analysis/jobs/{id}/usage", get(get_job_usage))
        .route("/api/analysis/jobs/{id}/cancel", post(cancel_job))
        .route("/api/analysis/queue", get(get_analysis_queue))
        .route("/api/usage/summary", get(usage_summary))
        .fallback_service(
            ServeDir::new(&args.frontend_dist)
                .not_found_service(ServeFile::new(args.frontend_dist.join("index.html"))),
        )
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

async fn cloud_health() -> Json<CloudHealthResponse> {
    Json(CloudHealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        mode: "cloud",
    })
}

async fn cloud_login(
    State(state): State<CloudApiState>,
    Json(request): Json<CloudLoginRequest>,
) -> impl IntoResponse {
    if request.username != *state.admin_username || request.password != *state.admin_password {
        return (StatusCode::UNAUTHORIZED, "invalid username or password").into_response();
    }
    let session_token = Uuid::new_v4().to_string();
    state.auth_sessions.write().insert(session_token.clone());
    Json(CloudLoginResponse { session_token }).into_response()
}

async fn cloud_me(State(state): State<CloudApiState>, headers: HeaderMap) -> impl IntoResponse {
    match require_cloud_auth(&state, &headers) {
        Ok(()) => Json(CloudMeResponse {
            authenticated: true,
        })
        .into_response(),
        Err(error) => error.into_response(),
    }
}

async fn cloud_import_github(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    Json(request): Json<GithubImportRequest>,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    match import_github_workspace(&state, request).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn cloud_upload_zip(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    match upload_zip_workspace(&state, &mut multipart).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn cloud_get_job(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    match state.get_job(&id) {
        Some(job) => Json(cloud_job_response(&state, job)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn cloud_list_workspaces(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    let mut workspaces = state
        .workspaces
        .read()
        .values()
        .cloned()
        .map(|workspace| cloud_workspace_status_response(&state, workspace))
        .collect::<Vec<_>>();
    workspaces.sort_by(|left, right| {
        right
            .last_updated
            .cmp(&left.last_updated)
            .then_with(|| left.name.cmp(&right.name))
    });
    Json(CloudWorkspaceListResponse { workspaces }).into_response()
}

async fn cloud_workspace_status(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    match state.get_workspace(&id) {
        Some(workspace) => Json(cloud_workspace_status_response(&state, workspace)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn cloud_workspace_snapshot(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SnapshotQuery>,
) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    let Some(snapshot) = latest_workspace_snapshot(&state, &id) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let snapshot = query
        .mode
        .map(|mode| graph_builder::filter_snapshot(&snapshot, mode))
        .unwrap_or(snapshot);
    Json(snapshot).into_response()
}

async fn cloud_usage(State(state): State<CloudApiState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(error) = require_cloud_auth(&state, &headers) {
        return error.into_response();
    }
    Json(cloud_usage_response(&state)).into_response()
}

async fn cloud_ws_handler(
    State(state): State<CloudApiState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| cloud_websocket(socket, state))
}

async fn agent_create_session(
    State(state): State<CloudApiState>,
    Json(request): Json<AgentSessionRequest>,
) -> impl IntoResponse {
    if request.token != *state.dev_token {
        return (StatusCode::UNAUTHORIZED, "invalid agent token").into_response();
    }
    let workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: request.project_name.clone(),
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

async fn agent_upload_files(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<AgentFilesRequest>,
) -> impl IntoResponse {
    match store_agent_files(&state, &id, request) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(error) => error.into_response(),
    }
}

async fn agent_analyze_session(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match analyze_agent_session(&state, &id) {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn create_job(
    State(state): State<CloudApiState>,
    Json(request): Json<CreateAnalysisJobRequest>,
) -> impl IntoResponse {
    match state.create_job_for_request(request) {
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

async fn get_job_snapshot(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Some(result) = state.analysis_results.read().get(&id).cloned() {
        return (StatusCode::OK, Json(result.snapshot)).into_response();
    }
    match state.get_job(&id) {
        Some(job) if job.status == AnalysisJobStatus::Failed => (
            StatusCode::CONFLICT,
            job.error
                .or(job.message)
                .unwrap_or_else(|| "analysis failed".into()),
        )
            .into_response(),
        Some(_) => StatusCode::ACCEPTED.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_job_usage(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Some(usage) = state.analysis_usage.read().get(&id).cloned() {
        return (StatusCode::OK, Json(usage)).into_response();
    }
    match state.get_job(&id) {
        Some(job) if job.status == AnalysisJobStatus::Failed => (
            StatusCode::CONFLICT,
            job.error
                .or(job.message)
                .unwrap_or_else(|| "analysis failed".into()),
        )
            .into_response(),
        Some(_) => StatusCode::ACCEPTED.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn usage_summary(State(state): State<CloudApiState>) -> Json<UsageSummaryResponse> {
    Json(state.usage_summary())
}

async fn get_analysis_queue(
    State(state): State<CloudApiState>,
) -> Json<AnalysisQueueStatusResponse> {
    Json(state.queue_status())
}

async fn cancel_job(
    State(state): State<CloudApiState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.cancel_job(&id) {
        Ok(Some(job)) => Json(job).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error.into_response(),
    }
}

fn is_terminal(status: AnalysisJobStatus) -> bool {
    matches!(
        status,
        AnalysisJobStatus::Completed | AnalysisJobStatus::Failed | AnalysisJobStatus::Cancelled
    )
}

fn is_running_status(status: AnalysisJobStatus) -> bool {
    matches!(
        status,
        AnalysisJobStatus::Preparing
            | AnalysisJobStatus::Indexing
            | AnalysisJobStatus::RunningAnalyzers
            | AnalysisJobStatus::BuildingGraph
    )
}

fn cloud_status_name(status: AnalysisJobStatus) -> &'static str {
    match status {
        AnalysisJobStatus::Queued => "queued",
        AnalysisJobStatus::Preparing => "importing",
        AnalysisJobStatus::Indexing => "indexing",
        AnalysisJobStatus::RunningAnalyzers => "analyzing",
        AnalysisJobStatus::BuildingGraph => "analyzing",
        AnalysisJobStatus::Completed => "completed",
        AnalysisJobStatus::Failed => "failed",
        AnalysisJobStatus::Cancelled => "cancelled",
    }
}

fn require_cloud_auth(state: &CloudApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".into()))?;
    if state.auth_sessions.read().contains(token) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("invalid or expired session".into()))
    }
}

fn recover_running_jobs(jobs: &mut HashMap<String, AnalysisJob>) {
    for job in jobs.values_mut() {
        if is_running_status(job.status) {
            job.status = AnalysisJobStatus::Failed;
            job.message = Some("Cloud analysis failed".into());
            job.progress = None;
            job.finished_at = Some(timestamp());
            job.error = Some("cloud-api restarted while job was running".into());
        }
    }
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

async fn import_github_workspace(
    state: &CloudApiState,
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

async fn upload_zip_workspace(
    state: &CloudApiState,
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

fn import_project_directory(
    state: &CloudApiState,
    root: &Path,
    display_name: &str,
    source: Option<AnalysisJobSource>,
) -> Result<CloudStartResponse, ApiError> {
    let workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: display_name.to_string(),
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

fn store_workspace_files(
    state: &CloudApiState,
    workspace_id: &str,
    files: &[CollectedWorkspaceFile],
) -> Result<(), ApiError> {
    for file in files {
        state.upload_blob(workspace_id, &file.entry.content_hash, &file.bytes)?;
    }
    Ok(())
}

fn collect_workspace_files(
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

fn collect_workspace_files_inner(
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

fn store_agent_files(
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

fn analyze_agent_session(
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
    })?;
    let session_token = Uuid::new_v4().to_string();
    state.auth_sessions.write().insert(session_token.clone());
    Ok(CloudStartResponse {
        workspace_id: revision_response.workspace.id,
        job_id: job.id,
        status: "queued".into(),
        session_token: Some(session_token),
    })
}

fn cloud_job_response(state: &CloudApiState, job: AnalysisJob) -> CloudJobResponse {
    let workspace_id = state
        .get_job_revision_target(&job.id)
        .map(|target| target.workspace_id);
    let updated_at = job.finished_at.clone().or_else(|| job.started_at.clone());
    CloudJobResponse {
        job_id: job.id,
        workspace_id,
        status: job.status,
        message: job.error.clone().or(job.message),
        progress: job.progress.map(|progress| f32::from(progress) / 100.0),
        created_at: job.created_at,
        updated_at,
        credits_estimated: job.credits_estimated,
        credits_used: job.credits_used,
        analyzers: job
            .analyzer_statuses
            .into_iter()
            .map(|status| CloudJobAnalyzerResponse {
                kind: status.id,
                status: status.status,
            })
            .collect(),
    }
}

fn cloud_workspace_status_response(
    state: &CloudApiState,
    workspace: CloudWorkspace,
) -> CloudWorkspaceStatusResponse {
    let last_job = state
        .jobs
        .read()
        .values()
        .filter(|job| {
            state
                .get_job_revision_target(&job.id)
                .is_some_and(|target| target.workspace_id == workspace.id)
        })
        .max_by(|left, right| left.created_at.cmp(&right.created_at))
        .cloned();
    let status = last_job
        .as_ref()
        .map(|job| match job.status {
            AnalysisJobStatus::Completed => "ready",
            AnalysisJobStatus::Failed => "failed",
            AnalysisJobStatus::Cancelled => "cancelled",
            _ => "analyzing",
        })
        .unwrap_or("created")
        .to_string();
    let source = workspace.source.clone();
    CloudWorkspaceStatusResponse {
        workspace_id: workspace.id,
        name: workspace.display_name,
        source: CloudWorkspaceSourceResponse {
            source_type: source
                .as_ref()
                .map(|source| match source.kind {
                    graph_core::AnalysisJobSourceKind::GitRepository => "github",
                    graph_core::AnalysisJobSourceKind::UploadedArchive => "zip",
                    graph_core::AnalysisJobSourceKind::LocalPath => "agent",
                })
                .unwrap_or("unknown")
                .into(),
            url: source
                .as_ref()
                .and_then(|source| source.repository_url.clone()),
            git_ref: source.and_then(|source| source.git_ref),
        },
        status,
        file_count: workspace.files_count,
        last_job_id: last_job.map(|job| job.id),
        last_updated: workspace.updated_at,
    }
}

fn latest_workspace_snapshot(state: &CloudApiState, workspace_id: &str) -> Option<GraphSnapshot> {
    let revisions = state.revisions.read();
    let results = state.analysis_results.read();
    results
        .values()
        .filter(|result| result.workspace_id == workspace_id)
        .max_by_key(|result| {
            revisions
                .get(&result.revision_id)
                .and_then(|revision| revision.created_at.clone())
                .unwrap_or_else(|| result.created_at.clone())
        })
        .map(|result| result.snapshot.clone())
}

fn cloud_usage_response(state: &CloudApiState) -> CloudUsageResponse {
    let jobs = state.jobs.read();
    let mut usage = state
        .analysis_usage
        .read()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    usage.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    let credits_used = usage
        .iter()
        .map(|usage| usage.credits_used)
        .fold(0u32, u32::saturating_add);
    CloudUsageResponse {
        credits_remaining: 1000u32.saturating_sub(credits_used),
        credits_used,
        jobs: usage
            .into_iter()
            .map(|usage| {
                let job = jobs.get(&usage.job_id);
                CloudUsageJobResponse {
                    job_id: usage.job_id,
                    workspace_id: usage.workspace_id,
                    credits: usage.credits_used,
                    reason: format!(
                        "{} files, {} nodes, {} edges{}",
                        usage.input_files,
                        usage.output_nodes,
                        usage.output_edges,
                        job.and_then(|job| job.project_name.as_ref())
                            .map(|name| format!(" · {name}"))
                            .unwrap_or_default()
                    ),
                }
            })
            .collect(),
    }
}

async fn cloud_websocket(socket: WebSocket, state: CloudApiState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.ws_tx.subscribe();
    let forward = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&event) {
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });
    while let Some(Ok(message)) = receiver.next().await {
        if matches!(message, Message::Close(_)) {
            break;
        }
    }
    forward.abort();
}

#[derive(Debug)]
struct ParsedGithubUrl {
    repo: String,
    web_url: String,
    clone_url: String,
    git_ref: Option<String>,
}

fn parse_github_url(input: &str) -> Result<ParsedGithubUrl, ApiError> {
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

fn clean_github_component(value: &str, label: &str) -> Result<String, ApiError> {
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

fn validate_git_ref(value: &str) -> Result<(), ApiError> {
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

fn unpack_zip_bytes(
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

fn zip_display_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("uploaded-project")
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
        .collect::<String>()
}

fn validate_relative_path(path: &str) -> Result<String, ApiError> {
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

fn should_ignore_cloud_path(path: &Path) -> bool {
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

fn is_sync_file_path(path: &Path) -> bool {
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

fn language_for_path(path: &Path) -> Option<LanguageId> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => Some(LanguageId::Rust),
        Some("py") => Some(LanguageId::Python),
        Some("ts" | "tsx") => Some(LanguageId::TypeScript),
        Some("js" | "jsx") => Some(LanguageId::JavaScript),
        Some("qml") => Some(LanguageId::Qml),
        _ => None,
    }
}

fn requested_analyzers_for_collected_files(
    files: &[CollectedWorkspaceFile],
) -> Vec<AnalyzerEngine> {
    let entries = files
        .iter()
        .map(|file| file.entry.clone())
        .collect::<Vec<_>>();
    requested_analyzers_for_workspace_files(&entries)
}

fn requested_analyzers_for_workspace_files(files: &[WorkspaceFileEntry]) -> Vec<AnalyzerEngine> {
    if files.iter().any(|file| file.path == "Cargo.toml") {
        vec![AnalyzerEngine::RustAnalyzer]
    } else {
        Vec::new()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::NotFound(message) => (StatusCode::NOT_FOUND, message).into_response(),
            Self::Unauthorized(message) => (StatusCode::UNAUTHORIZED, message).into_response(),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            Self::Conflict(message) => (StatusCode::CONFLICT, message).into_response(),
            Self::TooManyRequests(message) => {
                (StatusCode::TOO_MANY_REQUESTS, message).into_response()
            }
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

fn materialize_revision(
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

fn materialized_child_path(root: &Path, relative_path: &str) -> Result<PathBuf> {
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

fn start_analysis_workers(state: CloudApiState) {
    for worker_index in 0..state.scheduler.config.max_concurrent_jobs {
        let worker_state = state.clone();
        tokio::spawn(async move {
            analysis_worker_loop(worker_state, worker_index).await;
        });
    }
}

async fn analysis_worker_loop(state: CloudApiState, worker_index: usize) {
    loop {
        let notified = state.scheduler.notify.notified();
        if run_one_queued_job(state.clone()).await {
            continue;
        }
        tracing::debug!(worker_index, "cloud analysis worker waiting for queued job");
        notified.await;
    }
}

async fn run_one_queued_job(state: CloudApiState) -> bool {
    let Some(job_id) = state.scheduler.pop_next() else {
        return false;
    };
    if !state.should_run_dequeued_job(&job_id) {
        return true;
    }
    state.scheduler.mark_running(job_id.clone());
    run_parser_cloud_analysis(state.clone(), job_id.clone()).await;
    state.scheduler.finish_running(&job_id);
    true
}

async fn run_parser_cloud_analysis(state: CloudApiState, job_id: String) {
    let timeout_seconds = state.analysis_config.analysis_timeout_seconds;
    let result = timeout(
        Duration::from_secs(timeout_seconds),
        execute_cloud_analysis_job(state.clone(), job_id.clone()),
    )
    .await;
    match result {
        Ok(Ok(_snapshot)) => {}
        Ok(Err(error)) => {
            state.fail_job(&job_id, error.to_string());
        }
        Err(_) => {
            state.fail_job(
                &job_id,
                format!("cloud analysis timed out after {timeout_seconds}s"),
            );
        }
    }
}

async fn execute_cloud_analysis_job(state: CloudApiState, job_id: String) -> Result<GraphSnapshot> {
    let total_start = Instant::now();
    let target = state
        .get_job_revision_target(&job_id)
        .ok_or_else(|| anyhow::anyhow!("job is not linked to a workspace revision"))?;
    let revision = state
        .get_revision(&target.workspace_id, &target.revision_id)
        .ok_or_else(|| anyhow::anyhow!("revision not found"))?;
    let requested_analyzers = state
        .get_job(&job_id)
        .map(|job| job.requested_analyzers)
        .unwrap_or_default();
    let credits_estimated = estimate_cloud_analysis_credits(
        revision.files_count,
        revision.total_bytes,
        &requested_analyzers,
    );
    state.update_job_status(
        &job_id,
        AnalysisJobStatus::Preparing,
        "Preparing cloud analysis",
        Some(10),
    );
    let materialization_start = Instant::now();
    let project_root = materialize_revision(&state, &target.workspace_id, &target.revision_id)?;
    let materialization_ms = elapsed_ms(materialization_start.elapsed());

    state.update_job_status(
        &job_id,
        AnalysisJobStatus::Indexing,
        "Indexing workspace revision",
        Some(35),
    );
    let graph_build_start = Instant::now();
    let (mut snapshot, project_index) = build_initial_snapshot(&project_root);
    let rust_analyzer_requested = requested_analyzers.contains(&AnalyzerEngine::RustAnalyzer);
    let mut rust_analyzer_final_status = None;
    if rust_analyzer_requested {
        state.update_job_status(
            &job_id,
            AnalysisJobStatus::RunningAnalyzers,
            "Running cloud analyzers",
            Some(60),
        );
        state.set_job_analyzer_statuses(
            &job_id,
            cloud_analyzer_statuses(
                &snapshot,
                Some(rust_analyzer_status(
                    AnalyzerStatus::Starting,
                    Some("Starting cloud rust-analyzer".into()),
                    0,
                    None,
                )),
            ),
        );
        if let Some(project_index) = project_index.as_ref() {
            match enrich_with_cloud_rust_analyzer(&state, &job_id, &mut snapshot, project_index)
                .await
            {
                Ok(()) => {
                    rust_analyzer_final_status = Some(rust_analyzer_status(
                        AnalyzerStatus::Ready,
                        Some("Cloud rust-analyzer completed".into()),
                        rust_file_count(Some(project_index)),
                        Some(credits_estimated),
                    ));
                }
                Err(error) => {
                    warn!(job_id = %job_id, %error, "cloud rust-analyzer failed; keeping parser graph");
                    rust_analyzer_final_status = Some(rust_analyzer_status(
                        AnalyzerStatus::Error,
                        Some(format!("rust-analyzer failed: {error}")),
                        0,
                        None,
                    ));
                }
            }
        } else {
            rust_analyzer_final_status = Some(rust_analyzer_status(
                AnalyzerStatus::Fallback,
                Some(
                    "rust-analyzer skipped: uploaded files are not a standalone Cargo project"
                        .into(),
                ),
                0,
                None,
            ));
        }
    }
    let graph_build_ms = elapsed_ms(graph_build_start.elapsed());

    state.update_job_status(
        &job_id,
        AnalysisJobStatus::BuildingGraph,
        "Building graph snapshot",
        Some(80),
    );
    snapshot.status.app_state = AppState::Normal;
    snapshot.status.analyzer_status = AnalyzerStatus::Ready;
    snapshot.status.analyzers = if rust_analyzer_requested {
        cloud_analyzer_statuses(&snapshot, rust_analyzer_final_status.clone())
    } else {
        parser_analyzer_statuses(&snapshot)
    };
    snapshot.status.message = Some(if rust_analyzer_requested {
        match rust_analyzer_final_status
            .as_ref()
            .map(|status| status.status)
            .unwrap_or(AnalyzerStatus::Fallback)
        {
            AnalyzerStatus::Ready => "Cloud rust-analyzer analysis completed".into(),
            _ => "Cloud parser analysis completed; rust-analyzer used fallback".into(),
        }
    } else {
        "Cloud parser analysis completed".into()
    });
    snapshot.status.progress = Some(100);
    snapshot.status.last_updated = Some(timestamp());

    let credits_used = if rust_analyzer_requested
        && rust_analyzer_final_status
            .as_ref()
            .is_some_and(|status| status.status == AnalyzerStatus::Ready)
    {
        credits_estimated
    } else {
        estimate_cloud_analysis_credits(revision.files_count, revision.total_bytes, &[])
    };
    let usage = CloudAnalysisUsage {
        job_id: job_id.clone(),
        workspace_id: Some(target.workspace_id.clone()),
        revision_id: Some(target.revision_id.clone()),
        input_files: revision.files_count,
        input_bytes: revision.total_bytes,
        output_nodes: snapshot.nodes.len() as u32,
        output_edges: snapshot.edges.len() as u32,
        output_files: snapshot.files.len() as u32,
        requested_analyzers,
        materialization_ms,
        graph_build_ms,
        total_wall_ms: elapsed_ms(total_start.elapsed()),
        credits_estimated,
        credits_used,
        created_at: Some(timestamp()),
    };
    let result = CloudAnalysisResult {
        job_id: job_id.clone(),
        workspace_id: target.workspace_id,
        revision_id: target.revision_id,
        snapshot: snapshot.clone(),
        created_at: timestamp(),
    };
    state
        .analysis_usage
        .write()
        .insert(job_id.clone(), usage.clone());
    state
        .analysis_results
        .write()
        .insert(job_id.clone(), result.clone());
    if let Err(error) = state.store.save_usage(&usage) {
        warn!(job_id = %job_id, %error, "failed to persist cloud analysis usage");
    }
    if let Err(error) = state.store.save_analysis_result(&result) {
        warn!(job_id = %job_id, %error, "failed to persist cloud analysis result");
    }
    state.set_job_analyzer_statuses(&job_id, snapshot.status.analyzers.clone());
    state.complete_job(&job_id, &snapshot, credits_used);
    Ok(snapshot)
}

fn build_initial_snapshot(project_root: &Path) -> (GraphSnapshot, Option<ProjectIndex>) {
    let status = AppStatus {
        app_state: AppState::Indexing,
        analyzer_status: AnalyzerStatus::Indexing,
        analyzers: Vec::new(),
        python_analyzer: None,
        project_name: None,
        project_path: Some(project_root.display().to_string()),
        last_updated: Some(timestamp()),
        message: Some("Building parser graph".into()),
        progress: Some(35),
    };
    if project_root.join("Cargo.toml").is_file() {
        if let Ok(index) = project_indexer::index_project(project_root) {
            let snapshot = graph_builder::build_fallback_graph(&index, status);
            return (snapshot, Some(index));
        }
    }
    (
        graph_builder::build_language_graph(project_root, status),
        None,
    )
}

async fn enrich_with_cloud_rust_analyzer(
    state: &CloudApiState,
    job_id: &str,
    snapshot: &mut GraphSnapshot,
    index: &ProjectIndex,
) -> Result<()> {
    let runtime = LspRuntime::new(LspRuntimeConfig {
        analyzer_id: "rust-analyzer",
        process_name: "rust-analyzer",
        default_language_id: "rust",
        binary: state.analysis_config.rust_analyzer.clone(),
        args: Vec::new(),
        mode: LspRuntimeMode::Required,
        fallback_message: "rust-analyzer unavailable in cloud worker.",
        resolver: cloud_binary_resolver,
        root: index.root.clone(),
    });
    let rust_files = index
        .files
        .iter()
        .filter(|file| file.absolute_path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .collect::<Vec<_>>();
    state.set_job_analyzer_statuses(
        job_id,
        cloud_analyzer_statuses(
            snapshot,
            Some(rust_analyzer_status(
                AnalyzerStatus::Indexing,
                Some(format!("Indexing {} Rust files", rust_files.len())),
                0,
                None,
            )),
        ),
    );

    let mut enriched_files = 0u32;
    let mut warnings = Vec::new();
    for file in rust_files {
        let symbols = match timeout(
            Duration::from_secs(state.analysis_config.lsp_file_timeout_seconds),
            runtime.document_symbols(&file.absolute_path, Some("rust")),
        )
        .await
        {
            Ok(Ok(symbols)) => symbols,
            Ok(Err(error)) if runtime.status() == ra_client::LspRuntimeStatus::Error => {
                anyhow::bail!("rust-analyzer unavailable in cloud worker: {error}");
            }
            Ok(Err(error)) => {
                warnings.push(format!("{}: {error}", file.relative_path));
                continue;
            }
            Err(_) => {
                warnings.push(format!("{}: rust-analyzer timed out", file.relative_path));
                continue;
            }
        };
        graph_builder::enrich_file_symbols(snapshot, file, &symbols);
        enriched_files += 1;
    }

    let message = if warnings.is_empty() {
        "Cloud rust-analyzer completed".to_string()
    } else {
        format!(
            "Cloud rust-analyzer completed with {} file warnings",
            warnings.len()
        )
    };
    state.set_job_analyzer_statuses(
        job_id,
        cloud_analyzer_statuses(
            snapshot,
            Some(rust_analyzer_status(
                AnalyzerStatus::Ready,
                Some(message),
                enriched_files,
                None,
            )),
        ),
    );
    Ok(())
}

fn cloud_binary_resolver(configured: &Path, _root: &Path) -> PathBuf {
    configured.to_path_buf()
}

fn requests_rust_analyzer(job: &AnalysisJob) -> bool {
    job.requested_analyzers
        .contains(&AnalyzerEngine::RustAnalyzer)
}

fn rust_file_count(index: Option<&ProjectIndex>) -> u32 {
    index
        .map(|index| {
            index
                .files
                .iter()
                .filter(|file| {
                    file.absolute_path.extension().and_then(|ext| ext.to_str()) == Some("rs")
                })
                .count() as u32
        })
        .unwrap_or_default()
}

fn parser_analyzer_statuses(snapshot: &GraphSnapshot) -> Vec<AnalyzerServiceStatus> {
    cloud_analyzer_statuses(snapshot, None)
}

fn cloud_analyzer_statuses(
    snapshot: &GraphSnapshot,
    rust_analyzer: Option<AnalyzerServiceStatus>,
) -> Vec<AnalyzerServiceStatus> {
    let mut statuses = vec![AnalyzerServiceStatus {
        id: "cloud-parser".into(),
        kind: AnalyzerKind::Other,
        engine: AnalyzerEngine::Parser,
        label: "Cloud parser graph".into(),
        status: AnalyzerStatus::Ready,
        mode: Some("parser".into()),
        message: Some("Parser-only cloud analysis".into()),
        capabilities: vec![AnalyzerCapability::Symbols],
        files_indexed: snapshot.files.len() as u32,
        last_updated: Some(timestamp()),
        provider: AnalyzerProvider::Cloud,
        billable: false,
        credits_used: None,
    }];
    if let Some(rust_analyzer) = rust_analyzer {
        statuses.push(rust_analyzer);
    }
    statuses
}

fn rust_analyzer_status(
    status: AnalyzerStatus,
    message: Option<String>,
    files_indexed: u32,
    credits_used: Option<u32>,
) -> AnalyzerServiceStatus {
    AnalyzerServiceStatus {
        id: "rust-analyzer".into(),
        kind: AnalyzerKind::Rust,
        engine: AnalyzerEngine::RustAnalyzer,
        label: "rust-analyzer".into(),
        status,
        mode: Some("cloud".into()),
        message,
        capabilities: vec![AnalyzerCapability::Symbols],
        files_indexed,
        last_updated: Some(timestamp()),
        provider: AnalyzerProvider::Cloud,
        billable: true,
        credits_used,
    }
}

fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
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
        test_state_with_config(test_analysis_config())
    }

    fn test_state_with_config(analysis_config: CloudAnalysisConfig) -> CloudApiState {
        test_state_with_config_and_scheduler_config(analysis_config, JobSchedulerConfig::default())
    }

    fn test_state_with_scheduler_config(scheduler_config: JobSchedulerConfig) -> CloudApiState {
        test_state_with_config_and_scheduler_config(test_analysis_config(), scheduler_config)
    }

    fn test_state_with_config_and_scheduler_config(
        analysis_config: CloudAnalysisConfig,
        scheduler_config: JobSchedulerConfig,
    ) -> CloudApiState {
        let root = std::env::temp_dir().join(format!("rust-watcher-cloud-api-{}", Uuid::new_v4()));
        let blobs_dir = root.join("blobs");
        let workspaces_dir = root.join("workspaces");
        std::fs::create_dir_all(&blobs_dir).unwrap();
        std::fs::create_dir_all(&workspaces_dir).unwrap();
        let store = CloudMetadataStore::open(root.join("cloud-api.sqlite")).unwrap();
        store.init_schema().unwrap();
        CloudApiState::from_persisted(
            blobs_dir,
            workspaces_dir,
            analysis_config,
            test_cloud_limits(),
            "dev-token".into(),
            "admin".into(),
            "dev-password".into(),
            store,
            scheduler_config,
            PersistedCloudState::default(),
        )
        .unwrap()
    }

    fn test_cloud_limits() -> CloudLimits {
        CloudLimits {
            max_upload_bytes: 200 * 1024 * 1024,
            max_unpacked_bytes: 400 * 1024 * 1024,
            max_file_count: 20_000,
            max_file_bytes: 20 * 1024 * 1024,
        }
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
        file_entry_at("src/main.rs", content)
    }

    fn file_entry_at(path: &str, content: &[u8]) -> WorkspaceFileEntry {
        WorkspaceFileEntry {
            path: path.into(),
            content_hash: sha256_content_hash(content),
            size_bytes: content.len() as u64,
            language: path
                .ends_with(".rs")
                .then_some(LanguageId::Rust)
                .or_else(|| path.ends_with(".tsx").then_some(LanguageId::TypeScript)),
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

    fn test_analysis_config() -> CloudAnalysisConfig {
        CloudAnalysisConfig {
            rust_analyzer: PathBuf::from("rust-analyzer"),
            analysis_timeout_seconds: 120,
            lsp_file_timeout_seconds: 3,
        }
    }

    fn cargo_toml() -> &'static [u8] {
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
    }

    fn create_rust_revision(state: &CloudApiState) -> (CloudWorkspace, WorkspaceRevision) {
        let workspace = state.create_workspace(workspace_request());
        let cargo = file_entry_at("Cargo.toml", cargo_toml());
        let main_content = b"fn main() {}";
        let main = file_entry_at("src/main.rs", main_content);
        state
            .upload_blob(&workspace.id, &cargo.content_hash, cargo_toml())
            .unwrap();
        state
            .upload_blob(&workspace.id, &main.content_hash, main_content)
            .unwrap();
        let revision = state
            .create_revision(
                &workspace.id,
                CreateWorkspaceRevisionRequest {
                    base_revision: None,
                    files: vec![cargo, main],
                },
            )
            .unwrap()
            .revision;
        (workspace, revision)
    }

    fn workspace_job_request(
        workspace: &CloudWorkspace,
        revision: &WorkspaceRevision,
    ) -> CreateAnalysisJobRequest {
        CreateAnalysisJobRequest {
            source: None,
            requested_analyzers: Vec::new(),
            workspace_id: Some(workspace.id.clone()),
            revision_id: Some(revision.id.clone()),
            project_name: None,
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
        let cancelled = state.cancel_job(&job.id).unwrap().unwrap();

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

            let after_cancel = state.cancel_job(&id).unwrap().unwrap();

            assert_eq!(after_cancel.status, status);
            assert_eq!(after_cancel.message, job.message);
            assert_eq!(after_cancel.progress, job.progress);
        }
    }

    #[test]
    fn unknown_job_lookup_and_cancel_are_missing() {
        let state = test_state();

        assert!(state.get_job("missing").is_none());
        assert!(state.cancel_job("missing").unwrap().is_none());
    }

    #[test]
    fn running_jobs_are_marked_failed_on_state_hydration() {
        let root = std::env::temp_dir().join(format!(
            "rust-watcher-cloud-api-recovery-{}",
            Uuid::new_v4()
        ));
        let blobs_dir = root.join("blobs");
        let workspaces_dir = root.join("workspaces");
        std::fs::create_dir_all(&blobs_dir).unwrap();
        std::fs::create_dir_all(&workspaces_dir).unwrap();
        let store = CloudMetadataStore::open(root.join("cloud-api.sqlite")).unwrap();
        store.init_schema().unwrap();
        let running_job = terminal_job(AnalysisJobStatus::RunningAnalyzers);
        let job_id = running_job.id.clone();
        store
            .save_job(&running_job, Some("workspace_1"), Some("revision_1"))
            .unwrap();
        let persisted = store.load_all().unwrap();

        let state = CloudApiState::from_persisted(
            blobs_dir,
            workspaces_dir,
            test_analysis_config(),
            test_cloud_limits(),
            "dev-token".into(),
            "admin".into(),
            "dev-password".into(),
            store,
            JobSchedulerConfig::default(),
            persisted,
        )
        .unwrap();
        let job = state.get_job(&job_id).unwrap();

        assert_eq!(job.status, AnalysisJobStatus::Failed);
        assert!(job
            .error
            .as_deref()
            .is_some_and(|error| error.contains("restarted")));
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
    fn materialized_child_path_accepts_safe_relative_paths() {
        let root = PathBuf::from("/tmp/workspace");

        assert_eq!(
            materialized_child_path(&root, "src/main.rs").unwrap(),
            root.join("src/main.rs")
        );
        assert_eq!(
            materialized_child_path(&root, "Cargo.toml").unwrap(),
            root.join("Cargo.toml")
        );
        assert_eq!(
            materialized_child_path(&root, "frontend/App.tsx").unwrap(),
            root.join("frontend/App.tsx")
        );
    }

    #[test]
    fn materialized_child_path_rejects_escaping_paths() {
        let root = PathBuf::from("/tmp/workspace");

        assert!(materialized_child_path(&root, "/etc/passwd").is_err());
        assert!(materialized_child_path(&root, "../secret.rs").is_err());
        assert!(materialized_child_path(&root, "src/../../secret.rs").is_err());
    }

    #[test]
    fn materialize_revision_writes_files() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let main_content = b"fn main() {}";
        let app_content = b"export function App() {}";
        let main = file_entry_at("src/main.rs", main_content);
        let app = file_entry_at("frontend/App.tsx", app_content);
        state
            .upload_blob(&workspace.id, &main.content_hash, main_content)
            .unwrap();
        state
            .upload_blob(&workspace.id, &app.content_hash, app_content)
            .unwrap();
        let revision = state
            .create_revision(
                &workspace.id,
                CreateWorkspaceRevisionRequest {
                    base_revision: None,
                    files: vec![main, app],
                },
            )
            .unwrap()
            .revision;

        let root = materialize_revision(&state, &workspace.id, &revision.id).unwrap();

        assert_eq!(
            std::fs::read(root.join("src/main.rs")).unwrap(),
            main_content
        );
        assert_eq!(
            std::fs::read(root.join("frontend/App.tsx")).unwrap(),
            app_content
        );
    }

    #[test]
    fn materialize_revision_fails_for_missing_blob() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let revision = WorkspaceRevision {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace.id.clone(),
            files: vec![file_entry(b"fn main() {}")],
            files_count: 1,
            total_bytes: 12,
            parent_revision: None,
            created_at: Some(timestamp()),
        };
        state
            .revisions
            .write()
            .insert(revision.id.clone(), revision.clone());

        let error = materialize_revision(&state, &workspace.id, &revision.id).unwrap_err();

        assert!(error.to_string().contains("missing blob"));
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
        assert!(job.credits_estimated.is_some());
        assert_eq!(job.credits_used, None);
    }

    #[tokio::test]
    async fn queue_accepts_workspace_revision_job() {
        let state = test_state();
        let (workspace, revision) = create_rust_revision(&state);
        let job = state
            .create_job(workspace_job_request(&workspace, &revision))
            .unwrap();

        state.enqueue_analysis_job(&job.id).unwrap();

        let status = state.queue_status();
        assert_eq!(status.queued_jobs, 1);
        assert_eq!(status.running_jobs, 0);
        assert_eq!(status.queued_job_ids, vec![job.id]);
    }

    #[tokio::test]
    async fn queue_full_returns_too_many_requests() {
        let state =
            test_state_with_scheduler_config(JobSchedulerConfig::new(1, 1).expect("config"));
        let (workspace, revision) = create_rust_revision(&state);
        let first_response = create_job(
            State(state.clone()),
            Json(workspace_job_request(&workspace, &revision)),
        )
        .await
        .into_response();
        let second_response = create_job(
            State(state.clone()),
            Json(workspace_job_request(&workspace, &revision)),
        )
        .await
        .into_response();

        assert_eq!(first_response.status(), StatusCode::CREATED);
        assert_eq!(second_response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(state.list_jobs().len(), 1);
    }

    #[tokio::test]
    async fn worker_completes_parser_job() {
        let state = test_state();
        let (workspace, revision) = create_rust_revision(&state);
        let job = state
            .create_job(workspace_job_request(&workspace, &revision))
            .unwrap();
        state.enqueue_analysis_job(&job.id).unwrap();

        assert!(run_one_queued_job(state.clone()).await);

        let updated = state.get_job(&job.id).unwrap();
        assert_eq!(updated.status, AnalysisJobStatus::Completed);
        assert_eq!(state.queue_status().queued_jobs, 0);
        assert_eq!(state.queue_status().running_jobs, 0);
        assert!(state.analysis_results.read().contains_key(&job.id));
    }

    #[tokio::test]
    async fn worker_skips_cancelled_queued_job() {
        let state = test_state();
        let (workspace, revision) = create_rust_revision(&state);
        let job = state
            .create_job(workspace_job_request(&workspace, &revision))
            .unwrap();
        state.enqueue_analysis_job(&job.id).unwrap();
        state.cancel_job(&job.id).unwrap().unwrap();

        assert!(run_one_queued_job(state.clone()).await);

        let updated = state.get_job(&job.id).unwrap();
        assert_eq!(updated.status, AnalysisJobStatus::Cancelled);
        assert!(!state.analysis_results.read().contains_key(&job.id));
    }

    #[tokio::test]
    async fn running_job_cancellation_returns_conflict() {
        let state = test_state();
        let (workspace, revision) = create_rust_revision(&state);
        let job = state
            .create_job(workspace_job_request(&workspace, &revision))
            .unwrap();
        state.scheduler.mark_running(job.id.clone());

        let response = cancel_job(State(state.clone()), AxumPath(job.id.clone()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            state.get_job(&job.id).unwrap().status,
            AnalysisJobStatus::Queued
        );
    }

    #[test]
    fn startup_requeues_valid_queued_jobs() {
        let root =
            std::env::temp_dir().join(format!("rust-watcher-cloud-api-requeue-{}", Uuid::new_v4()));
        let blobs_dir = root.join("blobs");
        let workspaces_dir = root.join("workspaces");
        std::fs::create_dir_all(&blobs_dir).unwrap();
        std::fs::create_dir_all(&workspaces_dir).unwrap();
        let store = CloudMetadataStore::open(root.join("cloud-api.sqlite")).unwrap();
        store.init_schema().unwrap();
        let workspace = CloudWorkspace {
            id: "workspace_1".into(),
            display_name: "demo".into(),
            source: None,
            current_revision: Some("revision_1".into()),
            files_count: 1,
            total_bytes: 12,
            created_at: Some(timestamp()),
            updated_at: Some(timestamp()),
        };
        let revision = WorkspaceRevision {
            id: "revision_1".into(),
            workspace_id: workspace.id.clone(),
            files: vec![file_entry(b"fn main() {}")],
            files_count: 1,
            total_bytes: 12,
            parent_revision: None,
            created_at: Some(timestamp()),
        };
        let queued_job = terminal_job(AnalysisJobStatus::Queued);
        store.save_workspace(&workspace).unwrap();
        store.save_revision(&revision).unwrap();
        store
            .save_job(&queued_job, Some(&workspace.id), Some(&revision.id))
            .unwrap();
        let persisted = store.load_all().unwrap();

        let state = CloudApiState::from_persisted(
            blobs_dir,
            workspaces_dir,
            test_analysis_config(),
            test_cloud_limits(),
            "dev-token".into(),
            "admin".into(),
            "dev-password".into(),
            store,
            JobSchedulerConfig::default(),
            persisted,
        )
        .unwrap();

        assert_eq!(state.queue_status().queued_job_ids, vec![queued_job.id]);
    }

    #[test]
    fn startup_does_not_requeue_recovered_running_jobs() {
        let root = std::env::temp_dir().join(format!(
            "rust-watcher-cloud-api-running-recovery-{}",
            Uuid::new_v4()
        ));
        let blobs_dir = root.join("blobs");
        let workspaces_dir = root.join("workspaces");
        std::fs::create_dir_all(&blobs_dir).unwrap();
        std::fs::create_dir_all(&workspaces_dir).unwrap();
        let store = CloudMetadataStore::open(root.join("cloud-api.sqlite")).unwrap();
        store.init_schema().unwrap();
        let workspace = CloudWorkspace {
            id: "workspace_1".into(),
            display_name: "demo".into(),
            source: None,
            current_revision: Some("revision_1".into()),
            files_count: 1,
            total_bytes: 12,
            created_at: Some(timestamp()),
            updated_at: Some(timestamp()),
        };
        let revision = WorkspaceRevision {
            id: "revision_1".into(),
            workspace_id: workspace.id.clone(),
            files: vec![file_entry(b"fn main() {}")],
            files_count: 1,
            total_bytes: 12,
            parent_revision: None,
            created_at: Some(timestamp()),
        };
        let running_job = terminal_job(AnalysisJobStatus::RunningAnalyzers);
        let job_id = running_job.id.clone();
        store.save_workspace(&workspace).unwrap();
        store.save_revision(&revision).unwrap();
        store
            .save_job(&running_job, Some(&workspace.id), Some(&revision.id))
            .unwrap();
        let persisted = store.load_all().unwrap();

        let state = CloudApiState::from_persisted(
            blobs_dir,
            workspaces_dir,
            test_analysis_config(),
            test_cloud_limits(),
            "dev-token".into(),
            "admin".into(),
            "dev-password".into(),
            store,
            JobSchedulerConfig::default(),
            persisted,
        )
        .unwrap();

        assert_eq!(state.queue_status().queued_jobs, 0);
        assert_eq!(
            state.get_job(&job_id).unwrap().status,
            AnalysisJobStatus::Failed
        );
    }

    #[tokio::test]
    async fn scheduler_respects_single_concurrent_job_limit() {
        let state =
            test_state_with_scheduler_config(JobSchedulerConfig::new(1, 10).expect("config"));
        let (workspace, revision) = create_rust_revision(&state);
        let first = state
            .create_job(workspace_job_request(&workspace, &revision))
            .unwrap();
        let second = state
            .create_job(workspace_job_request(&workspace, &revision))
            .unwrap();
        state.enqueue_analysis_job(&first.id).unwrap();
        state.enqueue_analysis_job(&second.id).unwrap();

        let dequeued = state.scheduler.pop_next().unwrap();
        state.scheduler.mark_running(dequeued.clone());
        let status = state.queue_status();

        assert_eq!(status.running_jobs, 1);
        assert_eq!(status.queued_jobs, 1);
        assert_eq!(status.running_job_ids, vec![dequeued]);
        assert_eq!(status.queued_job_ids, vec![second.id]);
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

    #[test]
    fn request_detection_identifies_rust_analyzer_jobs() {
        let mut job = terminal_job(AnalysisJobStatus::Queued);
        job.requested_analyzers = Vec::new();
        assert!(!requests_rust_analyzer(&job));

        job.requested_analyzers = vec![AnalyzerEngine::RustAnalyzer];
        assert!(requests_rust_analyzer(&job));
    }

    #[test]
    fn rust_analyzer_job_estimates_more_credits_than_parser_only() {
        let state = test_state();
        let (workspace, revision) = create_rust_revision(&state);

        let parser_job = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id.clone()),
                revision_id: Some(revision.id.clone()),
                project_name: None,
            })
            .unwrap();
        let rust_analyzer_job = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();

        assert!(
            rust_analyzer_job.credits_estimated.unwrap() > parser_job.credits_estimated.unwrap()
        );
    }

    #[tokio::test]
    async fn parser_cloud_job_completes_and_stores_snapshot() {
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
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id.clone()),
                revision_id: Some(revision.id.clone()),
                project_name: None,
            })
            .unwrap();

        run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

        let updated = state.get_job(&job.id).unwrap();
        assert_eq!(updated.status, AnalysisJobStatus::Completed);
        assert_eq!(updated.progress, Some(100));
        assert!(updated.started_at.is_some());
        assert!(updated.finished_at.is_some());
        assert_eq!(updated.credits_used, updated.credits_estimated);
        assert_eq!(updated.analyzer_statuses.len(), 1);
        let result = state.analysis_results.read().get(&job.id).cloned().unwrap();
        assert_eq!(result.workspace_id, workspace.id);
        assert_eq!(result.revision_id, revision.id);
        assert!(!result.snapshot.nodes.is_empty());
        let usage = state.analysis_usage.read().get(&job.id).cloned().unwrap();
        assert_eq!(usage.job_id, job.id);
        assert_eq!(usage.workspace_id.as_deref(), Some(workspace.id.as_str()));
        assert_eq!(usage.revision_id.as_deref(), Some(revision.id.as_str()));
        assert_eq!(usage.input_files, revision.files_count);
        assert_eq!(usage.input_bytes, revision.total_bytes);
        assert_eq!(usage.output_nodes, result.snapshot.nodes.len() as u32);
        assert_eq!(usage.output_edges, result.snapshot.edges.len() as u32);
        assert_eq!(usage.output_files, result.snapshot.files.len() as u32);
        assert_eq!(usage.credits_used, usage.credits_estimated);
        assert_eq!(updated.credits_used, Some(usage.credits_used));
    }

    #[tokio::test]
    async fn unavailable_rust_analyzer_fails_requested_job() {
        let state = test_state_with_config(CloudAnalysisConfig {
            rust_analyzer: PathBuf::from("/path/that/does/not/exist/rust-analyzer"),
            analysis_timeout_seconds: 120,
            lsp_file_timeout_seconds: 1,
        });
        let (workspace, revision) = create_rust_revision(&state);
        let job = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();

        run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

        let updated = state.get_job(&job.id).unwrap();
        assert_eq!(updated.status, AnalysisJobStatus::Failed);
        assert!(updated
            .error
            .as_deref()
            .is_some_and(|error| error.contains("rust-analyzer")));
        let status = updated
            .analyzer_statuses
            .iter()
            .find(|status| status.engine == AnalyzerEngine::RustAnalyzer)
            .expect("rust-analyzer status");
        assert_eq!(status.provider, AnalyzerProvider::Cloud);
        assert!(status.billable);
        assert_eq!(status.engine, AnalyzerEngine::RustAnalyzer);
        assert_eq!(status.status, AnalyzerStatus::Error);
    }

    #[tokio::test]
    async fn usage_endpoint_returns_accepted_before_usage_is_ready() {
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
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();

        let response = get_job_usage(State(state), AxumPath(job.id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn usage_endpoint_returns_usage_after_completion() {
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
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();
        run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

        let response = get_job_usage(State(state), AxumPath(job.id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn usage_endpoint_reports_missing_and_failed_jobs() {
        let state = test_state();
        let missing_response = get_job_usage(State(state.clone()), AxumPath("missing".into()))
            .await
            .into_response();
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);

        let workspace = state.create_workspace(workspace_request());
        let revision = WorkspaceRevision {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace.id.clone(),
            files: vec![file_entry(b"fn main() {}")],
            files_count: 1,
            total_bytes: 12,
            parent_revision: None,
            created_at: Some(timestamp()),
        };
        state
            .revisions
            .write()
            .insert(revision.id.clone(), revision.clone());
        let job = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();
        run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

        let failed_response = get_job_usage(State(state), AxumPath(job.id))
            .await
            .into_response();

        assert_eq!(failed_response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn usage_summary_includes_completed_job_usage() {
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
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();
        run_parser_cloud_analysis(state.clone(), job.id).await;

        let Json(summary) = usage_summary(State(state)).await;

        assert_eq!(summary.jobs_count, 1);
        assert_eq!(summary.completed_jobs, 1);
        assert_eq!(summary.failed_jobs, 0);
        assert_eq!(summary.total_input_files, 1);
        assert_eq!(summary.total_input_bytes, content.len() as u64);
        assert!(summary.total_credits_used >= 1);
    }

    #[tokio::test]
    async fn failed_parser_cloud_job_records_error() {
        let state = test_state();
        let workspace = state.create_workspace(workspace_request());
        let revision = WorkspaceRevision {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace.id.clone(),
            files: vec![file_entry(b"fn main() {}")],
            files_count: 1,
            total_bytes: 12,
            parent_revision: None,
            created_at: Some(timestamp()),
        };
        state
            .revisions
            .write()
            .insert(revision.id.clone(), revision.clone());
        let job = state
            .create_job(CreateAnalysisJobRequest {
                source: None,
                requested_analyzers: Vec::new(),
                workspace_id: Some(workspace.id),
                revision_id: Some(revision.id),
                project_name: None,
            })
            .unwrap();

        run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

        let updated = state.get_job(&job.id).unwrap();
        assert_eq!(updated.status, AnalysisJobStatus::Failed);
        assert_eq!(updated.message.as_deref(), Some("Cloud analysis failed"));
        assert!(updated.error.is_some());
        assert!(updated.progress.is_none());
    }
}

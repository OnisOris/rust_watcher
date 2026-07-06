use axum::body::Body;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{FromRequestParts, Path as AxumPath, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, Request, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use graph_core::{
    estimate_cloud_analysis_credits, AnalysisJob, AnalysisJobSource, AnalysisJobStatus,
    AnalysisMode, AnalyzerServiceStatus, AnalyzerStatus, CloudWorkspace, CreateAnalysisJobRequest,
    GraphSnapshot,
};
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use crate::auth::{require_cloud_auth, require_cloud_session_token, require_internal_api_token};
use crate::errors::ApiError;
use crate::scheduler::{
    cloud_status_name, is_terminal, parser_analyzer_statuses, requests_rust_analyzer,
    rust_analyzer_status,
};
use crate::state::{CloudApiState, CloudEvent, JobRevisionTarget};
use crate::workspaces::{
    requested_analyzers_for_workspace_files, timestamp, CloudWorkspaceListResponse,
    CloudWorkspaceSourceResponse, CloudWorkspaceStatusResponse, SnapshotQuery,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListAnalysisJobsResponse {
    pub(crate) jobs: Vec<AnalysisJob>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisQueueStatusResponse {
    pub(crate) queued_jobs: usize,
    pub(crate) running_jobs: usize,
    pub(crate) max_concurrent_jobs: usize,
    pub(crate) max_queued_jobs: usize,
    pub(crate) queued_job_ids: Vec<String>,
    pub(crate) running_job_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSummaryResponse {
    pub jobs_count: u32,
    pub completed_jobs: u32,
    pub failed_jobs: u32,
    pub total_input_files: u64,
    pub total_input_bytes: u64,
    pub total_wall_ms: u64,
    pub total_credits_used: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudJobResponse {
    pub(crate) job_id: String,
    pub(crate) workspace_id: Option<String>,
    pub(crate) status: AnalysisJobStatus,
    pub(crate) message: Option<String>,
    pub(crate) progress: Option<f32>,
    pub(crate) analysis_mode: AnalysisMode,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
    pub(crate) credits_estimated: Option<u32>,
    pub(crate) credits_used: Option<u32>,
    pub(crate) analyzers: Vec<CloudJobAnalyzerResponse>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudJobAnalyzerResponse {
    pub(crate) kind: String,
    pub(crate) status: AnalyzerStatus,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudAnalyzeWorkspaceRequest {
    #[serde(default)]
    pub(crate) incremental: bool,
    pub(crate) base_revision_id: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudUsageResponse {
    pub(crate) credits_remaining: u32,
    pub(crate) credits_used: u32,
    pub(crate) jobs: Vec<CloudUsageJobResponse>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudUsageJobResponse {
    pub(crate) job_id: String,
    pub(crate) workspace_id: Option<String>,
    pub(crate) credits: u32,
    pub(crate) reason: String,
}

impl CloudApiState {
    pub(crate) fn create_job(
        &self,
        request: CreateAnalysisJobRequest,
    ) -> Result<AnalysisJob, ApiError> {
        let mut credits_estimated = None;
        let target = match (&request.workspace_id, &request.revision_id) {
            (Some(workspace_id), Some(revision_id)) => Some(JobRevisionTarget {
                workspace_id: workspace_id.clone(),
                revision_id: revision_id.clone(),
                base_revision_id: request.base_revision_id.clone(),
                incremental: request.incremental,
                changed_files: Vec::new(),
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
        let analysis_mode = if request.incremental {
            AnalysisMode::Incremental
        } else {
            AnalysisMode::Full
        };
        let job = AnalysisJob {
            id: id.clone(),
            status: AnalysisJobStatus::Queued,
            source,
            project_name,
            message: Some(message.into()),
            progress: Some(0),
            analysis_mode,
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
    pub(crate) fn create_job_for_request(
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
    pub(crate) fn get_job(&self, id: &str) -> Option<AnalysisJob> {
        self.jobs.read().get(id).cloned()
    }
    pub(crate) fn get_job_revision_target(&self, id: &str) -> Option<JobRevisionTarget> {
        self.job_revision_targets.read().get(id).cloned()
    }
    pub(crate) fn has_valid_revision_target(&self, id: &str) -> bool {
        self.get_job_revision_target(id)
            .and_then(|target| self.get_revision(&target.workspace_id, &target.revision_id))
            .is_some()
    }
    pub(crate) fn enqueue_analysis_job(&self, id: &str) -> Result<(), ApiError> {
        self.scheduler.enqueue(id.to_string())
    }
    pub(crate) fn queue_status(&self) -> AnalysisQueueStatusResponse {
        self.scheduler.status()
    }
    pub(crate) fn requeue_persisted_jobs(&self) {
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
    pub(crate) fn should_run_dequeued_job(&self, id: &str) -> bool {
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
    pub(crate) fn update_job_status(
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
    pub(crate) fn set_job_analyzer_statuses(
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
    pub(crate) fn set_job_analysis_mode(
        &self,
        id: &str,
        analysis_mode: AnalysisMode,
    ) -> Option<AnalysisJob> {
        let job = {
            let mut jobs = self.jobs.write();
            let job = jobs.get_mut(id)?;
            job.analysis_mode = analysis_mode;
            job.clone()
        };
        self.persist_job(&job);
        self.emit_job_event(&job, "jobStatus", job.message.clone());
        Some(job)
    }
    pub(crate) fn complete_job(
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
    pub(crate) fn emit_job_event(
        &self,
        job: &AnalysisJob,
        event_type: &str,
        message: Option<String>,
    ) {
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
    pub(crate) fn fail_job(&self, id: &str, error: impl Into<String>) -> Option<AnalysisJob> {
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
    pub(crate) fn persist_job(&self, job: &AnalysisJob) {
        let target = self.job_revision_targets.read().get(&job.id).cloned();
        if let Err(error) = self.store.save_job_with_target(job, target.as_ref()) {
            warn!(job_id = %job.id, %error, "failed to persist cloud analysis job");
        }
    }
    pub(crate) fn usage_summary(&self) -> UsageSummaryResponse {
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
    pub(crate) fn list_jobs(&self) -> Vec<AnalysisJob> {
        let mut jobs = self.jobs.read().values().cloned().collect::<Vec<_>>();
        jobs.sort_by(|left, right| right.id.cmp(&left.id));
        jobs
    }
    pub(crate) fn cancel_job(&self, id: &str) -> Result<Option<AnalysisJob>, ApiError> {
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
}

pub(crate) async fn cloud_get_job(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    match state.get_job(&id) {
        Some(job) if state.can_access_job(&job, &username) => {
            Json(cloud_job_response(&state, job)).into_response()
        }
        Some(_) => StatusCode::NOT_FOUND.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
pub(crate) async fn cloud_list_workspaces(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    let mut workspaces = state
        .list_workspaces_for_user(&username)
        .into_iter()
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
pub(crate) async fn cloud_workspace_status(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    match state.get_workspace_for_user(&id, &username) {
        Some(workspace) => Json(cloud_workspace_status_response(&state, workspace)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
pub(crate) async fn cloud_workspace_snapshot(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SnapshotQuery>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
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
pub(crate) async fn cloud_analyze_workspace(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    body: Option<Json<CloudAnalyzeWorkspaceRequest>>,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    if state.get_workspace_for_user(&id, &username).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let revision = match state.current_revision(&id) {
        Ok(revision) => revision,
        Err(error) => return error.into_response(),
    };
    let body = body
        .map(|Json(body)| body)
        .unwrap_or(CloudAnalyzeWorkspaceRequest {
            incremental: false,
            base_revision_id: None,
        });
    let requested_analyzers = requested_analyzers_for_workspace_files(&revision.files);
    match state.create_job_for_request(CreateAnalysisJobRequest {
        source: None,
        requested_analyzers,
        project_name: None,
        workspace_id: Some(id),
        revision_id: Some(revision.id),
        incremental: body.incremental,
        base_revision_id: body.base_revision_id,
    }) {
        Ok(job) => (StatusCode::ACCEPTED, Json(cloud_job_response(&state, job))).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn cloud_usage(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let username = match require_cloud_auth(&state, &headers) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    Json(cloud_usage_response(&state, &username)).into_response()
}
pub(crate) async fn cloud_ws_handler(
    State(state): State<CloudApiState>,
    request: Request<Body>,
) -> impl IntoResponse {
    let username = match cloud_ws_username_for_uri(&state, request.uri()) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    let (mut parts, _body) = request.into_parts();
    let ws = match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
        Ok(ws) => ws,
        Err(error) => return error.into_response(),
    };
    ws.on_upgrade(move |socket| cloud_websocket(socket, state, username))
        .into_response()
}
pub(crate) async fn create_job(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateAnalysisJobRequest>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.create_job_for_request(request) {
        Ok(job) => (StatusCode::CREATED, Json(job)).into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) async fn get_job(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.get_job(&id) {
        Some(job) => (StatusCode::OK, Json(job)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
pub(crate) async fn list_jobs(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    Json(ListAnalysisJobsResponse {
        jobs: state.list_jobs(),
    })
    .into_response()
}
pub(crate) async fn get_job_snapshot(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
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
pub(crate) async fn get_job_usage(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
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
pub(crate) async fn usage_summary(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> Result<Json<UsageSummaryResponse>, ApiError> {
    require_internal_api_token(&state, &headers)?;
    Ok(Json(state.usage_summary()))
}
pub(crate) async fn get_analysis_queue(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    Json(state.queue_status()).into_response()
}
pub(crate) async fn cancel_job(
    State(state): State<CloudApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if let Err(error) = require_internal_api_token(&state, &headers) {
        return error.into_response();
    }
    match state.cancel_job(&id) {
        Ok(Some(job)) => Json(job).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error.into_response(),
    }
}
pub(crate) fn cloud_job_response(state: &CloudApiState, job: AnalysisJob) -> CloudJobResponse {
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
        analysis_mode: job.analysis_mode,
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
pub(crate) fn cloud_workspace_status_response(
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
        current_revision: workspace.current_revision,
        last_job_id: last_job.map(|job| job.id),
        last_updated: workspace.updated_at,
    }
}
pub(crate) fn latest_workspace_snapshot(
    state: &CloudApiState,
    workspace_id: &str,
) -> Option<GraphSnapshot> {
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
pub(crate) fn cloud_usage_response(state: &CloudApiState, username: &str) -> CloudUsageResponse {
    let jobs = state.jobs.read();
    let workspaces = state.workspaces.read();
    let mut usage = state
        .analysis_usage
        .read()
        .values()
        .filter(|usage| {
            usage
                .workspace_id
                .as_deref()
                .and_then(|workspace_id| workspaces.get(workspace_id))
                .is_some_and(|workspace| state.workspace_belongs_to(workspace, username))
        })
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
pub(crate) fn cloud_ws_username_for_uri(
    state: &CloudApiState,
    uri: &Uri,
) -> Result<String, ApiError> {
    let token = uri
        .query()
        .and_then(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "token")
                .map(|(_, value)| value.into_owned())
        })
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| ApiError::Unauthorized("missing session token".into()))?;
    require_cloud_session_token(state, &token)
}
pub(crate) fn cloud_event_visible_to_user(
    state: &CloudApiState,
    event: &CloudEvent,
    username: &str,
) -> bool {
    if let Some(workspace_id) = event.workspace_id.as_deref() {
        return state
            .get_workspace_for_user(workspace_id, username)
            .is_some();
    }
    if let Some(job_id) = event.job_id.as_deref() {
        return state
            .get_job(job_id)
            .is_some_and(|job| state.can_access_job(&job, username));
    }
    matches!(event.event_type.as_str(), "heartbeat" | "connected")
}
pub(crate) async fn cloud_websocket(socket: WebSocket, state: CloudApiState, username: String) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.ws_tx.subscribe();
    let forward_state = state.clone();
    let forward = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if !cloud_event_visible_to_user(&forward_state, &event, &username) {
                continue;
            }
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

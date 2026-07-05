use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Serialize;

use crate::agent;
use crate::auth;
use crate::ide;
use crate::imports;
use crate::jobs;
use crate::self_update;
use crate::state::CloudApiState;
use crate::workspaces;

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) ok: bool,
    pub(crate) service: &'static str,
    pub(crate) version: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudHealthResponse {
    pub(crate) ok: bool,
    pub(crate) version: &'static str,
    pub(crate) mode: &'static str,
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "cloud-api",
        version: env!("CARGO_PKG_VERSION"),
    })
}
pub(crate) async fn cloud_health() -> Json<CloudHealthResponse> {
    Json(CloudHealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        mode: "cloud",
    })
}

pub(crate) fn router() -> Router<CloudApiState> {
    Router::new()
        .route("/api/cloud/health", get(cloud_health))
        .route("/api/cloud/auth/login", post(auth::cloud_login))
        .route("/api/cloud/auth/me", get(auth::cloud_me))
        .route(
            "/api/cloud/update/status",
            get(self_update::cloud_update_status),
        )
        .route(
            "/api/cloud/update/apply",
            post(self_update::cloud_update_apply),
        )
        .route(
            "/api/cloud/import/github",
            post(imports::cloud_import_github),
        )
        .route("/api/cloud/upload", post(imports::cloud_upload_zip))
        .route("/api/cloud/jobs/{id}", get(jobs::cloud_get_job))
        .route("/api/cloud/workspaces", get(jobs::cloud_list_workspaces))
        .route(
            "/api/cloud/workspaces/{id}/status",
            get(jobs::cloud_workspace_status),
        )
        .route(
            "/api/cloud/workspaces/{id}/snapshot",
            get(jobs::cloud_workspace_snapshot),
        )
        .route(
            "/api/cloud/workspaces/{id}/files",
            get(ide::cloud_workspace_files),
        )
        .route(
            "/api/cloud/workspaces/{id}/files/content",
            get(ide::cloud_workspace_file_content).put(ide::cloud_save_workspace_file),
        )
        .route(
            "/api/cloud/workspaces/{id}/analyze",
            post(jobs::cloud_analyze_workspace),
        )
        .route("/api/cloud/usage", get(jobs::cloud_usage))
        .route("/api/cloud/ws", get(jobs::cloud_ws_handler))
        .route(
            "/api/cloud/agent/sessions",
            post(agent::agent_create_session),
        )
        .route(
            "/api/cloud/agent/sessions/{id}/files",
            post(agent::agent_upload_files),
        )
        .route(
            "/api/cloud/agent/sessions/{id}/changes",
            post(agent::agent_upload_files),
        )
        .route(
            "/api/cloud/agent/sessions/{id}/analyze",
            post(agent::agent_analyze_session),
        )
        .route("/api/health", get(health))
        .route(
            "/api/workspaces",
            get(workspaces::list_workspaces).post(workspaces::create_workspace),
        )
        .route("/api/workspaces/{id}", get(workspaces::get_workspace))
        .route(
            "/api/workspaces/{id}/sync-plan",
            post(workspaces::sync_plan),
        )
        .route(
            "/api/workspaces/{id}/blobs/{content_hash}",
            put(workspaces::upload_blob),
        )
        .route(
            "/api/workspaces/{id}/revisions",
            post(workspaces::create_revision),
        )
        .route(
            "/api/workspaces/{workspace_id}/revisions/{revision_id}",
            get(workspaces::get_revision),
        )
        .route(
            "/api/analysis/jobs",
            get(jobs::list_jobs).post(jobs::create_job),
        )
        .route("/api/analysis/jobs/{id}", get(jobs::get_job))
        .route(
            "/api/analysis/jobs/{id}/snapshot",
            get(jobs::get_job_snapshot),
        )
        .route("/api/analysis/jobs/{id}/usage", get(jobs::get_job_usage))
        .route("/api/analysis/jobs/{id}/cancel", post(jobs::cancel_job))
        .route("/api/analysis/queue", get(jobs::get_analysis_queue))
        .route("/api/usage/summary", get(jobs::usage_summary))
}

use anyhow::{Context, Result};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use graph_core::{
    AnalysisJob, AnalysisJobSource, AnalysisJobStatus, AnalyzerEngine, AnalyzerServiceStatus,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
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
}

#[derive(Clone, Default)]
struct CloudApiState {
    jobs: Arc<RwLock<HashMap<String, AnalysisJob>>>,
}

impl CloudApiState {
    fn create_job(&self, request: CreateAnalysisJobRequest) -> AnalysisJob {
        let id = Uuid::new_v4().to_string();
        let job = AnalysisJob {
            id: id.clone(),
            status: AnalysisJobStatus::Queued,
            source: request.source,
            project_name: request.project_name,
            message: Some("Queued for analysis".into()),
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
        job
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAnalysisJobRequest {
    source: AnalysisJobSource,
    #[serde(default)]
    requested_analyzers: Vec<AnalyzerEngine>,
    project_name: Option<String>,
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
    let state = CloudApiState::default();
    let app = Router::new()
        .route("/api/health", get(health))
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
    (StatusCode::CREATED, Json(state.create_job(request)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{AnalysisJobSourceKind, AnalyzerProvider};

    fn local_request() -> CreateAnalysisJobRequest {
        CreateAnalysisJobRequest {
            source: AnalysisJobSource {
                kind: AnalysisJobSourceKind::LocalPath,
                display_name: Some("demo".into()),
                path: Some("/tmp/demo".into()),
                repository_url: None,
                git_ref: None,
                commit_sha: None,
            },
            requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
            project_name: Some("demo".into()),
        }
    }

    fn terminal_job(status: AnalysisJobStatus) -> AnalysisJob {
        AnalysisJob {
            id: Uuid::new_v4().to_string(),
            status,
            source: local_request().source,
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
        let state = CloudApiState::default();
        let job = state.create_job(local_request());

        assert_eq!(job.status, AnalysisJobStatus::Queued);
        assert_eq!(job.message.as_deref(), Some("Queued for analysis"));
        assert_eq!(job.progress, Some(0));
        assert_eq!(job.requested_analyzers, vec![AnalyzerEngine::RustAnalyzer]);
        assert_eq!(state.get_job(&job.id).unwrap().id, job.id);
    }

    #[test]
    fn getting_known_job_returns_it() {
        let state = CloudApiState::default();
        let job = state.create_job(local_request());

        assert_eq!(
            state.get_job(&job.id).unwrap().project_name.as_deref(),
            Some("demo")
        );
    }

    #[test]
    fn cancelling_queued_job_marks_cancelled() {
        let state = CloudApiState::default();
        let job = state.create_job(local_request());
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
            let state = CloudApiState::default();
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
        let state = CloudApiState::default();

        assert!(state.get_job("missing").is_none());
        assert!(state.cancel_job("missing").is_none());
    }
}

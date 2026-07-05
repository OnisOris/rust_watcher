use anyhow::Result;
use graph_core::{
    AnalysisJob, CloudAnalysisUsage, CloudWorkspace, GraphSnapshot, WorkspaceFileEntry,
    WorkspaceRevision,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::scheduler::{recover_running_jobs, JobScheduler, JobSchedulerConfig};
use crate::storage::{CloudMetadataStore, PersistedCloudState};

#[derive(Clone)]
pub(crate) struct CloudApiState {
    pub(crate) jobs: Arc<RwLock<HashMap<String, AnalysisJob>>>,
    pub(crate) job_revision_targets: Arc<RwLock<HashMap<String, JobRevisionTarget>>>,
    pub(crate) workspaces: Arc<RwLock<HashMap<String, CloudWorkspace>>>,
    pub(crate) revisions: Arc<RwLock<HashMap<String, WorkspaceRevision>>>,
    pub(crate) blobs: Arc<RwLock<HashMap<String, StoredBlob>>>,
    pub(crate) analysis_results: Arc<RwLock<HashMap<String, CloudAnalysisResult>>>,
    pub(crate) analysis_usage: Arc<RwLock<HashMap<String, CloudAnalysisUsage>>>,
    pub(crate) blobs_dir: Arc<PathBuf>,
    pub(crate) workspaces_dir: Arc<PathBuf>,
    pub(crate) analysis_config: Arc<CloudAnalysisConfig>,
    pub(crate) limits: Arc<CloudLimits>,
    pub(crate) dev_token: Arc<String>,
    pub(crate) auth_users: Arc<HashMap<String, String>>,
    pub(crate) auth_sessions: Arc<RwLock<HashMap<String, AuthSession>>>,
    pub(crate) auth_session_ttl_seconds: u64,
    pub(crate) default_owner_username: Arc<String>,
    pub(crate) agent_sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
    pub(crate) update_config: Arc<SelfUpdateConfig>,
    pub(crate) update_state: Arc<RwLock<SelfUpdateState>>,
    pub(crate) ws_tx: broadcast::Sender<CloudEvent>,
    pub(crate) scheduler: JobScheduler,
    pub(crate) store: Arc<CloudMetadataStore>,
}
#[derive(Debug, Clone)]
pub(crate) struct CloudAnalysisConfig {
    pub(crate) rust_analyzer: PathBuf,
    pub(crate) analysis_timeout_seconds: u64,
    pub(crate) lsp_file_timeout_seconds: u64,
}
#[derive(Debug, Clone)]
pub(crate) struct CloudLimits {
    pub(crate) max_upload_bytes: u64,
    pub(crate) max_unpacked_bytes: u64,
    pub(crate) max_file_count: usize,
    pub(crate) max_file_bytes: u64,
}
#[derive(Debug, Clone)]
pub(crate) struct SelfUpdateConfig {
    pub(crate) repository: String,
    pub(crate) asset_prefix: String,
    pub(crate) service_name: String,
    pub(crate) app_root: PathBuf,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct SelfUpdateState {
    pub(crate) running: bool,
    pub(crate) last_message: Option<String>,
}
#[derive(Debug, Clone)]
pub(crate) struct AuthSession {
    pub(crate) username: String,
    pub(crate) expires_at: u64,
}
#[derive(Debug, Clone)]
pub(crate) struct AgentSession {
    pub(crate) workspace_id: String,
    pub(crate) owner_username: String,
    pub(crate) project_name: String,
    pub(crate) files: HashMap<String, WorkspaceFileEntry>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudEvent {
    #[serde(rename = "type")]
    pub(crate) event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
}
#[derive(Debug, Clone)]
pub(crate) struct JobRevisionTarget {
    pub(crate) workspace_id: String,
    pub(crate) revision_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudAnalysisResult {
    pub job_id: String,
    pub workspace_id: String,
    pub revision_id: String,
    pub snapshot: GraphSnapshot,
    pub created_at: String,
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct StoredBlob {
    pub(crate) content_hash: String,
    pub(crate) size_bytes: u64,
    pub(crate) storage_path: String,
    pub(crate) created_at: String,
}

impl CloudApiState {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_persisted(
        blobs_dir: PathBuf,
        workspaces_dir: PathBuf,
        analysis_config: CloudAnalysisConfig,
        limits: CloudLimits,
        dev_token: String,
        auth_users: HashMap<String, String>,
        auth_session_ttl_seconds: u64,
        default_owner_username: String,
        update_config: SelfUpdateConfig,
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
            auth_users: Arc::new(auth_users),
            auth_sessions: Arc::new(RwLock::new(HashMap::new())),
            auth_session_ttl_seconds,
            default_owner_username: Arc::new(default_owner_username),
            agent_sessions: Arc::new(RwLock::new(HashMap::new())),
            update_config: Arc::new(update_config),
            update_state: Arc::new(RwLock::new(SelfUpdateState::default())),
            ws_tx: broadcast::channel(128).0,
            scheduler: JobScheduler::new(scheduler_config),
            store: Arc::new(store),
        };
        state.requeue_persisted_jobs();
        Ok(state)
    }
}

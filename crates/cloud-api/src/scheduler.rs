use anyhow::Result;
use graph_core::{
    estimate_cloud_analysis_credits, AnalysisJob, AnalysisJobStatus, AnalysisMode,
    AnalyzerCapability, AnalyzerEngine, AnalyzerKind, AnalyzerProvider, AnalyzerServiceStatus,
    AnalyzerStatus, AppState, AppStatus, CloudAnalysisUsage, GraphSnapshot, WorkspaceRevision,
};
use parking_lot::RwLock;
use project_indexer::ProjectIndex;
use ra_client::{LspRuntime, LspRuntimeConfig, LspRuntimeMode};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::errors::ApiError;
use crate::jobs::AnalysisQueueStatusResponse;
use crate::state::{
    CachedParserSnapshot, CloudAnalysisResult, CloudApiState, FileAnalysisCacheEntry,
    FileAnalysisCacheKey, FileAnalysisCacheMetrics, JobRevisionTarget, ParserSnapshotCacheFileKey,
    ParserSnapshotCacheKey,
};
use crate::workspaces::{materialize_revision, timestamp};

#[derive(Debug, Clone, Copy)]
pub(crate) struct JobSchedulerConfig {
    pub(crate) max_concurrent_jobs: usize,
    pub(crate) max_queued_jobs: usize,
}

impl JobSchedulerConfig {
    pub(crate) fn new(max_concurrent_jobs: usize, max_queued_jobs: usize) -> Result<Self> {
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
pub(crate) struct JobScheduler {
    pub(crate) queue: Arc<RwLock<VecDeque<String>>>,
    pub(crate) running: Arc<RwLock<HashSet<String>>>,
    pub(crate) notify: Arc<Notify>,
    pub(crate) config: JobSchedulerConfig,
}

impl JobScheduler {
    pub(crate) fn new(config: JobSchedulerConfig) -> Self {
        Self {
            queue: Arc::new(RwLock::new(VecDeque::new())),
            running: Arc::new(RwLock::new(HashSet::new())),
            notify: Arc::new(Notify::new()),
            config,
        }
    }

    pub(crate) fn enqueue(&self, job_id: String) -> Result<(), ApiError> {
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

    pub(crate) fn pop_next(&self) -> Option<String> {
        self.queue.write().pop_front()
    }

    pub(crate) fn mark_running(&self, job_id: String) {
        self.running.write().insert(job_id);
    }

    pub(crate) fn finish_running(&self, job_id: &str) {
        self.running.write().remove(job_id);
    }

    pub(crate) fn is_running(&self, job_id: &str) -> bool {
        self.running.read().contains(job_id)
    }

    pub(crate) fn status(&self) -> AnalysisQueueStatusResponse {
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

pub(crate) fn is_terminal(status: AnalysisJobStatus) -> bool {
    matches!(
        status,
        AnalysisJobStatus::Completed | AnalysisJobStatus::Failed | AnalysisJobStatus::Cancelled
    )
}
pub(crate) fn is_running_status(status: AnalysisJobStatus) -> bool {
    matches!(
        status,
        AnalysisJobStatus::Preparing
            | AnalysisJobStatus::Indexing
            | AnalysisJobStatus::RunningAnalyzers
            | AnalysisJobStatus::BuildingGraph
    )
}
pub(crate) fn cloud_status_name(status: AnalysisJobStatus) -> &'static str {
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
pub(crate) fn recover_running_jobs(jobs: &mut HashMap<String, AnalysisJob>) {
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
pub(crate) fn start_analysis_workers(state: CloudApiState) {
    for worker_index in 0..state.scheduler.config.max_concurrent_jobs {
        let worker_state = state.clone();
        tokio::spawn(async move {
            analysis_worker_loop(worker_state, worker_index).await;
        });
    }
}
pub(crate) async fn analysis_worker_loop(state: CloudApiState, worker_index: usize) {
    loop {
        let notified = state.scheduler.notify.notified();
        if run_one_queued_job(state.clone()).await {
            continue;
        }
        tracing::debug!(worker_index, "cloud analysis worker waiting for queued job");
        notified.await;
    }
}
pub(crate) async fn run_one_queued_job(state: CloudApiState) -> bool {
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
pub(crate) async fn run_parser_cloud_analysis(state: CloudApiState, job_id: String) {
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
pub(crate) async fn execute_cloud_analysis_job(
    state: CloudApiState,
    job_id: String,
) -> Result<GraphSnapshot> {
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
    let analysis_mode = prepare_cloud_analysis_mode(&state, &job_id, &target);
    let credits_estimated = estimate_cloud_analysis_credits(
        revision.files_count,
        revision.total_bytes,
        &requested_analyzers,
    );
    state.update_job_status(
        &job_id,
        AnalysisJobStatus::Preparing,
        match analysis_mode {
            AnalysisMode::Full => "Preparing cloud analysis",
            AnalysisMode::Incremental => "Preparing incremental cloud analysis",
            AnalysisMode::FallbackFull => "Preparing full cloud analysis fallback",
        },
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
    let (mut snapshot, project_index) =
        build_cached_initial_snapshot(&state, &job_id, &project_root, &revision);
    let rust_analyzer_requested = requested_analyzers.contains(&AnalyzerEngine::RustAnalyzer);
    if rust_analyzer_requested
        && state.analysis_config.rust_analyzer.is_absolute()
        && !state.analysis_config.rust_analyzer.exists()
    {
        anyhow::bail!(
            "rust-analyzer unavailable in cloud worker: {} does not exist",
            state.analysis_config.rust_analyzer.display()
        );
    }
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
                &[AnalyzerEngine::Parser, AnalyzerEngine::RustAnalyzer],
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
    let optional_semantic_requested = requested_analyzers.iter().any(|analyzer| {
        matches!(
            analyzer,
            AnalyzerEngine::Ty
                | AnalyzerEngine::TypeScriptLanguageServer
                | AnalyzerEngine::QmlLanguageServer
        )
    });
    snapshot.status.analyzers = cloud_analyzer_statuses(
        &snapshot,
        &requested_analyzers,
        rust_analyzer_final_status.clone(),
    );
    snapshot.status.message = Some(if rust_analyzer_requested {
        match rust_analyzer_final_status
            .as_ref()
            .map(|status| status.status)
            .unwrap_or(AnalyzerStatus::Fallback)
        {
            AnalyzerStatus::Ready => "Cloud rust-analyzer analysis completed".into(),
            _ => "Cloud parser analysis completed; rust-analyzer used fallback".into(),
        }
    } else if optional_semantic_requested {
        "Cloud parser analysis completed; optional semantic analyzers used fallback".into()
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

fn prepare_cloud_analysis_mode(
    state: &CloudApiState,
    job_id: &str,
    target: &JobRevisionTarget,
) -> AnalysisMode {
    if !target.incremental {
        state.set_job_analysis_mode(job_id, AnalysisMode::Full);
        return AnalysisMode::Full;
    }
    let Some(base_revision_id) = target.base_revision_id.as_deref() else {
        state.set_job_analysis_mode(job_id, AnalysisMode::FallbackFull);
        return AnalysisMode::FallbackFull;
    };
    let Ok(diff) = state.workspace_revision_diff(
        &target.workspace_id,
        base_revision_id,
        Some(&target.revision_id),
    ) else {
        state.set_job_analysis_mode(job_id, AnalysisMode::FallbackFull);
        return AnalysisMode::FallbackFull;
    };
    let changed_files = diff
        .added_files
        .iter()
        .chain(diff.modified_files.iter())
        .chain(diff.removed_files.iter())
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    if let Some(target) = state.job_revision_targets.write().get_mut(job_id) {
        target.changed_files = changed_files;
    }
    let mode = AnalysisMode::FallbackFull;
    state.set_job_analysis_mode(job_id, mode);
    mode
}
pub(crate) fn build_initial_snapshot(project_root: &Path) -> (GraphSnapshot, Option<ProjectIndex>) {
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
const PARSER_ANALYSIS_CACHE_VERSION: &str = "cloud-parser-v1";

pub(crate) fn build_cached_initial_snapshot(
    state: &CloudApiState,
    job_id: &str,
    project_root: &Path,
    revision: &WorkspaceRevision,
) -> (GraphSnapshot, Option<ProjectIndex>) {
    let snapshot_key = parser_snapshot_cache_key(revision);
    let total_files = snapshot_key.files.len();
    let mut metrics = {
        let cache = state.file_analysis_cache.read();
        let hits = snapshot_key
            .files
            .iter()
            .filter(|file| {
                cache
                    .entries
                    .get(&file.key)
                    .is_some_and(|entry| entry.last_path == file.path)
            })
            .count();
        FileAnalysisCacheMetrics {
            hits,
            misses: total_files.saturating_sub(hits),
            reused_files: 0,
        }
    };
    let cached_snapshot = {
        let cache = state.file_analysis_cache.read();
        cache
            .parser_snapshots
            .get(&snapshot_key)
            .map(|cached| cached.snapshot.clone())
    };
    if let Some(mut snapshot) = cached_snapshot {
        metrics.reused_files = total_files;
        state.file_analysis_cache.write().last_metrics = metrics.clone();
        refresh_cached_snapshot_status(&mut snapshot, project_root);
        info!(
            job_id = %job_id,
            cache_hits = metrics.hits,
            cache_misses = metrics.misses,
            reused_files_count = metrics.reused_files,
            "cloud parser analysis cache hit"
        );
        return (snapshot, index_project_if_available(project_root));
    }

    info!(
        job_id = %job_id,
        cache_hits = metrics.hits,
        cache_misses = metrics.misses,
        reused_files_count = metrics.reused_files,
        "cloud parser analysis cache miss"
    );
    let (snapshot, project_index) = build_initial_snapshot(project_root);
    {
        let mut cache = state.file_analysis_cache.write();
        for file in &snapshot_key.files {
            cache.entries.insert(
                file.key.clone(),
                FileAnalysisCacheEntry {
                    last_path: file.path.clone(),
                },
            );
        }
        cache.parser_snapshots.insert(
            snapshot_key,
            CachedParserSnapshot {
                snapshot: snapshot.clone(),
            },
        );
        cache.last_metrics = metrics;
    }
    (snapshot, project_index)
}

fn parser_snapshot_cache_key(revision: &WorkspaceRevision) -> ParserSnapshotCacheKey {
    let mut files = revision
        .files
        .iter()
        .map(|file| ParserSnapshotCacheFileKey {
            path: file.path.clone(),
            key: FileAnalysisCacheKey {
                content_hash: file.content_hash.clone(),
                language: file.language.as_ref().map(ToString::to_string),
                analyzer_engine: AnalyzerEngine::Parser,
                analyzer_config_hash: parser_analyzer_config_hash(),
            },
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    ParserSnapshotCacheKey { files }
}

fn parser_analyzer_config_hash() -> String {
    PARSER_ANALYSIS_CACHE_VERSION.to_string()
}

fn refresh_cached_snapshot_status(snapshot: &mut GraphSnapshot, project_root: &Path) {
    snapshot.status.project_path = Some(project_root.display().to_string());
    snapshot.status.last_updated = Some(timestamp());
}

fn index_project_if_available(project_root: &Path) -> Option<ProjectIndex> {
    if project_root.join("Cargo.toml").is_file() {
        project_indexer::index_project(project_root).ok()
    } else {
        None
    }
}
pub(crate) async fn enrich_with_cloud_rust_analyzer(
    state: &CloudApiState,
    job_id: &str,
    snapshot: &mut GraphSnapshot,
    index: &ProjectIndex,
) -> Result<()> {
    if state.analysis_config.rust_analyzer.is_absolute()
        && !state.analysis_config.rust_analyzer.exists()
    {
        anyhow::bail!(
            "rust-analyzer unavailable in cloud worker: {} does not exist",
            state.analysis_config.rust_analyzer.display()
        );
    }
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
            &[AnalyzerEngine::Parser, AnalyzerEngine::RustAnalyzer],
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
            &[AnalyzerEngine::Parser, AnalyzerEngine::RustAnalyzer],
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
pub(crate) fn cloud_binary_resolver(configured: &Path, _root: &Path) -> PathBuf {
    configured.to_path_buf()
}
pub(crate) fn requests_rust_analyzer(job: &AnalysisJob) -> bool {
    job.requested_analyzers
        .contains(&AnalyzerEngine::RustAnalyzer)
}
pub(crate) fn rust_file_count(index: Option<&ProjectIndex>) -> u32 {
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
pub(crate) fn parser_analyzer_statuses(snapshot: &GraphSnapshot) -> Vec<AnalyzerServiceStatus> {
    cloud_analyzer_statuses(snapshot, &[AnalyzerEngine::Parser], None)
}
pub(crate) fn cloud_analyzer_statuses(
    snapshot: &GraphSnapshot,
    requested_analyzers: &[AnalyzerEngine],
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
    for analyzer in requested_analyzers {
        if let Some(status) = optional_cloud_analyzer_fallback_status(*analyzer, snapshot) {
            statuses.push(status);
        }
    }
    statuses
}
fn optional_cloud_analyzer_fallback_status(
    analyzer: AnalyzerEngine,
    snapshot: &GraphSnapshot,
) -> Option<AnalyzerServiceStatus> {
    let (id, kind, label, file_count, message) = match analyzer {
        AnalyzerEngine::Ty => (
            "ty",
            AnalyzerKind::Python,
            "ty",
            files_with_extensions(snapshot, &["py"]),
            "Cloud ty analyzer is not available in this worker; Python parser fallback used",
        ),
        AnalyzerEngine::TypeScriptLanguageServer => (
            "typescript-language-server",
            AnalyzerKind::TypeScript,
            "typescript-language-server",
            files_with_extensions(snapshot, &["ts", "tsx", "js", "jsx"]),
            "Cloud TypeScript language server is not available in this worker; TypeScript parser fallback used",
        ),
        AnalyzerEngine::QmlLanguageServer => (
            "qmlls",
            AnalyzerKind::Qml,
            "qmlls",
            files_with_extensions(snapshot, &["qml"]),
            "Cloud qmlls analyzer is not available in this worker; QML parser fallback used",
        ),
        _ => return None,
    };
    Some(AnalyzerServiceStatus {
        id: id.into(),
        kind,
        engine: analyzer,
        label: label.into(),
        status: AnalyzerStatus::Fallback,
        mode: Some("cloud".into()),
        message: Some(message.into()),
        capabilities: vec![AnalyzerCapability::Symbols],
        files_indexed: file_count,
        last_updated: Some(timestamp()),
        provider: AnalyzerProvider::Cloud,
        billable: false,
        credits_used: None,
    })
}
fn files_with_extensions(snapshot: &GraphSnapshot, extensions: &[&str]) -> u32 {
    snapshot
        .files
        .iter()
        .filter(|file| {
            Path::new(file.path.as_str())
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
        })
        .count() as u32
}
pub(crate) fn rust_analyzer_status(
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
pub(crate) fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

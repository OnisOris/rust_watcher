use crate::{CloudAnalysisResult, JobRevisionTarget, StoredBlob};
use anyhow::{Context, Result};
use graph_core::{AnalysisJob, CloudAnalysisUsage, CloudWorkspace, WorkspaceRevision};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct CloudMetadataStore {
    db_path: PathBuf,
}

#[derive(Debug, Default)]
pub(crate) struct PersistedCloudState {
    pub workspaces: HashMap<String, CloudWorkspace>,
    pub revisions: HashMap<String, WorkspaceRevision>,
    pub blobs: HashMap<String, StoredBlob>,
    pub jobs: HashMap<String, AnalysisJob>,
    pub job_revision_targets: HashMap<String, JobRevisionTarget>,
    pub analysis_results: HashMap<String, CloudAnalysisResult>,
    pub analysis_usage: HashMap<String, CloudAnalysisUsage>,
}

impl CloudMetadataStore {
    pub fn open(db_path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = db_path.into();
        if let Some(parent) = db_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(Self { db_path })
    }

    pub fn init_schema(&self) -> Result<()> {
        self.connection()?
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS workspaces (
                    id TEXT PRIMARY KEY,
                    json TEXT NOT NULL,
                    created_at TEXT,
                    updated_at TEXT
                );
                CREATE TABLE IF NOT EXISTS revisions (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    json TEXT NOT NULL,
                    created_at TEXT
                );
                CREATE TABLE IF NOT EXISTS blobs (
                    content_hash TEXT PRIMARY KEY,
                    size_bytes INTEGER NOT NULL,
                    storage_path TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS analysis_jobs (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT,
                    revision_id TEXT,
                    json TEXT NOT NULL,
                    created_at TEXT,
                    updated_at TEXT
                );
                CREATE TABLE IF NOT EXISTS analysis_results (
                    job_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    revision_id TEXT NOT NULL,
                    snapshot_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS analysis_usage (
                    job_id TEXT PRIMARY KEY,
                    json TEXT NOT NULL,
                    created_at TEXT
                );
                "#,
            )
            .context("failed to initialize cloud metadata schema")
    }

    pub fn load_all(&self) -> Result<PersistedCloudState> {
        let connection = self.connection()?;
        let workspaces = load_json_map(&connection, "SELECT json FROM workspaces")?;
        let revisions = load_json_map(&connection, "SELECT json FROM revisions")?;
        let blobs = load_blobs(&connection)?;
        let (jobs, job_revision_targets) = load_jobs(&connection)?;
        let analysis_results = load_analysis_results(&connection)?;
        let analysis_usage = load_json_map(&connection, "SELECT json FROM analysis_usage")?;
        Ok(PersistedCloudState {
            workspaces,
            revisions,
            blobs,
            jobs,
            job_revision_targets,
            analysis_results,
            analysis_usage,
        })
    }

    pub fn save_workspace(&self, workspace: &CloudWorkspace) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR REPLACE INTO workspaces (id, json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                &workspace.id,
                serde_json::to_string(workspace)?,
                workspace.created_at.as_deref(),
                workspace.updated_at.as_deref(),
            ],
        )?;
        Ok(())
    }

    pub fn save_revision(&self, revision: &WorkspaceRevision) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR REPLACE INTO revisions (id, workspace_id, json, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                &revision.id,
                &revision.workspace_id,
                serde_json::to_string(revision)?,
                revision.created_at.as_deref(),
            ],
        )?;
        Ok(())
    }

    pub fn save_blob(&self, blob: &StoredBlob) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR REPLACE INTO blobs (content_hash, size_bytes, storage_path, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                &blob.content_hash,
                blob.size_bytes,
                &blob.storage_path,
                &blob.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn save_job(
        &self,
        job: &AnalysisJob,
        workspace_id: Option<&str>,
        revision_id: Option<&str>,
    ) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR REPLACE INTO analysis_jobs (id, workspace_id, revision_id, json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &job.id,
                workspace_id,
                revision_id,
                serde_json::to_string(job)?,
                job.created_at.as_deref(),
                job.finished_at.as_deref().or(job.started_at.as_deref()),
            ],
        )?;
        Ok(())
    }

    pub fn save_analysis_result(&self, result: &CloudAnalysisResult) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR REPLACE INTO analysis_results (job_id, workspace_id, revision_id, snapshot_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &result.job_id,
                &result.workspace_id,
                &result.revision_id,
                serde_json::to_string(&result.snapshot)?,
                &result.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn save_usage(&self, usage: &CloudAnalysisUsage) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR REPLACE INTO analysis_usage (job_id, json, created_at) VALUES (?1, ?2, ?3)",
            params![
                &usage.job_id,
                serde_json::to_string(usage)?,
                usage.created_at.as_deref(),
            ],
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        Connection::open(&self.db_path)
            .with_context(|| format!("failed to open {}", self.db_path.display()))
    }
}

fn load_json_map<T>(connection: &Connection, query: &str) -> Result<HashMap<String, T>>
where
    T: serde::de::DeserializeOwned + HasId,
{
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut values = HashMap::new();
    for row in rows {
        let value: T = serde_json::from_str(&row?)?;
        values.insert(value.id().to_string(), value);
    }
    Ok(values)
}

fn load_blobs(connection: &Connection) -> Result<HashMap<String, StoredBlob>> {
    let mut statement = connection
        .prepare("SELECT content_hash, size_bytes, storage_path, created_at FROM blobs")?;
    let rows = statement.query_map([], |row| {
        Ok(StoredBlob {
            content_hash: row.get(0)?,
            size_bytes: row.get::<_, i64>(1)? as u64,
            storage_path: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    let mut blobs = HashMap::new();
    for row in rows {
        let blob = row?;
        blobs.insert(blob.content_hash.clone(), blob);
    }
    Ok(blobs)
}

fn load_jobs(
    connection: &Connection,
) -> Result<(
    HashMap<String, AnalysisJob>,
    HashMap<String, JobRevisionTarget>,
)> {
    let mut statement =
        connection.prepare("SELECT id, workspace_id, revision_id, json FROM analysis_jobs")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut jobs = HashMap::new();
    let mut targets = HashMap::new();
    for row in rows {
        let (id, workspace_id, revision_id, json) = row?;
        let job: AnalysisJob = serde_json::from_str(&json)?;
        if let (Some(workspace_id), Some(revision_id)) = (workspace_id, revision_id) {
            targets.insert(
                id.clone(),
                JobRevisionTarget {
                    workspace_id,
                    revision_id,
                },
            );
        }
        jobs.insert(job.id.clone(), job);
    }
    Ok((jobs, targets))
}

fn load_analysis_results(connection: &Connection) -> Result<HashMap<String, CloudAnalysisResult>> {
    let mut statement = connection.prepare(
        "SELECT job_id, workspace_id, revision_id, snapshot_json, created_at FROM analysis_results",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut results = HashMap::new();
    for row in rows {
        let (job_id, workspace_id, revision_id, snapshot_json, created_at) = row?;
        let result = CloudAnalysisResult {
            job_id: job_id.clone(),
            workspace_id,
            revision_id,
            snapshot: serde_json::from_str(&snapshot_json)?,
            created_at,
        };
        results.insert(job_id, result);
    }
    Ok(results)
}

trait HasId {
    fn id(&self) -> &str;
}

impl HasId for CloudWorkspace {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for WorkspaceRevision {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for CloudAnalysisUsage {
    fn id(&self) -> &str {
        &self.job_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{
        AnalysisJobSource, AnalysisJobSourceKind, AnalysisJobStatus, AnalyzerEngine, AppStatus,
        GraphSnapshot, WorkspaceFileEntry,
    };
    use uuid::Uuid;

    fn store() -> CloudMetadataStore {
        let root =
            std::env::temp_dir().join(format!("rust-watcher-cloud-api-storage-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = CloudMetadataStore::open(root.join("cloud-api.sqlite")).unwrap();
        store.init_schema().unwrap();
        store
    }

    fn workspace() -> CloudWorkspace {
        CloudWorkspace {
            id: "workspace_1".into(),
            display_name: "demo".into(),
            source: None,
            current_revision: None,
            files_count: 0,
            total_bytes: 0,
            created_at: Some("1".into()),
            updated_at: Some("1".into()),
        }
    }

    fn revision() -> WorkspaceRevision {
        WorkspaceRevision {
            id: "revision_1".into(),
            workspace_id: "workspace_1".into(),
            files: vec![WorkspaceFileEntry {
                path: "src/main.rs".into(),
                content_hash: "sha256:abc".into(),
                size_bytes: 12,
                language: None,
            }],
            files_count: 1,
            total_bytes: 12,
            parent_revision: None,
            created_at: Some("2".into()),
        }
    }

    fn job(status: AnalysisJobStatus) -> AnalysisJob {
        AnalysisJob {
            id: "job_1".into(),
            status,
            source: AnalysisJobSource {
                kind: AnalysisJobSourceKind::LocalPath,
                display_name: Some("demo".into()),
                path: None,
                repository_url: None,
                git_ref: None,
                commit_sha: None,
            },
            project_name: Some("demo".into()),
            message: Some("queued".into()),
            progress: Some(0),
            requested_analyzers: vec![AnalyzerEngine::Parser],
            analyzer_statuses: Vec::new(),
            created_at: Some("3".into()),
            started_at: None,
            finished_at: None,
            credits_estimated: Some(1),
            credits_used: None,
            error: None,
        }
    }

    fn snapshot() -> GraphSnapshot {
        GraphSnapshot {
            nodes: Vec::new(),
            edges: Vec::new(),
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        }
    }

    fn usage() -> CloudAnalysisUsage {
        CloudAnalysisUsage {
            job_id: "job_1".into(),
            workspace_id: Some("workspace_1".into()),
            revision_id: Some("revision_1".into()),
            input_files: 1,
            input_bytes: 12,
            output_nodes: 0,
            output_edges: 0,
            output_files: 0,
            requested_analyzers: Vec::new(),
            materialization_ms: 1,
            graph_build_ms: 2,
            total_wall_ms: 3,
            credits_estimated: 4,
            credits_used: 4,
            created_at: Some("5".into()),
        }
    }

    #[test]
    fn schema_initialization_succeeds() {
        let store = store();
        assert!(store.load_all().is_ok());
    }

    #[test]
    fn workspace_persistence_roundtrip() {
        let store = store();
        store.save_workspace(&workspace()).unwrap();

        assert!(store
            .load_all()
            .unwrap()
            .workspaces
            .contains_key("workspace_1"));
    }

    #[test]
    fn revision_persistence_roundtrip() {
        let store = store();
        store.save_revision(&revision()).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.revisions["revision_1"].workspace_id, "workspace_1");
    }

    #[test]
    fn blob_metadata_persistence_roundtrip() {
        let store = store();
        let blob = StoredBlob {
            content_hash: "sha256:abc".into(),
            size_bytes: 12,
            storage_path: "/tmp/blob".into(),
            created_at: "3".into(),
        };
        store.save_blob(&blob).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.blobs["sha256:abc"].storage_path, "/tmp/blob");
    }

    #[test]
    fn job_persistence_roundtrip_includes_revision_target() {
        let store = store();
        store
            .save_job(
                &job(AnalysisJobStatus::Queued),
                Some("workspace_1"),
                Some("revision_1"),
            )
            .unwrap();

        let loaded = store.load_all().unwrap();
        assert!(loaded.jobs.contains_key("job_1"));
        assert_eq!(
            loaded.job_revision_targets["job_1"].revision_id,
            "revision_1"
        );
    }

    #[test]
    fn job_status_update_persists() {
        let store = store();
        store
            .save_job(
                &job(AnalysisJobStatus::Queued),
                Some("workspace_1"),
                Some("revision_1"),
            )
            .unwrap();
        let mut completed = job(AnalysisJobStatus::Completed);
        completed.finished_at = Some("4".into());
        store
            .save_job(&completed, Some("workspace_1"), Some("revision_1"))
            .unwrap();

        assert_eq!(
            store.load_all().unwrap().jobs["job_1"].status,
            AnalysisJobStatus::Completed
        );
    }

    #[test]
    fn analysis_result_persistence_roundtrip() {
        let store = store();
        store
            .save_analysis_result(&CloudAnalysisResult {
                job_id: "job_1".into(),
                workspace_id: "workspace_1".into(),
                revision_id: "revision_1".into(),
                snapshot: snapshot(),
                created_at: "4".into(),
            })
            .unwrap();

        assert!(store
            .load_all()
            .unwrap()
            .analysis_results
            .contains_key("job_1"));
    }

    #[test]
    fn usage_persistence_roundtrip() {
        let store = store();
        store.save_usage(&usage()).unwrap();

        assert_eq!(
            store.load_all().unwrap().analysis_usage["job_1"].credits_used,
            4
        );
    }
}

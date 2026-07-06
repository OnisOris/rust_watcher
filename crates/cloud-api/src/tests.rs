use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use graph_core::{
    AnalysisJob, AnalysisJobSource, AnalysisJobStatus, AnalysisMode, AnalyzerEngine,
    AnalyzerServiceStatus, AnalyzerStatus, CloudAnalysisUsage, CloudWorkspace,
    CreateAnalysisJobRequest, CreateWorkspaceRevisionRequest, WorkspaceRevision,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

use crate::auth::{
    cloud_logout, create_auth_session, require_cloud_auth, unix_timestamp, validate_auth_defaults,
    DEFAULT_ADMIN_PASSWORD, DEFAULT_ADMIN_USERNAME, DEFAULT_AUTH_SESSION_TTL_SECONDS,
    DEFAULT_DEV_TOKEN, INTERNAL_API_TOKEN_HEADER,
};
use crate::errors::ApiError;
use crate::ide::SaveWorkspaceFileRequest;
use crate::jobs::{
    cancel_job, cloud_analyze_workspace, cloud_event_visible_to_user, cloud_usage_response,
    create_job, get_job_usage, usage_summary, CloudAnalyzeWorkspaceRequest,
};
use crate::scheduler::{
    requests_rust_analyzer, run_one_queued_job, run_parser_cloud_analysis, JobSchedulerConfig,
};
use crate::state::{
    AuthSession, CloudAnalysisConfig, CloudApiState, CloudEvent, CloudLimits, JobRevisionTarget,
    SelfUpdateConfig,
};
use crate::storage::{CloudMetadataStore, PersistedCloudState};
use crate::workspaces::{
    materialize_revision, materialized_child_path, requested_analyzers_for_workspace_files,
    sha256_content_hash, timestamp, CreateWorkspaceRequest,
};
use graph_core::{
    AnalysisJobSourceKind, AnalyzerProvider, LanguageId, WorkspaceFileEntry,
    WorkspaceSyncPlanRequest,
};
use tower::ServiceExt;

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
    test_state_with_config_scheduler_and_internal_token(
        analysis_config,
        scheduler_config,
        Some("internal-token".into()),
    )
}

fn test_state_without_internal_token() -> CloudApiState {
    test_state_with_config_scheduler_and_internal_token(
        test_analysis_config(),
        JobSchedulerConfig::default(),
        None,
    )
}

fn test_state_with_config_scheduler_and_internal_token(
    analysis_config: CloudAnalysisConfig,
    scheduler_config: JobSchedulerConfig,
    internal_api_token: Option<String>,
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
        internal_api_token,
        test_auth_users(),
        DEFAULT_AUTH_SESSION_TTL_SECONDS,
        "admin".into(),
        test_update_config(),
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

fn test_auth_users() -> HashMap<String, String> {
    HashMap::from([("admin".into(), "dev-password".into())])
}

fn test_update_config() -> SelfUpdateConfig {
    SelfUpdateConfig {
        repository: "OnisOris/rust_watcher".into(),
        asset_prefix: "rust-watcher-cloud-linux-x86_64".into(),
        service_name: "rust-watcher-cloud-api.service".into(),
        app_root: std::env::temp_dir(),
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
        incremental: false,
        base_revision_id: None,
        project_name: Some("demo".into()),
    }
}

fn auth_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    headers
}

fn internal_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        INTERNAL_API_TOKEN_HEADER,
        HeaderValue::from_str(token).unwrap(),
    );
    headers
}

fn workspace_request() -> CreateWorkspaceRequest {
    CreateWorkspaceRequest {
        display_name: "demo".into(),
        owner_username: None,
        source: None,
    }
}

#[test]
fn default_dev_credentials_are_rejected_without_allow_flag() {
    assert!(validate_auth_defaults(
        "",
        DEFAULT_ADMIN_USERNAME,
        DEFAULT_ADMIN_PASSWORD,
        DEFAULT_DEV_TOKEN,
        false,
    )
    .is_err());
    assert!(validate_auth_defaults(
        "admin:dev-password",
        "root",
        "strong-password",
        "non-default-token",
        false,
    )
    .is_err());
    assert!(validate_auth_defaults(
        "",
        DEFAULT_ADMIN_USERNAME,
        DEFAULT_ADMIN_PASSWORD,
        DEFAULT_DEV_TOKEN,
        true,
    )
    .is_ok());
}

#[test]
fn expired_session_is_rejected() {
    let state = test_state();
    let token = "expired-session".to_string();
    state.auth_sessions.write().insert(
        token.clone(),
        AuthSession {
            username: "admin".into(),
            expires_at: unix_timestamp().saturating_sub(1),
        },
    );

    let result = require_cloud_auth(&state, &auth_headers(&token));

    assert!(matches!(result, Err(ApiError::Unauthorized(_))));
    assert!(!state.auth_sessions.read().contains_key(&token));
}

#[tokio::test]
async fn logout_invalidates_session() {
    let state = test_state();
    let token = create_auth_session(&state, "admin".into());
    let headers = auth_headers(&token);

    assert_eq!(
        require_cloud_auth(&state, &headers).unwrap(),
        "admin".to_string()
    );

    let response = cloud_logout(State(state.clone()), headers.clone())
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(matches!(
        require_cloud_auth(&state, &headers),
        Err(ApiError::Unauthorized(_))
    ));
}

#[tokio::test]
async fn legacy_internal_endpoint_forbidden_without_token() {
    let state = test_state();

    let missing_header_response =
        crate::workspaces::list_workspaces(State(state.clone()), HeaderMap::new())
            .await
            .into_response();

    assert_eq!(missing_header_response.status(), StatusCode::FORBIDDEN);

    let disabled_state = test_state_without_internal_token();
    let disabled_response = crate::workspaces::list_workspaces(
        State(disabled_state),
        internal_headers("internal-token"),
    )
    .await
    .into_response();

    assert_eq!(disabled_response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn legacy_internal_endpoint_accepts_internal_token() {
    let state = test_state();

    let header_response = crate::workspaces::list_workspaces(
        State(state.clone()),
        internal_headers("internal-token"),
    )
    .await
    .into_response();
    let bearer_response =
        crate::workspaces::list_workspaces(State(state), auth_headers("internal-token"))
            .await
            .into_response();

    assert_eq!(header_response.status(), StatusCode::OK);
    assert_eq!(bearer_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn cloud_websocket_rejects_missing_token() {
    let state = test_state();
    let response = crate::routes::router()
        .with_state(state)
        .oneshot(
            Request::builder()
                .uri("/api/cloud/ws")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cloud_websocket_rejects_invalid_token() {
    let state = test_state();
    let response = crate::routes::router()
        .with_state(state)
        .oneshot(
            Request::builder()
                .uri("/api/cloud/ws?token=invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cloud_websocket_valid_token_reaches_upgrade_path() {
    let state = test_state();
    let token = create_auth_session(&state, "admin".into());
    let response = crate::routes::router()
        .with_state(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/cloud/ws?token={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn cloud_websocket_events_are_scoped_to_owner() {
    let state = test_state();
    let admin_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "admin-demo".into(),
        owner_username: Some("admin".into()),
        source: None,
    });
    let user_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "user-demo".into(),
        owner_username: Some("user".into()),
        source: None,
    });
    let mut admin_job = terminal_job(AnalysisJobStatus::Queued);
    admin_job.id = "admin-job".into();
    state
        .jobs
        .write()
        .insert(admin_job.id.clone(), admin_job.clone());
    state.job_revision_targets.write().insert(
        admin_job.id.clone(),
        stored_job_target(&admin_workspace.id, "revision"),
    );

    let admin_workspace_event = CloudEvent {
        event_type: "snapshotReady".into(),
        job_id: None,
        workspace_id: Some(admin_workspace.id.clone()),
        status: None,
        progress: None,
        message: None,
    };
    let user_workspace_event = CloudEvent {
        event_type: "snapshotReady".into(),
        job_id: None,
        workspace_id: Some(user_workspace.id),
        status: None,
        progress: None,
        message: None,
    };
    let admin_job_event = CloudEvent {
        event_type: "jobStatus".into(),
        job_id: Some(admin_job.id),
        workspace_id: None,
        status: None,
        progress: None,
        message: None,
    };
    let unsafe_global_event = CloudEvent {
        event_type: "jobStatus".into(),
        job_id: None,
        workspace_id: None,
        status: None,
        progress: None,
        message: Some("not scoped".into()),
    };

    assert!(cloud_event_visible_to_user(
        &state,
        &admin_workspace_event,
        "admin"
    ));
    assert!(!cloud_event_visible_to_user(
        &state,
        &admin_workspace_event,
        "user"
    ));
    assert!(!cloud_event_visible_to_user(
        &state,
        &user_workspace_event,
        "admin"
    ));
    assert!(cloud_event_visible_to_user(
        &state,
        &admin_job_event,
        "admin"
    ));
    assert!(!cloud_event_visible_to_user(
        &state,
        &admin_job_event,
        "user"
    ));
    assert!(!cloud_event_visible_to_user(
        &state,
        &unsafe_global_event,
        "admin"
    ));
}

#[test]
fn cloud_workspace_listing_is_scoped_to_owner() {
    let state = test_state();
    let admin_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "admin-demo".into(),
        owner_username: Some("admin".into()),
        source: None,
    });
    let user_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "user-demo".into(),
        owner_username: Some("user".into()),
        source: None,
    });
    let legacy_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "legacy-demo".into(),
        owner_username: None,
        source: None,
    });

    let admin_ids = state
        .list_workspaces_for_user("admin")
        .into_iter()
        .map(|workspace| workspace.id)
        .collect::<HashSet<_>>();
    let user_ids = state
        .list_workspaces_for_user("user")
        .into_iter()
        .map(|workspace| workspace.id)
        .collect::<HashSet<_>>();

    assert!(admin_ids.contains(&admin_workspace.id));
    assert!(admin_ids.contains(&legacy_workspace.id));
    assert!(!admin_ids.contains(&user_workspace.id));
    assert_eq!(user_ids, HashSet::from([user_workspace.id]));
}

#[test]
fn cloud_usage_is_scoped_to_owner() {
    let state = test_state();
    let admin_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "admin-demo".into(),
        owner_username: Some("admin".into()),
        source: None,
    });
    let user_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "user-demo".into(),
        owner_username: Some("user".into()),
        source: None,
    });
    state.analysis_usage.write().insert(
        "admin-job".into(),
        CloudAnalysisUsage {
            job_id: "admin-job".into(),
            workspace_id: Some(admin_workspace.id),
            revision_id: None,
            input_files: 1,
            input_bytes: 10,
            output_nodes: 1,
            output_edges: 0,
            output_files: 1,
            requested_analyzers: Vec::new(),
            materialization_ms: 1,
            graph_build_ms: 1,
            total_wall_ms: 2,
            credits_estimated: 7,
            credits_used: 7,
            created_at: Some("2".into()),
        },
    );
    state.analysis_usage.write().insert(
        "user-job".into(),
        CloudAnalysisUsage {
            job_id: "user-job".into(),
            workspace_id: Some(user_workspace.id),
            revision_id: None,
            input_files: 1,
            input_bytes: 10,
            output_nodes: 1,
            output_edges: 0,
            output_files: 1,
            requested_analyzers: Vec::new(),
            materialization_ms: 1,
            graph_build_ms: 1,
            total_wall_ms: 2,
            credits_estimated: 3,
            credits_used: 3,
            created_at: Some("1".into()),
        },
    );

    let admin_usage = cloud_usage_response(&state, "admin");
    let user_usage = cloud_usage_response(&state, "user");

    assert_eq!(admin_usage.credits_used, 7);
    assert_eq!(admin_usage.jobs.len(), 1);
    assert_eq!(user_usage.credits_used, 3);
    assert_eq!(user_usage.jobs.len(), 1);
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

#[test]
fn requested_analyzers_include_parser_baseline() {
    assert_eq!(
        requested_analyzers_for_workspace_files(&[file_entry_at("README.md", b"# demo")]),
        vec![AnalyzerEngine::Parser]
    );
}

#[test]
fn requested_analyzers_select_rust_from_manifest_or_sources() {
    assert_eq!(
        requested_analyzers_for_workspace_files(&[file_entry_at("src/lib.rs", b"")]),
        vec![AnalyzerEngine::Parser, AnalyzerEngine::RustAnalyzer]
    );
    assert_eq!(
        requested_analyzers_for_workspace_files(&[file_entry_at("crates/app/Cargo.toml", b"")]),
        vec![AnalyzerEngine::Parser, AnalyzerEngine::RustAnalyzer]
    );
}

#[test]
fn requested_analyzers_select_python_typescript_and_qml() {
    assert_eq!(
        requested_analyzers_for_workspace_files(&[file_entry_at("app/main.py", b"")]),
        vec![AnalyzerEngine::Parser, AnalyzerEngine::Ty]
    );
    assert_eq!(
        requested_analyzers_for_workspace_files(&[file_entry_at("frontend/package.json", b"{}")]),
        vec![
            AnalyzerEngine::Parser,
            AnalyzerEngine::TypeScriptLanguageServer
        ]
    );
    assert_eq!(
        requested_analyzers_for_workspace_files(&[file_entry_at("ui/App.qml", b"")]),
        vec![AnalyzerEngine::Parser, AnalyzerEngine::QmlLanguageServer]
    );
}

#[test]
fn requested_analyzers_keep_stable_mixed_order() {
    assert_eq!(
        requested_analyzers_for_workspace_files(&[
            file_entry_at("src/main.rs", b""),
            file_entry_at("scripts/main.py", b""),
            file_entry_at("web/app.tsx", b""),
            file_entry_at("qml/Main.qml", b""),
        ]),
        vec![
            AnalyzerEngine::Parser,
            AnalyzerEngine::RustAnalyzer,
            AnalyzerEngine::Ty,
            AnalyzerEngine::TypeScriptLanguageServer,
            AnalyzerEngine::QmlLanguageServer,
        ]
    );
}

fn terminal_job(status: AnalysisJobStatus) -> AnalysisJob {
    AnalysisJob {
        id: Uuid::new_v4().to_string(),
        status,
        source: local_request().source.unwrap(),
        project_name: Some("demo".into()),
        message: Some("terminal".into()),
        progress: Some(100),
        analysis_mode: AnalysisMode::Full,
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

fn stored_job_target(workspace_id: &str, revision_id: &str) -> JobRevisionTarget {
    JobRevisionTarget {
        workspace_id: workspace_id.into(),
        revision_id: revision_id.into(),
        base_revision_id: None,
        incremental: false,
        changed_files: Vec::new(),
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

#[test]
fn save_workspace_file_creates_new_revision_and_reads_content() {
    let state = test_state();
    let (workspace, revision) = create_rust_revision(&state);

    let response = state
        .save_workspace_file(
            &workspace.id,
            SaveWorkspaceFileRequest {
                path: "src/main.rs".into(),
                content: "fn main() { println!(\"hi\"); }\n".into(),
                base_revision: Some(revision.id.clone()),
            },
        )
        .unwrap();

    assert_ne!(response.revision_id, revision.id);
    assert_eq!(response.file.path, "src/main.rs");
    assert_eq!(response.files_count, 2);

    let content = state
        .workspace_file_content(&workspace.id, "src/main.rs", Some(&response.revision_id))
        .unwrap();
    assert_eq!(content.content, "fn main() { println!(\"hi\"); }\n");
    assert_eq!(content.revision_id, response.revision_id);
    assert_eq!(
        state.get_workspace(&workspace.id).unwrap().current_revision,
        Some(response.revision_id)
    );
}

#[test]
fn save_workspace_file_rejects_stale_base_revision() {
    let state = test_state();
    let (workspace, revision) = create_rust_revision(&state);
    state
        .save_workspace_file(
            &workspace.id,
            SaveWorkspaceFileRequest {
                path: "src/main.rs".into(),
                content: "fn main() {}\n".into(),
                base_revision: Some(revision.id.clone()),
            },
        )
        .unwrap();

    let error = state
        .save_workspace_file(
            &workspace.id,
            SaveWorkspaceFileRequest {
                path: "src/main.rs".into(),
                content: "fn stale() {}\n".into(),
                base_revision: Some(revision.id),
            },
        )
        .unwrap_err();

    assert_eq!(
        error,
        ApiError::Conflict("workspace revision changed".into())
    );
}

#[test]
fn workspace_revision_diff_reports_added_removed_modified_and_unchanged() {
    let state = test_state();
    let workspace = state.create_workspace(workspace_request());
    let unchanged = file_entry_at("src/unchanged.rs", b"pub fn same() {}\n");
    let old_modified = file_entry_at("src/modified.rs", b"pub fn value() -> i32 { 1 }\n");
    let removed = file_entry_at("src/removed.rs", b"pub fn removed() {}\n");
    let new_modified = file_entry_at("src/modified.rs", b"pub fn value() -> i32 { 2 }\n");
    let added = file_entry_at("src/added.rs", b"pub fn added() {}\n");
    for (entry, content) in [
        (&unchanged, b"pub fn same() {}\n".as_slice()),
        (&old_modified, b"pub fn value() -> i32 { 1 }\n".as_slice()),
        (&removed, b"pub fn removed() {}\n".as_slice()),
        (&new_modified, b"pub fn value() -> i32 { 2 }\n".as_slice()),
        (&added, b"pub fn added() {}\n".as_slice()),
    ] {
        state
            .upload_blob(&workspace.id, &entry.content_hash, content)
            .unwrap();
    }
    let base = state
        .create_revision(
            &workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: None,
                files: vec![unchanged.clone(), old_modified.clone(), removed.clone()],
            },
        )
        .unwrap()
        .revision;
    let head = state
        .create_revision(
            &workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: Some(base.id.clone()),
                files: vec![unchanged.clone(), new_modified.clone(), added.clone()],
            },
        )
        .unwrap()
        .revision;

    let diff = state
        .workspace_revision_diff(&workspace.id, &base.id, Some(&head.id))
        .unwrap();
    let current_head_diff = state
        .workspace_revision_diff(&workspace.id, &base.id, None)
        .unwrap();

    assert_eq!(diff.added_files.len(), 1);
    assert_eq!(current_head_diff.head_revision_id, head.id);
    assert_eq!(diff.added_files[0].path, "src/added.rs");
    assert_eq!(diff.removed_files.len(), 1);
    assert_eq!(diff.removed_files[0].path, "src/removed.rs");
    assert_eq!(diff.modified_files.len(), 1);
    assert_eq!(diff.modified_files[0].path, "src/modified.rs");
    assert_eq!(diff.unchanged_count, 1);
    assert_eq!(
        diff.modified_files[0].old_content_hash.as_deref(),
        Some(old_modified.content_hash.as_str())
    );
    assert_eq!(
        diff.modified_files[0].new_content_hash.as_deref(),
        Some(new_modified.content_hash.as_str())
    );

    let file_diff = state
        .workspace_file_diff(&workspace.id, "src/modified.rs", &base.id, Some(&head.id))
        .unwrap();

    assert_eq!(file_diff.old_size_bytes, Some(old_modified.size_bytes));
    assert_eq!(file_diff.new_size_bytes, Some(new_modified.size_bytes));
    assert_eq!(
        file_diff.old_content_hash.as_deref(),
        Some(old_modified.content_hash.as_str())
    );
    assert_eq!(
        file_diff.new_content_hash.as_deref(),
        Some(new_modified.content_hash.as_str())
    );
    assert!(!file_diff.truncated);
    let unified = file_diff.unified_diff.expect("unified diff");
    assert!(unified.contains("-pub fn value() -> i32 { 1 }"));
    assert!(unified.contains("+pub fn value() -> i32 { 2 }"));
}

#[tokio::test]
async fn workspace_revision_diff_requires_workspace_owner() {
    let state = test_state();
    let (workspace, revision) = create_rust_revision(&state);
    let user_workspace = state.create_workspace(CreateWorkspaceRequest {
        display_name: "user-demo".into(),
        owner_username: Some("user".into()),
        source: None,
    });
    let user_file = file_entry_at("src/lib.rs", b"pub fn user() {}\n");
    state
        .upload_blob(
            &user_workspace.id,
            &user_file.content_hash,
            b"pub fn user() {}\n",
        )
        .unwrap();
    state
        .create_revision(
            &user_workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: None,
                files: vec![user_file],
            },
        )
        .unwrap();
    let token = create_auth_session(&state, "admin".into());

    let response = crate::ide::cloud_workspace_diff(
        State(state),
        auth_headers(&token),
        AxumPath(user_workspace.id),
        Query(crate::ide::WorkspaceRevisionDiffQuery {
            base_revision_id: revision.id,
            head_revision_id: Some(workspace.current_revision.unwrap_or_default()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cloud_workspace_analyze_without_body_queues_full_job() {
    let state = test_state();
    let (workspace, _revision) = create_rust_revision(&state);
    let token = create_auth_session(&state, "admin".into());

    let response = cloud_analyze_workspace(
        State(state.clone()),
        auth_headers(&token),
        AxumPath(workspace.id),
        None,
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let job = state
        .list_jobs()
        .into_iter()
        .next()
        .expect("queued analysis job");
    assert_eq!(job.analysis_mode, AnalysisMode::Full);
    assert_eq!(
        job.requested_analyzers,
        vec![AnalyzerEngine::Parser, AnalyzerEngine::RustAnalyzer]
    );
}

#[tokio::test]
async fn optional_cloud_analyzers_fallback_without_failing_job() {
    let state = test_state();
    let workspace = state.create_workspace(workspace_request());
    let files = [
        ("scripts/main.py", b"print('hi')\n".as_slice()),
        (
            "frontend/app.tsx",
            b"export const App = () => null;\n".as_slice(),
        ),
        ("qml/Main.qml", b"import QtQuick\nItem {}\n".as_slice()),
    ];
    let entries = files
        .iter()
        .map(|(path, content)| {
            let entry = file_entry_at(path, content);
            state
                .upload_blob(&workspace.id, &entry.content_hash, content)
                .unwrap();
            entry
        })
        .collect::<Vec<_>>();
    state
        .create_revision(
            &workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: None,
                files: entries,
            },
        )
        .unwrap();
    let token = create_auth_session(&state, "admin".into());

    let response = cloud_analyze_workspace(
        State(state.clone()),
        auth_headers(&token),
        AxumPath(workspace.id),
        None,
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let job = state
        .list_jobs()
        .into_iter()
        .next()
        .expect("queued analysis job");
    assert_eq!(
        job.requested_analyzers,
        vec![
            AnalyzerEngine::Parser,
            AnalyzerEngine::Ty,
            AnalyzerEngine::TypeScriptLanguageServer,
            AnalyzerEngine::QmlLanguageServer,
        ]
    );

    run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

    let completed = state.get_job(&job.id).unwrap();
    assert_eq!(completed.status, AnalysisJobStatus::Completed);
    for analyzer in [
        AnalyzerEngine::Ty,
        AnalyzerEngine::TypeScriptLanguageServer,
        AnalyzerEngine::QmlLanguageServer,
    ] {
        let status = completed
            .analyzer_statuses
            .iter()
            .find(|status| status.engine == analyzer)
            .expect("optional analyzer status");
        assert_eq!(status.provider, AnalyzerProvider::Cloud);
        assert_eq!(status.status, AnalyzerStatus::Fallback);
        assert!(!status.billable);
    }
}

#[tokio::test]
async fn parser_analysis_cache_reuses_unchanged_file_between_revisions() {
    let state = test_state();
    let workspace = state.create_workspace(workspace_request());
    let content = b"def value():\n    return 1\n";
    let file = file_entry_at("app/main.py", content);
    state
        .upload_blob(&workspace.id, &file.content_hash, content)
        .unwrap();
    let first_revision = state
        .create_revision(
            &workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: None,
                files: vec![file.clone()],
            },
        )
        .unwrap()
        .revision;
    let first_job = state
        .create_job(workspace_job_request(&workspace, &first_revision))
        .unwrap();

    run_parser_cloud_analysis(state.clone(), first_job.id.clone()).await;

    assert_eq!(
        state.file_analysis_cache.read().last_metrics,
        crate::state::FileAnalysisCacheMetrics {
            hits: 0,
            misses: 1,
            reused_files: 0,
        }
    );
    let second_revision = state
        .create_revision(
            &workspace.id,
            CreateWorkspaceRevisionRequest {
                base_revision: Some(first_revision.id),
                files: vec![file],
            },
        )
        .unwrap()
        .revision;
    let second_job = state
        .create_job(workspace_job_request(&workspace, &second_revision))
        .unwrap();

    run_parser_cloud_analysis(state.clone(), second_job.id.clone()).await;

    assert_eq!(
        state.file_analysis_cache.read().last_metrics,
        crate::state::FileAnalysisCacheMetrics {
            hits: 1,
            misses: 0,
            reused_files: 1,
        }
    );
    assert_eq!(
        state.get_job(&second_job.id).unwrap().status,
        AnalysisJobStatus::Completed
    );
}

#[tokio::test]
async fn incremental_workspace_analysis_falls_back_to_full_when_fast_path_unavailable() {
    let state = test_state();
    let (workspace, base_revision) = create_rust_revision(&state);
    let base_job = state
        .create_job_for_request(workspace_job_request(&workspace, &base_revision))
        .unwrap();
    run_parser_cloud_analysis(state.clone(), base_job.id.clone()).await;
    let save = state
        .save_workspace_file(
            &workspace.id,
            SaveWorkspaceFileRequest {
                path: "src/main.rs".into(),
                content: "fn main() { println!(\"incremental\"); }\n".into(),
                base_revision: Some(base_revision.id.clone()),
            },
        )
        .unwrap();
    let token = create_auth_session(&state, "admin".into());

    let response = cloud_analyze_workspace(
        State(state.clone()),
        auth_headers(&token),
        AxumPath(workspace.id.clone()),
        Some(Json(CloudAnalyzeWorkspaceRequest {
            incremental: true,
            base_revision_id: Some(base_revision.id.clone()),
        })),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let job = state
        .list_jobs()
        .into_iter()
        .find(|job| job.id != base_job.id)
        .expect("incremental queued job");
    assert_eq!(job.analysis_mode, AnalysisMode::Incremental);
    let target = state.get_job_revision_target(&job.id).unwrap();
    assert!(target.incremental);
    assert_eq!(
        target.base_revision_id.as_deref(),
        Some(base_revision.id.as_str())
    );

    run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

    let completed = state.get_job(&job.id).unwrap();
    assert_eq!(completed.analysis_mode, AnalysisMode::FallbackFull);
    assert_eq!(
        state
            .get_job_revision_target(&job.id)
            .unwrap()
            .changed_files,
        vec!["src/main.rs".to_string()]
    );
    let result = state
        .analysis_results
        .read()
        .get(&job.id)
        .cloned()
        .expect("analysis result");
    assert_eq!(result.revision_id, save.revision_id);
    assert!(!result.snapshot.nodes.is_empty());
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
        incremental: false,
        base_revision_id: None,
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
    let target = stored_job_target("workspace_1", "revision_1");
    store
        .save_job_with_target(&running_job, Some(&target))
        .unwrap();
    let persisted = store.load_all().unwrap();

    let state = CloudApiState::from_persisted(
        blobs_dir,
        workspaces_dir,
        test_analysis_config(),
        test_cloud_limits(),
        "dev-token".into(),
        Some("internal-token".into()),
        test_auth_users(),
        DEFAULT_AUTH_SESSION_TTL_SECONDS,
        "admin".into(),
        test_update_config(),
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
            incremental: false,
            base_revision_id: None,
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
    let state = test_state_with_scheduler_config(JobSchedulerConfig::new(1, 1).expect("config"));
    let (workspace, revision) = create_rust_revision(&state);
    let first_response = create_job(
        State(state.clone()),
        internal_headers("internal-token"),
        Json(workspace_job_request(&workspace, &revision)),
    )
    .await
    .into_response();
    let second_response = create_job(
        State(state.clone()),
        internal_headers("internal-token"),
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

    let response = cancel_job(
        State(state.clone()),
        internal_headers("internal-token"),
        AxumPath(job.id.clone()),
    )
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
        owner_username: None,
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
    let target = stored_job_target(&workspace.id, &revision.id);
    store
        .save_job_with_target(&queued_job, Some(&target))
        .unwrap();
    let persisted = store.load_all().unwrap();

    let state = CloudApiState::from_persisted(
        blobs_dir,
        workspaces_dir,
        test_analysis_config(),
        test_cloud_limits(),
        "dev-token".into(),
        Some("internal-token".into()),
        test_auth_users(),
        DEFAULT_AUTH_SESSION_TTL_SECONDS,
        "admin".into(),
        test_update_config(),
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
        owner_username: None,
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
    let target = stored_job_target(&workspace.id, &revision.id);
    store
        .save_job_with_target(&running_job, Some(&target))
        .unwrap();
    let persisted = store.load_all().unwrap();

    let state = CloudApiState::from_persisted(
        blobs_dir,
        workspaces_dir,
        test_analysis_config(),
        test_cloud_limits(),
        "dev-token".into(),
        Some("internal-token".into()),
        test_auth_users(),
        DEFAULT_AUTH_SESSION_TTL_SECONDS,
        "admin".into(),
        test_update_config(),
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
    let state = test_state_with_scheduler_config(JobSchedulerConfig::new(1, 10).expect("config"));
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
            incremental: false,
            base_revision_id: None,
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
            incremental: false,
            base_revision_id: None,
            project_name: None,
        })
        .unwrap();
    let rust_analyzer_job = state
        .create_job(CreateAnalysisJobRequest {
            source: None,
            requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
            workspace_id: Some(workspace.id),
            revision_id: Some(revision.id),
            incremental: false,
            base_revision_id: None,
            project_name: None,
        })
        .unwrap();

    assert!(rust_analyzer_job.credits_estimated.unwrap() > parser_job.credits_estimated.unwrap());
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
            incremental: false,
            base_revision_id: None,
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
            incremental: false,
            base_revision_id: None,
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
            incremental: false,
            base_revision_id: None,
            project_name: None,
        })
        .unwrap();

    let response = get_job_usage(
        State(state),
        internal_headers("internal-token"),
        AxumPath(job.id),
    )
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
            incremental: false,
            base_revision_id: None,
            project_name: None,
        })
        .unwrap();
    run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

    let response = get_job_usage(
        State(state),
        internal_headers("internal-token"),
        AxumPath(job.id),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn usage_endpoint_reports_missing_and_failed_jobs() {
    let state = test_state();
    let missing_response = get_job_usage(
        State(state.clone()),
        internal_headers("internal-token"),
        AxumPath("missing".into()),
    )
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
            incremental: false,
            base_revision_id: None,
            project_name: None,
        })
        .unwrap();
    run_parser_cloud_analysis(state.clone(), job.id.clone()).await;

    let failed_response = get_job_usage(
        State(state),
        internal_headers("internal-token"),
        AxumPath(job.id),
    )
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
            incremental: false,
            base_revision_id: None,
            project_name: None,
        })
        .unwrap();
    run_parser_cloud_analysis(state.clone(), job.id).await;

    let Json(summary) = usage_summary(State(state), internal_headers("internal-token"))
        .await
        .unwrap();

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
            incremental: false,
            base_revision_id: None,
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

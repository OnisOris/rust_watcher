use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::info;

mod agent;
mod auth;
mod errors;
mod ide;
mod imports;
mod jobs;
mod routes;
mod scheduler;
mod self_update;
mod state;
mod storage;
mod workspaces;

use auth::{
    parse_auth_users, validate_auth_defaults, DEFAULT_ADMIN_PASSWORD, DEFAULT_ADMIN_USERNAME,
    DEFAULT_AUTH_SESSION_TTL_SECONDS, DEFAULT_DEV_TOKEN,
};
use scheduler::{start_analysis_workers, JobSchedulerConfig};
use state::{CloudAnalysisConfig, CloudApiState, CloudLimits, SelfUpdateConfig};
use storage::CloudMetadataStore;

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
pub(crate) struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: IpAddr,
    #[arg(long, default_value_t = 8080)]
    pub(crate) port: u16,
    #[arg(long, default_value = ".rust-watcher-cloud/blobs")]
    pub(crate) blobs_dir: PathBuf,
    #[arg(long, default_value = ".rust-watcher-cloud/workspaces")]
    pub(crate) workspaces_dir: PathBuf,
    #[arg(long, default_value = ".rust-watcher-cloud/cloud-api.sqlite")]
    pub(crate) db_path: PathBuf,
    #[arg(long, default_value = "frontend/dist")]
    pub(crate) frontend_dist: PathBuf,
    #[arg(long, default_value = "rust-analyzer")]
    pub(crate) rust_analyzer: PathBuf,
    #[arg(long, default_value_t = 120)]
    pub(crate) analysis_timeout_seconds: u64,
    #[arg(long, default_value_t = 3)]
    pub(crate) lsp_file_timeout_seconds: u64,
    #[arg(long, default_value_t = 2)]
    pub(crate) max_concurrent_jobs: usize,
    #[arg(long, default_value_t = 100)]
    pub(crate) max_queued_jobs: usize,
    #[arg(long, env = "RUST_WATCHER_DEV_TOKEN", default_value = DEFAULT_DEV_TOKEN)]
    pub(crate) dev_token: String,
    #[arg(long, env = "RUST_WATCHER_INTERNAL_API_TOKEN")]
    pub(crate) internal_api_token: Option<String>,
    #[arg(long, env = "RUST_WATCHER_ADMIN_USERNAME", default_value = DEFAULT_ADMIN_USERNAME)]
    pub(crate) admin_username: String,
    #[arg(
        long,
        env = "RUST_WATCHER_ADMIN_PASSWORD",
        default_value = DEFAULT_ADMIN_PASSWORD
    )]
    pub(crate) admin_password: String,
    #[arg(long, env = "RUST_WATCHER_USERS", default_value = "")]
    pub(crate) users: String,
    #[arg(
        long,
        env = "RUST_WATCHER_ALLOW_INSECURE_DEV_AUTH",
        default_value_t = false
    )]
    pub(crate) allow_insecure_dev_auth: bool,
    #[arg(
        long,
        env = "RUST_WATCHER_AUTH_SESSION_TTL_SECONDS",
        default_value_t = DEFAULT_AUTH_SESSION_TTL_SECONDS
    )]
    pub(crate) auth_session_ttl_seconds: u64,
    #[arg(
        long,
        env = "RUST_WATCHER_UPDATE_REPOSITORY",
        default_value = "OnisOris/rust_watcher"
    )]
    pub(crate) update_repository: String,
    #[arg(
        long,
        env = "RUST_WATCHER_UPDATE_ASSET_PREFIX",
        default_value = "rust-watcher-cloud-linux-x86_64"
    )]
    pub(crate) update_asset_prefix: String,
    #[arg(
        long,
        env = "RUST_WATCHER_UPDATE_SERVICE",
        default_value = "rust-watcher-cloud-api.service"
    )]
    pub(crate) update_service: String,
    #[arg(long, env = "RUST_WATCHER_MAX_UPLOAD_MB", default_value_t = 200)]
    pub(crate) max_upload_mb: u64,
    #[arg(long, env = "RUST_WATCHER_MAX_FILES", default_value_t = 20_000)]
    pub(crate) max_files: usize,
    #[arg(long, default_value_t = 20)]
    pub(crate) max_file_mb: u64,
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
    let app_root = std::env::current_dir().context("failed to resolve app root")?;
    std::fs::create_dir_all(&args.blobs_dir)
        .with_context(|| format!("failed to create {}", args.blobs_dir.display()))?;
    std::fs::create_dir_all(&args.workspaces_dir)
        .with_context(|| format!("failed to create {}", args.workspaces_dir.display()))?;
    let store = CloudMetadataStore::open(args.db_path.clone())?;
    store.init_schema()?;
    let persisted = store.load_all()?;
    validate_auth_defaults(
        &args.users,
        &args.admin_username,
        &args.admin_password,
        &args.dev_token,
        args.allow_insecure_dev_auth,
    )?;
    let auth_users = parse_auth_users(&args.users, &args.admin_username, &args.admin_password)?;
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
        args.internal_api_token
            .as_ref()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty()),
        auth_users,
        args.auth_session_ttl_seconds,
        args.admin_username.clone(),
        SelfUpdateConfig {
            repository: args.update_repository.clone(),
            asset_prefix: args.update_asset_prefix.clone(),
            service_name: args.update_service.clone(),
            app_root,
        },
        store,
        scheduler_config,
        persisted,
    )?;
    start_analysis_workers(state.clone());
    let app = routes::router()
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

#[cfg(test)]
mod tests;

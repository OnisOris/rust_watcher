use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::IpAddr;
use std::path::PathBuf;

mod analysis;
mod analyzer_paths;
mod python_ty;
mod qml_lsp;
mod routes;
mod server;
mod services;
mod state;
mod typescript_lsp;

pub(crate) use server::{
    analysis_event, fallback_status, publish_snapshot, ready_status, update_status,
};
pub(crate) use services::diagnostics::apply_lsp_diagnostics;
#[cfg(test)]
pub(crate) use services::diagnostics::diagnostic_from_lsp_with_language;
pub(crate) use state::{AnalyzerState, AppStateHandle};

#[derive(Parser)]
#[command(name = "rust-code-command-center")]
#[command(about = "Local browser command center for Rust project graphs")]
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
    #[arg(long)]
    pub(crate) project: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: IpAddr,
    #[arg(long, default_value_t = 0)]
    pub(crate) port: u16,
    #[arg(long)]
    pub(crate) open: bool,
    #[arg(long, default_value = "frontend/dist")]
    pub(crate) frontend_dist: PathBuf,
    #[arg(long, default_value = "rust-analyzer")]
    pub(crate) rust_analyzer: PathBuf,
    #[arg(long)]
    pub(crate) enable_editor_open: bool,
    #[arg(long, value_enum, default_value_t = python_ty::PythonAnalyzerMode::Auto)]
    pub(crate) python_analyzer: python_ty::PythonAnalyzerMode,
    #[arg(long, default_value = "ty")]
    pub(crate) ty_path: PathBuf,
    #[arg(long)]
    pub(crate) disable_ty: bool,
    #[arg(long, value_enum, default_value_t = typescript_lsp::TypeScriptAnalyzerMode::Auto)]
    pub(crate) typescript_analyzer: typescript_lsp::TypeScriptAnalyzerMode,
    #[arg(long, default_value = "typescript-language-server")]
    pub(crate) typescript_language_server_path: PathBuf,
    #[arg(long)]
    pub(crate) disable_typescript_language_server: bool,
    #[arg(long, value_enum, default_value_t = qml_lsp::QmlAnalyzerMode::Auto)]
    pub(crate) qml_analyzer: qml_lsp::QmlAnalyzerMode,
    #[arg(long, default_value = "qmlls")]
    pub(crate) qmlls_path: PathBuf,
    #[arg(long)]
    pub(crate) disable_qmlls: bool,
    #[arg(long)]
    pub(crate) qmlls_build_dir: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    pub(crate) qmlls_no_cmake_calls: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "web_server=info,ra_client=info,project_indexer=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve(args) => server::serve(args).await,
    }
}

#[cfg(test)]
mod tests;

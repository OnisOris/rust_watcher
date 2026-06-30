use anyhow::{bail, Context, Result};
use base64::Engine as _;
use clap::{Parser, Subcommand};
use graph_core::{AnalysisJobStatus, LanguageId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use url::Url;

#[derive(Debug, Parser)]
#[command(name = "local-agent")]
#[command(about = "Connect a local project to rust_watcher cloud mode")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Connect(ConnectArgs),
}

#[derive(Debug, Parser)]
struct ConnectArgs {
    #[arg(long)]
    project: PathBuf,
    #[arg(long)]
    server: Url,
    #[arg(long)]
    token: String,
    #[arg(long, default_value_t = 300)]
    timeout_seconds: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionRequest {
    project_name: String,
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionResponse {
    session_id: String,
    workspace_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadFilesRequest {
    files: Vec<UploadFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadFile {
    path: String,
    language: Option<LanguageId>,
    content_base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeResponse {
    workspace_id: String,
    job_id: String,
    status: String,
    session_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobResponse {
    job_id: String,
    status: AnalysisJobStatus,
    message: Option<String>,
    progress: Option<f32>,
    credits_used: Option<u32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Connect(args) => connect(args).await,
    }
}

async fn connect(args: ConnectArgs) -> Result<()> {
    let project = args
        .project
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", args.project.display()))?;
    if !project.is_dir() {
        bail!("project path is not a directory: {}", project.display());
    }

    let client = reqwest::Client::new();
    let project_name = project
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();
    let session: CreateSessionResponse = post_json(
        &client,
        args.server.clone(),
        "api/cloud/agent/sessions",
        &CreateSessionRequest {
            project_name,
            token: args.token,
        },
    )
    .await?;

    let files = collect_files(&project)?;
    println!(
        "Session: {} · Workspace: {} · uploading {} files",
        session.session_id,
        session.workspace_id,
        files.len()
    );
    for chunk in files.chunks(100) {
        post_empty(
            &client,
            args.server.clone(),
            &format!("api/cloud/agent/sessions/{}/files", session.session_id),
            &UploadFilesRequest {
                files: chunk.to_vec(),
            },
        )
        .await?;
    }

    let analysis: AnalyzeResponse = post_json(
        &client,
        args.server.clone(),
        &format!("api/cloud/agent/sessions/{}/analyze", session.session_id),
        &serde_json::json!({}),
    )
    .await?;
    println!(
        "Analysis queued: job={} workspace={} status={}",
        analysis.job_id, analysis.workspace_id, analysis.status
    );
    let workspace_url = workspace_url(&args.server, &analysis.workspace_id);
    println!("Workspace URL: {workspace_url}");

    let started = Instant::now();
    loop {
        let job: JobResponse = get_json(
            &client,
            args.server.clone(),
            &format!("api/cloud/jobs/{}", analysis.job_id),
            analysis.session_token.as_deref(),
        )
        .await?;
        println!(
            "Job {}: {:?} {}{}",
            job.job_id,
            job.status,
            job.message.unwrap_or_default(),
            job.progress
                .map(|progress| format!(" ({:.0}%)", progress * 100.0))
                .unwrap_or_default()
        );
        match job.status {
            AnalysisJobStatus::Completed => {
                println!(
                    "Completed. Credits used: {}",
                    job.credits_used.unwrap_or_default()
                );
                println!("Open workspace: {workspace_url}");
                return Ok(());
            }
            AnalysisJobStatus::Failed | AnalysisJobStatus::Cancelled => {
                bail!("analysis ended with status {:?}", job.status)
            }
            _ => {}
        }
        if started.elapsed() >= Duration::from_secs(args.timeout_seconds) {
            bail!("timed out waiting for analysis job");
        }
        sleep(Duration::from_secs(2)).await;
    }
}

fn workspace_url(server: &Url, workspace_id: &str) -> String {
    let mut url = server.clone();
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    url.query_pairs_mut()
        .append_pair("mode", "cloud")
        .append_pair("workspace", workspace_id);
    url.to_string()
}

async fn get_json<T>(
    client: &reqwest::Client,
    server: Url,
    path: &str,
    bearer_token: Option<&str>,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut request = client.get(server.join(path)?);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await?;
    ensure_success(response)
        .await?
        .json()
        .await
        .context("invalid JSON")
}

async fn post_json<T, B>(client: &reqwest::Client, server: Url, path: &str, body: &B) -> Result<T>
where
    T: serde::de::DeserializeOwned,
    B: Serialize + ?Sized,
{
    let response = client.post(server.join(path)?).json(body).send().await?;
    ensure_success(response)
        .await?
        .json()
        .await
        .context("invalid JSON")
}

async fn post_empty<B>(client: &reqwest::Client, server: Url, path: &str, body: &B) -> Result<()>
where
    B: Serialize + ?Sized,
{
    let response = client.post(server.join(path)?).json(body).send().await?;
    ensure_success(response).await.map(|_| ())
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    bail!("server returned {status}: {text}")
}

fn collect_files(root: &Path) -> Result<Vec<UploadFile>> {
    let mut files = Vec::new();
    collect_files_inner(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_files_inner(root: &Path, current: &Path, files: &mut Vec<UploadFile>) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if should_ignore(relative) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_files_inner(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() || !is_sync_file(&path) {
            continue;
        }
        let bytes = fs::read(&path)?;
        files.push(UploadFile {
            path: project_indexer::relative_to(root, &path),
            language: language_for_path(&path),
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }
    Ok(())
}

fn should_ignore(path: &Path) -> bool {
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
    }) || matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("png" | "jpg" | "jpeg" | "gif" | "mp4" | "zip" | "tar" | "gz")
    )
}

fn is_sync_file(path: &Path) -> bool {
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

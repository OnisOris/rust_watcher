use anyhow::{Context, Result};
use clap::Parser;
use cloud_client::{CloudClient, CloudClientConfig, SyncProjectRequest};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Parser)]
#[command(name = "cloud-sync")]
#[command(about = "Sync a local project directory to a rust_watcher cloud-api workspace")]
struct Args {
    #[arg(long)]
    cloud_url: Url,
    #[arg(long)]
    project: PathBuf,
    #[arg(long)]
    workspace_id: Option<String>,
    #[arg(long)]
    display_name: Option<String>,
    #[arg(long)]
    base_revision: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = CloudClient::new(CloudClientConfig {
        base_url: args.cloud_url,
    });
    let result = client
        .sync_project(SyncProjectRequest {
            project_root: args
                .project
                .canonicalize()
                .with_context(|| format!("failed to canonicalize {}", args.project.display()))?,
            workspace_id: args.workspace_id,
            display_name: args.display_name,
            base_revision: args.base_revision,
        })
        .await?;

    println!("Workspace: {}", result.workspace.id);
    println!("Revision: {}", result.revision.id);
    println!("Files: {}", result.files_count);
    println!("Uploaded blobs: {}", result.uploaded_blobs);
    println!("Skipped blobs: {}", result.skipped_blobs);
    println!("Total bytes: {}", human_bytes(result.total_bytes));
    println!("Uploaded bytes: {}", human_bytes(result.uploaded_bytes));
    Ok(())
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

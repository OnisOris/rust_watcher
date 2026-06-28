use anyhow::{Context, Result};
use clap::Parser;
use cloud_client::{
    parse_analyzers, write_pretty_json, AnalyzeProjectRequest, CloudClient, CloudClientConfig,
    WaitForAnalysisOptions,
};
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

#[derive(Debug, Parser)]
#[command(name = "cloud-analyze")]
#[command(about = "Sync a local project and run cloud analysis")]
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
    #[arg(long = "analyzer")]
    analyzers: Vec<String>,
    #[arg(long, default_value_t = 2)]
    poll_interval_seconds: u64,
    #[arg(long, default_value_t = 300)]
    timeout_seconds: u64,
    #[arg(long)]
    output_snapshot: Option<PathBuf>,
    #[arg(long)]
    output_usage: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let requested_analyzers = parse_analyzers(&args.analyzers)?;
    let client = CloudClient::new(CloudClientConfig {
        base_url: args.cloud_url,
    });
    let result = client
        .analyze_project(AnalyzeProjectRequest {
            project_root: args
                .project
                .canonicalize()
                .with_context(|| format!("failed to canonicalize {}", args.project.display()))?,
            workspace_id: args.workspace_id,
            display_name: args.display_name,
            base_revision: args.base_revision,
            requested_analyzers,
            wait: WaitForAnalysisOptions {
                poll_interval: Duration::from_secs(args.poll_interval_seconds),
                timeout: Duration::from_secs(args.timeout_seconds),
            },
        })
        .await?;

    if let Some(path) = args.output_snapshot {
        write_pretty_json(path, &result.snapshot)?;
    }
    if let Some(path) = args.output_usage {
        write_pretty_json(path, &result.usage)?;
    }

    println!("Cloud analysis completed");
    println!();
    println!("Workspace: {}", result.sync.workspace.id);
    println!("Revision: {}", result.sync.revision.id);
    println!("Job: {}", result.job.id);
    println!();
    println!("Files: {}", result.sync.files_count);
    println!("Uploaded blobs: {}", result.sync.uploaded_blobs);
    println!("Skipped blobs: {}", result.sync.skipped_blobs);
    println!();
    println!("Snapshot:");
    println!("  nodes: {}", result.snapshot.nodes.len());
    println!("  edges: {}", result.snapshot.edges.len());
    println!("  files: {}", result.snapshot.files.len());
    println!();
    println!("Usage:");
    println!("  input files: {}", result.usage.input_files);
    println!("  input bytes: {}", human_bytes(result.usage.input_bytes));
    println!("  total wall: {} ms", result.usage.total_wall_ms);
    println!("  credits estimated: {}", result.usage.credits_estimated);
    println!("  credits used: {}", result.usage.credits_used);
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

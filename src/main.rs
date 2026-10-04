mod cli;
mod model;
mod project;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let project = project::Project::discover(&cli.path)?;
    if cli.json {
        println!("{}", serde_json::to_string(&project)?);
    } else {
        println!(
            "rust_watcher\n\nWorkspace\n  root:   {}\n  crates: {}\n  files:  {}",
            project.workspace_root.display(),
            project.packages.len(),
            project.rust_files.len()
        );
    }
    Ok(())
}

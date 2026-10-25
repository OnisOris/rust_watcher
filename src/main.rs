mod architecture;
mod cli;
mod git_analysis;
mod lsp;
mod maintainability;
mod metrics;
mod model;
mod module_graph;
mod output;
mod project;
mod repository;
mod rust;
mod tui;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Command};
use lsp::AnalyzerNotFound;
use model::{ProjectSummary, Severity};
use project::Project;
use rust::RustAnalyzer;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(error) = execute(&cli).await {
        if json {
            output::json_error(&error);
        } else {
            eprintln!("error: {error:#}");
            if error.downcast_ref::<AnalyzerNotFound>().is_some() {
                eprintln!("\nInstall rust-analyzer with:\n    rustup component add rust-analyzer");
            }
        }
        std::process::exit(1);
    }
}

async fn execute(cli: &Cli) -> Result<()> {
    if matches!(cli.command, Some(Command::Doctor)) {
        return doctor(&cli.path, cli.json).await;
    }
    let project = Project::discover(&cli.path)?;
    if let Some(Command::Modules { crate_name, target }) = &cli.command {
        let selected = module_graph::select(
            &project.architecture,
            crate_name.as_deref(),
            target.as_deref(),
        )?;
        let tree = repository::scan(&project.workspace_root)?;
        let graph = module_graph::analyze(&project.workspace_root, &tree, &selected, || false);
        if cli.json {
            return output::json(&graph);
        }
        module_graph::print(&graph);
        return Ok(());
    }
    if matches!(cli.command, Some(Command::Architecture)) {
        if cli.json {
            return output::json(&project.architecture);
        }
        architecture::print(&project.architecture);
        return Ok(());
    }
    if let Some(Command::Hotspots { days }) = cli.command {
        let options = git_analysis::Options {
            days,
            ..Default::default()
        };
        let tree = repository::scan(&project.workspace_root)?;
        let metrics = metrics::analyze(&project.workspace_root, &tree, &project);
        let ast = maintainability::analyze(&project.workspace_root, &tree, &project);
        let history = git_analysis::analyze(
            &project.workspace_root,
            &options,
            &git_analysis::GitRunner::default(),
        );
        let report = git_analysis::correlate(history, &metrics, &ast, &tree);
        if cli.json {
            return output::json(&report);
        }
        git_analysis::print(&report);
        return Ok(());
    }
    if matches!(cli.command, Some(Command::Maintainability)) {
        let tree = repository::scan(&project.workspace_root)?;
        let report = maintainability::analyze(&project.workspace_root, &tree, &project);
        if cli.json {
            return output::json(&report);
        }
        output::maintainability(&report);
        return Ok(());
    }
    if matches!(cli.command, Some(Command::Stats)) {
        let tree = repository::scan(&project.workspace_root)?;
        let metrics = metrics::analyze(&project.workspace_root, &tree, &project);
        if cli.json {
            return output::json(&metrics);
        }
        output::metrics(&metrics);
        return Ok(());
    }
    if cli.command.is_none() && !cli.json {
        return tui::run(project).await;
    }
    let mut analyzer = RustAnalyzer::start(project, Path::new("rust-analyzer")).await?;
    let result = run(cli, &mut analyzer).await;
    analyzer.shutdown().await;
    result
}

async fn run(cli: &Cli, analyzer: &mut RustAnalyzer) -> Result<()> {
    match &cli.command {
        None | Some(Command::Summary) => summary(analyzer, cli.json).await,
        Some(Command::Symbol { name }) => {
            // Only interactive search may return partial results during indexing.
            analyzer.wait_ready().await?;
            let symbols = analyzer.symbols(name).await?;
            if cli.json {
                output::json(&symbols)
            } else {
                output::symbols(&symbols);
                Ok(())
            }
        }
        Some(Command::Refs { name }) => {
            let symbol = analyzer.find_symbol(name).await?;
            let locations = analyzer.references(&symbol).await?;
            if cli.json {
                output::json(&locations)
            } else {
                output::locations(&locations);
                Ok(())
            }
        }
        Some(Command::Definition { name }) => {
            let symbol = analyzer.find_symbol(name).await?;
            let location = analyzer.definition(&symbol).await?;
            if cli.json {
                output::json(&location)
            } else {
                output::locations(&location.into_iter().collect::<Vec<_>>());
                Ok(())
            }
        }
        Some(Command::Calls { name, depth }) => {
            call_command(analyzer, name, *depth, false, cli.json).await
        }
        Some(Command::Callers { name, depth }) => {
            call_command(analyzer, name, *depth, true, cli.json).await
        }
        Some(Command::Diagnostics { errors, warnings }) => {
            let mut diagnostics = analyzer.diagnostics().await?;
            if *errors {
                diagnostics.retain(|item| item.severity == Severity::Error);
            }
            if *warnings {
                diagnostics.retain(|item| item.severity == Severity::Warning);
            }
            if cli.json {
                output::json(&diagnostics)
            } else {
                output::diagnostics(&diagnostics);
                Ok(())
            }
        }
        Some(Command::Explain { name }) => {
            let symbol = analyzer.find_symbol(name).await?;
            let explanation = analyzer.explain(symbol).await?;
            if cli.json {
                output::json(&explanation)
            } else {
                output::explanation(&explanation);
                Ok(())
            }
        }
        Some(
            Command::Doctor
            | Command::Stats
            | Command::Maintainability
            | Command::Hotspots { .. }
            | Command::Architecture
            | Command::Modules { .. },
        ) => unreachable!(),
    }
}

async fn summary(analyzer: &mut RustAnalyzer, json: bool) -> Result<()> {
    analyzer.wait_ready().await?;
    let symbols = analyzer.all_document_symbols().await?;
    let diagnostics = analyzer.diagnostics().await?;
    let entrypoints: Vec<_> = analyzer
        .project
        .binary_entrypoints()
        .into_iter()
        .map(|file| model::Location {
            file,
            range: model::Range {
                start: model::Position {
                    line: 0,
                    character: 0,
                },
                end: model::Position {
                    line: 0,
                    character: 0,
                },
            },
        })
        .collect();
    let summary = ProjectSummary {
        workspace_root: analyzer.project.workspace_root.clone(),
        crates: analyzer.project.packages.len(),
        files: analyzer.project.rust_files.len(),
        symbols: symbols.len(),
        errors: diagnostics
            .iter()
            .filter(|item| item.severity == Severity::Error)
            .count(),
        warnings: diagnostics
            .iter()
            .filter(|item| item.severity == Severity::Warning)
            .count(),
        entrypoints,
    };
    if json {
        output::json(&summary)
    } else {
        output::summary(&summary);
        Ok(())
    }
}

async fn call_command(
    analyzer: &mut RustAnalyzer,
    name: &str,
    depth: u8,
    incoming: bool,
    json: bool,
) -> Result<()> {
    let symbol = analyzer.find_symbol(name).await?;
    let calls = analyzer.calls(&symbol, depth, incoming).await?;
    if json {
        output::json(&calls)
    } else {
        output::calls(name, &calls);
        Ok(())
    }
}

#[derive(Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: Option<String>,
}

async fn doctor(path: &Path, json: bool) -> Result<()> {
    let mut checks = vec![
        tool_check("cargo", &["--version"]),
        tool_check("rustc", &["--version"]),
    ];
    let analyzer_check = tool_check("rust-analyzer", &["--version"]);
    let analyzer_ok = analyzer_check.ok;
    checks.push(analyzer_check);
    let manifest = project::find_manifest(path);
    checks.push(DoctorCheck {
        name: "Cargo.toml",
        ok: manifest.is_ok(),
        detail: manifest.as_ref().err().map(ToString::to_string),
    });
    let project = Project::discover(path);
    checks.push(DoctorCheck {
        name: "cargo metadata",
        ok: project.is_ok(),
        detail: project.as_ref().err().map(ToString::to_string),
    });
    if analyzer_ok {
        let workspace = match project {
            Ok(project) => match RustAnalyzer::start(project, Path::new("rust-analyzer")).await {
                Ok(mut analyzer) => {
                    analyzer.shutdown().await;
                    DoctorCheck {
                        name: "rust-analyzer workspace",
                        ok: true,
                        detail: None,
                    }
                }
                Err(error) => DoctorCheck {
                    name: "rust-analyzer workspace",
                    ok: false,
                    detail: Some(error.to_string()),
                },
            },
            Err(error) => DoctorCheck {
                name: "rust-analyzer workspace",
                ok: false,
                detail: Some(error.to_string()),
            },
        };
        checks.push(workspace);
    }
    let failed = checks.iter().any(|check| !check.ok);
    if !json {
        for check in &checks {
            println!(
                "{} {}{}",
                if check.ok { "✓" } else { "✗" },
                check.name,
                check
                    .detail
                    .as_ref()
                    .map(|value| format!(": {value}"))
                    .unwrap_or_default()
            );
        }
    }
    if !analyzer_ok {
        return Err(AnalyzerNotFound(Path::new("rust-analyzer").to_path_buf()).into());
    }
    if failed {
        bail!("one or more doctor checks failed");
    }
    if json {
        output::json(&checks)?;
    }
    Ok(())
}

fn tool_check(name: &'static str, args: &[&str]) -> DoctorCheck {
    match std::process::Command::new(name)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(result) if result.status.success() => DoctorCheck {
            name,
            ok: true,
            detail: None,
        },
        Ok(result) => DoctorCheck {
            name,
            ok: false,
            detail: Some(String::from_utf8_lossy(&result.stderr).trim().to_owned()),
        },
        Err(error) => DoctorCheck {
            name,
            ok: false,
            detail: Some(error.to_string()),
        },
    }
}

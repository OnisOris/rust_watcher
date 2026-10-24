//! Passive Rust syntax measurements. Never resolves semantic identities.
mod syntax;
#[cfg(test)]
mod tests;

use crate::project::Project;
use crate::repository::{RepoEntryKind, RepositoryTree};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_AST_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_AST_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
pub const FUNCTION_LIMIT: usize = 100_000;
pub const RANKING_LIMIT: usize = 10;
pub const FAILURE_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FunctionKind {
    Function,
    Method,
    AssociatedFunction,
    TraitDefault,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionMetrics {
    pub path: PathBuf,
    pub name: String,
    pub qualified_name: String,
    pub kind: FunctionKind,
    pub start_line: usize,
    pub end_line: usize,
    pub identifier_line: usize,
    pub identifier_column: usize,
    pub physical_lines: usize,
    pub max_control_depth: usize,
    pub control_flow_nodes: usize,
    pub package: Option<String>,
    pub test_classified: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintainabilityReport {
    #[serde(skip)]
    pub(crate) files: Vec<FileMaintainability>,
    pub analyzed_files: usize,
    pub parsed_files: usize,
    pub parse_failures: usize,
    pub skipped_files: usize,
    pub read_bytes: u64,
    pub failures: Vec<FileFailure>,
    pub functions: usize,
    pub median_function_lines: usize,
    pub p95_function_lines: usize,
    pub max_control_depth: usize,
    pub longest_functions: Vec<FunctionMetrics>,
    pub deepest_functions: Vec<FunctionMetrics>,
    pub packages: Vec<PackageFunctionMetrics>,
    pub partial: bool,
}

#[derive(Debug, Clone)]
pub struct FileMaintainability {
    pub path: PathBuf,
    pub structure: Option<FileStructure>,
}

#[derive(Debug, Clone, Default)]
pub struct FileStructure {
    pub functions: usize,
    pub longest_function_lines: usize,
    pub max_control_depth: usize,
    pub control_flow_nodes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFailure {
    pub file: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFunctionMetrics {
    pub name: String,
    pub root: PathBuf,
    pub function_count: usize,
    pub median_function_lines: usize,
    pub p95_function_lines: usize,
    pub max_control_depth: usize,
}

pub fn analyze(root: &Path, tree: &RepositoryTree, project: &Project) -> MaintainabilityReport {
    analyze_until(root, tree, project, || false)
}

pub fn analyze_until(
    root: &Path,
    tree: &RepositoryTree,
    project: &Project,
    cancelled: impl Fn() -> bool,
) -> MaintainabilityReport {
    let mut report = MaintainabilityReport {
        partial: tree.truncated || tree.skipped_errors > 0,
        ..Default::default()
    };
    let mut lengths = BTreeMap::new();
    let mut package_lengths =
        BTreeMap::<PathBuf, (PackageFunctionMetrics, BTreeMap<usize, usize>)>::new();
    for p in &project.packages {
        if let Some(relative) = p
            .manifest_path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
        {
            package_lengths.insert(
                relative.into(),
                (
                    PackageFunctionMetrics {
                        name: p.name.clone(),
                        root: relative.into(),
                        ..Default::default()
                    },
                    BTreeMap::new(),
                ),
            );
        }
    }
    let mut budget = MAX_AST_TOTAL_BYTES;
    for entry in &tree.entries {
        if cancelled() {
            report.partial = true;
            break;
        }
        if entry.kind != RepoEntryKind::File || entry.path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        report.analyzed_files += 1;
        let file_index = report.files.len();
        report.files.push(FileMaintainability {
            path: entry.path.clone(),
            structure: None,
        });
        let source = match read_source(&root.join(&entry.path), &mut budget, &cancelled) {
            Ok(source) => source,
            Err(reason) => {
                report.skipped_files += 1;
                failure(&mut report, &entry.path, reason);
                continue;
            }
        };
        if cancelled() {
            report.partial = true;
            break;
        }
        let owner = package_lengths
            .keys()
            .filter(|p| entry.path.starts_with(p))
            .max_by_key(|p| p.components().count())
            .cloned();
        let package = owner.as_ref().map(|p| package_lengths[p].0.name.clone());
        let functions = match syntax::functions(
            &source,
            &entry.path,
            package,
            FUNCTION_LIMIT - report.functions,
            &cancelled,
        ) {
            Ok(functions) => functions,
            Err(syntax::Failure::Parse(error)) => {
                report.parse_failures += 1;
                failure(&mut report, &entry.path, error);
                continue;
            }
            Err(syntax::Failure::Resource(reason)) => {
                report.skipped_files += 1;
                failure(&mut report, &entry.path, reason.into());
                continue;
            }
        };
        report.parsed_files += 1;
        if report.functions + functions.len() < FUNCTION_LIMIT {
            report.files[file_index].structure = Some(FileStructure {
                functions: functions.len(),
                longest_function_lines: functions
                    .iter()
                    .map(|f| f.physical_lines)
                    .max()
                    .unwrap_or(0),
                max_control_depth: functions
                    .iter()
                    .map(|f| f.max_control_depth)
                    .max()
                    .unwrap_or(0),
                control_flow_nodes: functions.iter().map(|f| f.control_flow_nodes).sum(),
            });
        }
        for function in functions {
            *lengths.entry(function.physical_lines).or_default() += 1;
            if let Some(owner) = &owner {
                let (p, hist) = package_lengths.get_mut(owner).expect("known package root");
                p.function_count += 1;
                p.max_control_depth = p.max_control_depth.max(function.max_control_depth);
                *hist.entry(function.physical_lines).or_default() += 1;
            }
            report.functions += 1;
            report.max_control_depth = report.max_control_depth.max(function.max_control_depth);
            report.longest_functions.push(function.clone());
            report.longest_functions.sort_by(|a, b| {
                b.physical_lines
                    .cmp(&a.physical_lines)
                    .then_with(|| location_order(a, b))
            });
            report.longest_functions.truncate(RANKING_LIMIT);
            report.deepest_functions.push(function);
            report.deepest_functions.sort_by(|a, b| {
                b.max_control_depth
                    .cmp(&a.max_control_depth)
                    .then_with(|| location_order(a, b))
            });
            report.deepest_functions.truncate(RANKING_LIMIT);
        }
        if report.functions == FUNCTION_LIMIT {
            failure(&mut report, &entry.path, "function budget reached".into());
            break;
        }
    }
    report.read_bytes = MAX_AST_TOTAL_BYTES - budget;
    report.median_function_lines = percentile(&lengths, report.functions, 50);
    report.p95_function_lines = percentile(&lengths, report.functions, 95);
    report.packages = package_lengths
        .into_values()
        .map(|(mut p, h)| {
            p.median_function_lines = percentile(&h, p.function_count, 50);
            p.p95_function_lines = percentile(&h, p.function_count, 95);
            p
        })
        .collect();
    report
        .packages
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.root.cmp(&b.root)));
    report
}

fn location_order(a: &FunctionMetrics, b: &FunctionMetrics) -> std::cmp::Ordering {
    a.path
        .cmp(&b.path)
        .then(a.start_line.cmp(&b.start_line))
        .then(a.name.cmp(&b.name))
        .then(a.identifier_column.cmp(&b.identifier_column))
}

fn percentile(histogram: &BTreeMap<usize, usize>, count: usize, p: usize) -> usize {
    let rank = (count * p).div_ceil(100);
    let mut seen = 0;
    for (&lines, &n) in histogram {
        seen += n;
        if seen >= rank {
            return lines;
        }
    }
    0
}

fn failure(report: &mut MaintainabilityReport, file: &Path, reason: String) {
    report.partial = true;
    if report.failures.len() < FAILURE_LIMIT {
        report.failures.push(FileFailure {
            file: file.into(),
            reason: reason.chars().take(180).collect(),
        });
    }
}

fn read_source(
    path: &Path,
    budget: &mut u64,
    cancelled: &impl Fn() -> bool,
) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("read: {e}"))?;
    let size = file.metadata().map_err(|e| format!("metadata: {e}"))?.len();
    if size > MAX_AST_FILE_BYTES {
        return Err("AST per-file byte budget".into());
    }
    if size > *budget {
        return Err("AST total-read byte budget".into());
    }
    let mut bytes = Vec::with_capacity(size as usize);
    let mut chunk = [0; 64 * 1024];
    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        let allowed = (*budget)
            .min(MAX_AST_FILE_BYTES - bytes.len() as u64)
            .min(chunk.len() as u64) as usize;
        if allowed == 0 {
            if bytes.len() as u64 == size {
                break;
            }
            return Err("AST byte budget exhausted during read".into());
        }
        let n = file
            .read(&mut chunk[..allowed])
            .map_err(|e| format!("read: {e}"))?;
        *budget -= n as u64;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(bytes).map_err(|_| "invalid UTF-8".into())
}

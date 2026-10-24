use super::{time, History, Unavailable, RANKING_LIMIT};
use crate::{
    maintainability::MaintainabilityReport, metrics::RepositoryMetrics, repository::RepositoryTree,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub const FREQUENT_CHANGES: usize = 3;
pub const LARGE_SOURCE_LINES: u64 = 400;
pub const LONG_FUNCTION_LINES: usize = 80;
pub const DEEP_CONTROL: usize = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HotspotSignal {
    FrequentlyChanged,
    LargeSource,
    LongFunction,
    DeepNesting,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoChanged {
    pub path: PathBuf,
    pub commits: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHotspot {
    pub path: PathBuf,
    pub change_commits: usize,
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
    pub churn_lines: Option<u64>,
    pub last_changed_at: String,
    pub present_in_tree: bool,
    pub source_nonblank_lines: Option<u64>,
    pub functions: Option<usize>,
    pub longest_function_lines: Option<usize>,
    pub max_control_depth: Option<usize>,
    pub control_flow_nodes: Option<usize>,
    pub signals: Vec<HotspotSignal>,
    pub review_candidate: bool,
    pub co_changed: Vec<CoChanged>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotspotReport {
    pub available: bool,
    pub reason: Option<Unavailable>,
    pub since_days: u16,
    pub as_of_utc: String,
    pub head_sha: Option<String>,
    pub commits_analyzed: Option<usize>,
    pub changed_files: Option<usize>,
    pub candidate_files: Option<usize>,
    pub history_shallow: Option<bool>,
    pub working_tree_dirty: Option<bool>,
    pub history_partial: bool,
    pub partial: bool,
    pub partial_reasons: Vec<String>,
    pub hotspots: Vec<FileHotspot>,
    pub most_changed_files: Vec<FileHotspot>,
}

pub fn correlate(
    history: History,
    metrics: &RepositoryMetrics,
    ast: &MaintainabilityReport,
    tree: &RepositoryTree,
) -> HotspotReport {
    let source: BTreeMap<_, _> = metrics
        .source_files
        .iter()
        .map(|f| (&f.path, f.nonblank_lines))
        .collect();
    let structures: BTreeMap<_, _> = ast
        .files
        .iter()
        .map(|f| (&f.path, f.structure.as_ref()))
        .collect();
    let paths: BTreeSet<_> = tree.entries.iter().map(|f| &f.path).collect();
    let mut files = Vec::new();
    for (path, change) in &history.files {
        let structure = structures.get(path).copied().flatten();
        let loc = source.get(path).copied();
        let mut signals = Vec::new();
        if change.commits >= FREQUENT_CHANGES {
            signals.push(HotspotSignal::FrequentlyChanged);
        }
        if loc.is_some_and(|n| n >= LARGE_SOURCE_LINES) {
            signals.push(HotspotSignal::LargeSource);
        }
        if structure.is_some_and(|s| s.longest_function_lines >= LONG_FUNCTION_LINES) {
            signals.push(HotspotSignal::LongFunction);
        }
        if structure.is_some_and(|s| s.max_control_depth >= DEEP_CONTROL) {
            signals.push(HotspotSignal::DeepNesting);
        }
        let candidate = change.commits >= FREQUENT_CHANGES && signals.len() >= 2;
        files.push(FileHotspot {
            path: path.clone(),
            change_commits: change.commits,
            added_lines: change.added,
            deleted_lines: change.deleted,
            churn_lines: change
                .added
                .zip(change.deleted)
                .and_then(|(a, b)| a.checked_add(b)),
            last_changed_at: time::utc(change.last),
            present_in_tree: paths.contains(path),
            source_nonblank_lines: loc,
            functions: structure.map(|s| s.functions),
            longest_function_lines: structure.map(|s| s.longest_function_lines),
            max_control_depth: structure.map(|s| s.max_control_depth),
            control_flow_nodes: structure.map(|s| s.control_flow_nodes),
            signals,
            review_candidate: candidate,
            co_changed: Vec::new(),
        });
    }
    files.sort_by(|a, b| {
        b.change_commits
            .cmp(&a.change_commits)
            .then(b.churn_lines.cmp(&a.churn_lines))
            .then(a.path.cmp(&b.path))
    });
    let candidate_files = files.iter().filter(|f| f.review_candidate).count();
    let mut hotspots: Vec<_> = files
        .iter()
        .filter(|f| f.review_candidate)
        .take(RANKING_LIMIT)
        .cloned()
        .collect();
    files.truncate(RANKING_LIMIT);
    for file in hotspots.iter_mut().chain(files.iter_mut()) {
        let mut neighbors = BTreeMap::new();
        for commit in &history.cochanges {
            if commit.binary_search(&file.path).is_ok() {
                for path in commit {
                    if path != &file.path {
                        *neighbors.entry(path.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut neighbors: Vec<_> = neighbors
            .into_iter()
            .map(|(path, commits)| CoChanged { path, commits })
            .collect();
        neighbors.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.path.cmp(&b.path)));
        neighbors.truncate(RANKING_LIMIT);
        file.co_changed = neighbors;
    }
    let available = history.reason.is_none();
    let history_partial = !history.partial_reasons.is_empty();
    let mut partial_reasons = history.partial_reasons;
    if metrics.partial {
        partial_reasons.push("currentFileMetricsPartial".into());
    }
    if ast.partial {
        partial_reasons.push("currentRustStructurePartial".into());
    }
    HotspotReport {
        available,
        reason: history.reason,
        since_days: history.since_days,
        as_of_utc: history.as_of_utc,
        head_sha: history.head_sha,
        commits_analyzed: available.then_some(history.commits_analyzed),
        changed_files: available.then_some(history.files.len()),
        candidate_files: available.then_some(candidate_files),
        history_shallow: history.history_shallow,
        working_tree_dirty: history.working_tree_dirty,
        history_partial,
        partial: !partial_reasons.is_empty(),
        partial_reasons,
        hotspots: if available { hotspots } else { Vec::new() },
        most_changed_files: if available { files } else { Vec::new() },
    }
}

impl HotspotReport {
    pub fn rows(&self) -> &[FileHotspot] {
        if self.hotspots.is_empty() {
            &self.most_changed_files
        } else {
            &self.hotspots
        }
    }
    pub fn status(&self) -> String {
        if !self.available {
            return format!(
                "Git history unavailable: {}",
                self.reason
                    .as_ref()
                    .map_or("unknown", |r| r.message.as_str())
            );
        }
        if self.commits_analyzed == Some(0) {
            return if self.history_partial {
                "Git history: partial · no complete commits measured"
            } else {
                "No commits in selected window"
            }
            .into();
        }
        let label = if self.hotspots.is_empty() {
            if self.commits_analyzed.unwrap_or(0) < FREQUENT_CHANGES && !self.history_partial {
                "Insufficient change history for hotspot candidates"
            } else {
                "No hotspots found · most changed files"
            }
        } else {
            "Git Hotspots ready"
        };
        format!("{label}{}", if self.partial { " · partial" } else { "" })
    }
}

pub fn print(report: &HotspotReport) {
    println!(
        "Git Hotspots\n\nWindow             {} days\nAs of UTC          {}\n{}",
        report.since_days,
        report.as_of_utc,
        report.status()
    );
    if !report.available {
        return;
    }
    println!(
        "Commits analyzed   {}\nChanged files      {}\nHistory            {}",
        report.commits_analyzed.unwrap(),
        report.changed_files.unwrap(),
        if report.history_partial {
            "partial"
        } else {
            "complete"
        }
    );
    if report.working_tree_dirty == Some(true) {
        println!("Working tree has uncommitted changes");
    }
    for reason in &report.partial_reasons {
        println!("Limitation: {reason}");
    }
    for (title, files) in [
        ("Review Candidates", &report.hotspots),
        ("Most Changed Files", &report.most_changed_files),
    ] {
        println!("\n{title}");
        for (i, f) in files.iter().enumerate() {
            println!("{}. {:?}\n   changes: {} · churn: {} · source LOC: {} · max depth: {}\n   signals: {}", i+1, f.path, f.change_commits,
                display(f.churn_lines), display(f.source_nonblank_lines), display(f.max_control_depth),
                f.signals.iter().map(|s| serde_json::to_value(s).unwrap().as_str().unwrap().to_owned()).collect::<Vec<_>>().join(", "));
        }
    }
}
fn display(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "unknown".into(), |v| v.to_string())
}

//! Deterministic evidence and exact navigation values; no I/O or terminal types.
use super::{explanations::Metric, journey::Action};
use crate::{
    git_analysis::{FileHotspot, HotspotReport},
    maintainability::MaintainabilityReport,
};
use std::path::PathBuf;

pub(super) struct Evidence {
    pub metric: Metric,
    pub value: Option<u64>,
    pub unit: String,
}
pub(super) struct ReviewFinding {
    pub path: PathBuf,
    pub evidence: Vec<Evidence>,
    pub partial: bool,
    pub candidate: bool,
    pub next_actions: Vec<Action>,
}
pub(super) fn hotspot(
    report: &HotspotReport,
    file: &FileHotspot,
    ast: Option<&MaintainabilityReport>,
) -> ReviewFinding {
    let evidence = vec![
        Evidence {
            metric: Metric::Changes,
            value: Some(file.change_commits as u64),
            unit: format!("commits / {} days", report.since_days),
        },
        Evidence {
            metric: Metric::SourceLoc,
            value: file.source_nonblank_lines,
            unit: "nonblank lines (comments included)".into(),
        },
        Evidence {
            metric: Metric::PhysicalLoc,
            value: file.longest_function_lines.map(|n| n as u64),
            unit: "lines in longest measured function".into(),
        },
        Evidence {
            metric: Metric::Depth,
            value: file.max_control_depth.map(|n| n as u64),
            unit: "nested control structures".into(),
        },
    ];
    let mut next_actions = vec![];
    if file.present_in_tree {
        if let Some(ast) = ast {
            for f in ast
                .longest_functions
                .iter()
                .chain(&ast.deepest_functions)
                .filter(|f| f.path == file.path)
            {
                if !next_actions.iter().any(|a| matches!(a, Action::Function(old) if old.path == f.path && old.identifier_line == f.identifier_line && old.identifier_column == f.identifier_column)) {
                    next_actions.push(Action::Function(f.clone()));
                }
            }
        }
        next_actions.push(Action::File(
            file.path.clone(),
            "Inspect exact current file".into(),
        ));
    }
    ReviewFinding {
        path: file.path.clone(),
        evidence,
        partial: report.history_partial || report.current_metrics_partial,
        candidate: file.review_candidate,
        next_actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_structure_stays_unknown_and_historical_files_have_no_source_action() {
        let file = FileHotspot {
            path: "src/a.rs".into(),
            change_commits: 17,
            added_lines: None,
            deleted_lines: None,
            churn_lines: None,
            last_changed_at: String::new(),
            present_in_tree: false,
            source_nonblank_lines: None,
            functions: None,
            longest_function_lines: None,
            max_control_depth: None,
            control_flow_nodes: None,
            signals: vec![],
            review_candidate: false,
            co_changed: vec![],
        };
        let report = crate::git_analysis::correlate(
            Default::default(),
            &Default::default(),
            &Default::default(),
            &crate::repository::RepositoryTree {
                entries: vec![],
                truncated: false,
                skipped_errors: 0,
            },
        );
        let finding = hotspot(&report, &file, None);
        assert_eq!(finding.path, file.path);
        assert_eq!(finding.evidence[0].metric, Metric::Changes);
        assert_eq!(finding.evidence[0].value, Some(17));
        assert!(finding.evidence[1..].iter().all(|e| e.value.is_none()));
        assert!(!finding.candidate);
        assert!(finding.next_actions.is_empty());
        assert_eq!(
            finding
                .evidence
                .iter()
                .map(|e| e.metric)
                .collect::<Vec<_>>(),
            hotspot(&report, &file, None)
                .evidence
                .iter()
                .map(|e| e.metric)
                .collect::<Vec<_>>()
        );
    }
}

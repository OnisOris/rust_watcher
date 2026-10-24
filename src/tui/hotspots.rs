//! Presentation only; Git I/O lives in git_analysis.
use super::{app::App, overview::Target, ui::heading};
use crate::git_analysis::FileHotspot;
use ratatui::text::Line;
use std::path::Path;

pub(super) fn selected(app: &App) -> Option<&FileHotspot> {
    let targets = super::overview::targets(app);
    let Target::Hotspot(path) = targets.get(app.project_selection)? else {
        return None;
    };
    app.hotspots
        .as_ref()?
        .rows()
        .iter()
        .find(|f| &f.path == path)
}
pub(super) fn label(app: &App, path: &Path) -> String {
    let report = app.hotspots.as_ref().unwrap();
    let f = report.rows().iter().find(|f| f.path == path).unwrap();
    format!(
        "{} changes / {}d{} · {}\n  {:?}",
        f.change_commits,
        report.since_days,
        if f.review_candidate {
            ""
        } else {
            " [most changed]"
        },
        path.file_name().unwrap_or_default().to_string_lossy(),
        path
    )
}
pub(super) fn status(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![heading("Git history")];
    if let Some(r) = &app.hotspots {
        lines.push(Line::raw(r.status()));
        if r.history_shallow == Some(true) {
            lines.push(Line::raw("Shallow history · incomplete clone"));
        }
        if r.working_tree_dirty == Some(true) {
            lines.push(Line::raw("Working tree has uncommitted changes"));
        }
        lines.extend(
            r.partial_reasons
                .iter()
                .map(|s| Line::raw(format!("Limitation: {s}"))),
        );
    } else {
        lines.push(Line::raw(app.hotspots_error.as_ref().map_or_else(
            || "Git history: analyzing...".into(),
            |e| format!("Git history unavailable: {e}"),
        )));
    }
    lines
}
pub(super) fn details(app: &App, f: &FileHotspot) -> Vec<Line<'static>> {
    let r = app.hotspots.as_ref().unwrap();
    let mut lines = vec![
        heading("Hotspot details"),
        Line::raw(format!("{:?}", f.path)),
        Line::raw(format!(
            "Changes / {}d    {}",
            r.since_days, f.change_commits
        )),
        Line::raw(format!("Added lines       {}", value(f.added_lines))),
        Line::raw(format!("Deleted lines     {}", value(f.deleted_lines))),
        Line::raw(format!("Churn             {}", value(f.churn_lines))),
        Line::raw(format!("Last changed      {}", f.last_changed_at)),
        heading("Current structure · working tree"),
        Line::raw(format!(
            "Source nonblank LOC  {}",
            value(f.source_nonblank_lines)
        )),
        Line::raw(format!("Functions            {}", value(f.functions))),
        Line::raw(format!(
            "Longest function     {}",
            value(f.longest_function_lines)
        )),
        Line::raw(format!(
            "Max control depth    {}",
            value(f.max_control_depth)
        )),
        heading("Reasons · signals, not defects"),
    ];
    if let Some(notice) = &app.notice {
        lines.insert(1, Line::raw(notice.clone()));
    }
    for s in &f.signals {
        lines.push(Line::raw(
            serde_json::to_value(s)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        ));
    }
    if !f.review_candidate {
        lines.push(Line::raw("Most changed file · not a review candidate"));
    }
    if !f.present_in_tree {
        lines.push(Line::raw("File absent from current repository tree"));
    }
    lines.push(heading("Co-changed with · not call dependencies"));
    for other in &f.co_changed {
        lines.push(Line::raw(format!(
            "{:?}  {} commits",
            other.path, other.commits
        )));
    }
    lines.extend(status(app));
    lines.push(Line::raw(
        "Enter → Explorer → Rust symbols → Inspector → c Calls",
    ));
    lines
}
fn value(n: Option<impl std::fmt::Display>) -> String {
    n.map_or_else(|| "unknown".into(), |n| n.to_string())
}

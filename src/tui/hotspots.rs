//! Presentation only; Git I/O lives in git_analysis.
use super::{app::App, components, overview::Target, theme, ui::heading};
use crate::git_analysis::FileHotspot;
use ratatui::text::Line;

pub(super) fn selected(app: &App) -> Option<&FileHotspot> {
    let (Target::Hotspot(path) | Target::Historical(path)) = super::overview::selected(app)? else {
        return None;
    };
    app.hotspots
        .as_ref()?
        .rows()
        .iter()
        .chain(app.hotspots.as_ref()?.historical_files.iter())
        .find(|f| f.path == path)
}
pub(super) fn status(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![heading("Git history")];
    if let Some(r) = &app.hotspots {
        lines.push(Line::raw(r.status()));
        if r.available {
            for (label, partial) in [
                ("History", r.history_partial),
                ("Co-change", r.co_change_partial),
                ("Current metrics", r.current_metrics_partial),
            ] {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        format!("{label:<21}"),
                        ratatui::style::Style::default().fg(theme::MUTED),
                    ),
                    components::status_label(
                        if partial { "partial" } else { "complete" },
                        if partial {
                            theme::WARNING
                        } else {
                            theme::SUCCESS
                        },
                    ),
                ]));
            }
        }
        if r.history_shallow == Some(true) {
            lines.push(Line::raw("Shallow history · incomplete clone"));
        }
        if r.working_tree_dirty == Some(true) {
            lines.push(Line::raw("Working tree has uncommitted changes"));
        }
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
    let finding = super::findings::hotspot(r, f, app.maintainability.as_ref());
    let mut lines = vec![
        heading("Why review this file?"),
        components::path_label(&finding.path, usize::MAX),
        Line::raw(if finding.candidate {
            "Included by Git + structural review heuristic; not a defect."
        } else {
            "Most changed / historical file; not a review candidate."
        }),
        heading("Evidence · measured, not a verdict"),
    ];
    for e in &finding.evidence {
        lines.push(Line::raw(format!(
            "{}: {} {}",
            e.metric.label(),
            value(e.value),
            e.unit
        )));
    }
    lines.extend([
        heading("Suggested investigation"),
        Line::raw(if finding.next_actions.is_empty() {
            "No current source action · historical path is absent."
        } else {
            "Enter exact file · p choose measured function/source"
        }),
        Line::raw("Then Enter RA symbol → Inspector → c Calls. Esc returns."),
        Line::raw(if finding.partial {
            "PARTIAL evidence · unknown values are not zero."
        } else {
            "Local committed history + current working-tree structure."
        }),
        Line::raw("Size, frequency and nesting do not establish importance or bugs."),
        Line::raw("? explains changes, churn, co-change and candidate thresholds"),
        heading("Git details"),
        Line::raw(format!(
            "Changes / {}d    {}",
            r.since_days, f.change_commits
        )),
        Line::raw(format!("Added lines       {}", value(f.added_lines))),
        Line::raw(format!("Deleted lines     {}", value(f.deleted_lines))),
        Line::raw(format!("Churn             {}", value(f.churn_lines))),
        Line::raw(format!("Last changed      {}", f.last_changed_at)),
        Line::raw(format!("Functions         {}", value(f.functions))),
        heading("Reasons · signals, not defects"),
    ]);
    if let Some(notice) = &app.notice {
        lines.insert(1, Line::raw(notice.clone()));
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

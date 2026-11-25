//! Compact measurements; the Explorer itself is the visualization.
use super::*;
fn measurements(s: &State) -> Vec<Line<'static>> {
    let snap = &s.snapshot;
    let mut lines = vec![
        heading(s.query.heat_mode.label()),
        Line::raw(format!(
            "{} files changed · {} observed",
            snap.changed_files, snap.filesystem
        )),
        Line::raw(format!("{} measured changed lines", measured(snap.totals))),
        Line::styled("Δ = added + deleted", Style::default().fg(theme::MUTED)),
        Line::raw(format!("+{} / -{}", snap.totals.added, snap.totals.deleted)),
    ];
    if snap.totals.unknown > 0 {
        lines.push(Line::styled(
            format!("{} unknown deltas", snap.totals.unknown),
            Style::default().fg(theme::WARNING),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(heading("SELECTED"));
    if let Some(row) = s.rows.get(s.selected) {
        lines.push(Line::raw(label(&row.path)));
        if let Some(f) = snap.files.get(&row.path) {
            lines.push(Line::raw(
                if f.totals.measured == 0 && f.totals.unknown > 0 {
                    "Added ?  Deleted ?".into()
                } else {
                    format!("Added +{}  Deleted -{}", f.totals.added, f.totals.deleted)
                },
            ));
            lines.push(Line::raw(format!(
                "Churn {} · {}",
                measured(f.totals),
                age(f.last)
            )));
            lines.push(Line::raw(
                f.net
                    .map(|d| format!("Net +{} / -{}", d.added, d.deleted))
                    .unwrap_or_else(|| "Net diff unavailable".into()),
            ));
            if let Some(reason) = f.event.measurement.and_then(|m| m.reason) {
                lines.push(Line::styled(reason, Style::default().fg(theme::WARNING)));
            }
            lines.push(Line::raw(if f.event.kind == activity::Kind::Deleted {
                "Deleted · source unavailable"
            } else if f.event.source == activity::Source::Agent {
                "Agent reported · no measured claim"
            } else {
                "Filesystem observed"
            }));
        } else if let Some(d) = snap.directories.get(&row.path) {
            lines.push(Line::raw(format!(
                "{} unique changed files",
                d.changed_files
            )));
            lines.push(Line::raw(format!(
                "+{} / -{} · {} Δ",
                d.totals.added,
                d.totals.deleted,
                d.totals.churn()
            )));
            lines.push(Line::raw(format!(
                "{} unknown delta files",
                d.unknown_files
            )));
        } else {
            lines.push(Line::raw("Baseline · no matching measurement"));
        }
    } else {
        lines.push(Line::raw("Waiting for file changes"));
    }
    lines.push(Line::raw(""));
    lines.push(heading("HOTTEST BRANCHES"));
    for path in snap.clusters.iter().take(3) {
        let (churn, volume) = snap
            .files
            .iter()
            .filter(|(p, _)| p.parent().unwrap_or(Path::new("")) == path)
            .fold((0u64, 0.), |(n, v), (_, f)| {
                (n.saturating_add(f.totals.churn()), v + f.volume)
            });
        lines.push(Line::styled(
            format!(
                "{} {} Δ",
                crate::activity::heat::bar(crate::activity::heat::level(volume)),
                churn
            ),
            heat_style(s, 0, crate::activity::heat::level(volume)),
        ));
        lines.push(Line::raw(label(path)));
    }
    lines
}
fn provenance(s: &State) -> Vec<Line<'static>> {
    vec![
        heading("MEASUREMENT / SOURCE"),
        Line::raw(s.data.measurement_status.clone()),
        Line::raw(if s.data.watching {
            "Filesystem: connected"
        } else {
            "Filesystem: unavailable"
        }),
        Line::raw(format!("Agent: {}", s.data.agent_status)),
        Line::styled(
            if s.data.incomplete {
                "Activity history incomplete"
            } else {
                "Reports do not prove authorship"
            },
            Style::default().fg(if s.data.incomplete {
                theme::WARNING
            } else {
                theme::MUTED
            }),
        ),
    ]
}
pub(super) fn inspector(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let block = panel(" CHANGE INSPECTOR ", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(6)]).split(inner);
    frame.render_widget(
        Paragraph::new(measurements(s)).wrap(Wrap { trim: false }),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(provenance(s)).wrap(Wrap { trim: false }),
        parts[1],
    );
}
pub(super) fn summary(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let mut lines = vec![
        heading("SESSION SUMMARY"),
        Line::raw(format!(
            "Duration {}s · current filters",
            s.data.started.elapsed().as_secs()
        )),
    ];
    lines.extend(measurements(s));
    lines.push(Line::raw(""));
    lines.push(heading("CHANGE MIX · EVENTS"));
    for (i, label) in [
        "M modified",
        "N created",
        "D deleted",
        "R renamed",
        "A agent reported",
    ]
    .iter()
    .enumerate()
    {
        lines.push(Line::raw(format!("{} {label}", s.snapshot.mix[i])));
    }
    lines.extend(provenance(s));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((s.summary_scroll as u16, 0))
            .block(panel(" SESSION SUMMARY ", true)),
        area,
    );
}

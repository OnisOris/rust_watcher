use super::*;
use crate::activity::{clock, Kind, Source};
use crate::tui::{components, theme};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
fn panel(title: impl Into<String>, active: bool) -> ratatui::widgets::Block<'static> {
    components::styled_panel("", active).title(title.into())
}
pub(in crate::tui) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let s = &app.activity;
    let d = &s.data;
    s.compact.set(area.width < 110);
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(if area.height < 24 { 5 } else { 8 }),
    ])
    .split(area);
    let secs = d.started.elapsed().as_secs();
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(format!(
                "AGENT FOCUS · LIVE · {} · Session {:02}:{:02}:{:02}{}",
                if d.watching {
                    "Watching ●"
                } else {
                    "Not watching ?"
                },
                secs / 3600,
                secs / 60 % 60,
                secs % 60,
                if d.incomplete {
                    " · Activity incomplete"
                } else {
                    ""
                }
            )),
            Line::raw(format!(
                "{} · {} · {} files · {} events",
                s.filter.label(),
                if s.directories_only {
                    "Directories"
                } else {
                    "Directories + files"
                },
                d.touched
                    .values()
                    .filter(|t| t.filesystem.is_some())
                    .count(),
                d.total_events
            )),
            Line::raw(format!("{} · Agent: {}", d.status, d.agent_status)),
        ])
        .style(Style::default().fg(if d.incomplete {
            theme::WARNING
        } else {
            theme::ACCENT
        })),
        rows[0],
    );
    if s.summary {
        summary(frame, rows[1], s);
    } else {
        let body = Layout::horizontal([
            Constraint::Percentage(if area.width < 110 { 57 } else { 73 }),
            Constraint::Min(18),
        ])
        .split(rows[1]);
        if s.children.is_empty() {
            frame.render_widget(Paragraph::new("No activity in this filter.\nFilesystem changes will appear here.\nf cycles filters; select All files to explore.\nInitial disk state is baseline, not a session change.").wrap(Wrap{trim:false}).block(panel(" PROJECT ",!s.timeline)),body[0]);
        } else if s.compact.get() {
            render_list(
                frame,
                body[0],
                s,
                &s.rows,
                s.compact_selection,
                " PROJECT · compact tree ",
                !s.timeline,
            );
        } else {
            let first = s.column.saturating_sub(2);
            let last = (first + 3).min(s.columns.len());
            let cols = Layout::horizontal(vec![
                Constraint::Ratio(1, (last - first) as u32);
                last - first
            ])
            .split(body[0]);
            for (i, area) in (first..last).zip(cols.iter()) {
                let col = &s.columns[i];
                let entries = s
                    .children
                    .get(&col.parent)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let title = if col.parent.as_os_str().is_empty() {
                    " PROJECT ".into()
                } else {
                    format!(" {} / ", col.parent.display())
                };
                render_list(
                    frame,
                    *area,
                    s,
                    entries,
                    col.selected,
                    &title,
                    i == s.column && !s.timeline,
                );
            }
        }
        details(frame, body[1], s);
    }
    timeline(frame, rows[2], s);
}
fn render_list(
    frame: &mut Frame<'_>,
    area: Rect,
    s: &State,
    rows: &[Row],
    selected: usize,
    title: &str,
    active: bool,
) {
    let start = selected.saturating_sub(area.height.saturating_sub(3) as usize);
    let end = (start + area.height as usize).min(rows.len());
    let items: Vec<_> = rows[start..end]
        .iter()
        .map(|r| {
            let name = r.path.file_name().unwrap_or_default().to_string_lossy();
            let t = s.data.touched.get(&r.path);
            let badge = if r.dir {
                format!(" [{}]", s.counts.get(&r.path).copied().unwrap_or(0))
            } else {
                t.map(|t| {
                    format!(
                        " [{}{}]",
                        t.filesystem.map_or("", Kind::marker),
                        t.agent.map_or("", Kind::marker)
                    )
                })
                .unwrap_or_default()
            };
            let fold = if r.dir {
                if s.compact.get() && s.expanded.contains(&r.path) {
                    "v "
                } else {
                    "> "
                }
            } else {
                "  "
            };
            let line = format!(
                "{}{}{}{}{}",
                "  ".repeat(r.depth.min(10)),
                fold,
                name,
                if r.dir { "/" } else { "" },
                badge
            );
            let recent = t.is_some_and(|t| t.last.elapsed() < activity::RECENT);
            ListItem::new(line).style(Style::default().fg(if recent {
                theme::WARNING
            } else if r.dir && s.counts.get(&r.path).is_some_and(|n| *n > 0) {
                theme::ACCENT
            } else {
                theme::TEXT
            }))
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!rows.is_empty()).then_some(selected - start));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(panel(title, active)),
        area,
        &mut state,
    );
}
fn details(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let path = if s.timeline {
        s.selected_path()
    } else {
        s.selected().map(|r| r.path.clone())
    };
    let mut lines = vec![];
    if s.timeline {
        if let Some(e) = s.data.events.iter().rev().nth(s.timeline_selection) {
            lines.push(Line::raw(format!(
                "{} · {} · {:?}",
                clock(e.at),
                e.kind.marker(),
                e.source
            )));
            if let Some(details) = &e.details {
                lines.push(Line::raw(details.clone()));
            }
        }
    }
    if let Some(path) = path {
        lines.push(Line::raw(path.display().to_string()));
        if let Some(t) = s.data.touched.get(&path) {
            lines.push(Line::raw(format!("{} observed events", t.events)));
            lines.push(Line::raw(format!("Last: {} UTC", clock(t.at))));
            if let Some(k) = t.filesystem {
                lines.push(Line::raw(format!(
                    "{} · Filesystem",
                    match k {
                        Kind::Deleted => "Deleted",
                        Kind::Created => "Created",
                        Kind::Renamed => "Renamed",
                        _ => "Recently modified",
                    }
                )));
            }
            if let Some(k) = t.agent {
                lines.push(Line::raw(format!("{} · Agent report", k.marker())));
            }
        } else {
            lines.push(Line::raw("Baseline / no session event"));
        }
        if let Some(n) = s.counts.get(&path) {
            lines.push(Line::raw(format!("{n} unique touched files")));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw("o / Enter · source"));
        lines.push(Line::raw("m · existing Map"));
    } else {
        lines.push(Line::raw("Select a file or event."));
    }
    lines.extend([
        Line::raw(""),
        Line::raw("M modified · N new · D deleted"),
        Line::raw("A agent · R reported read"),
        Line::raw("? incomplete / unknown"),
        Line::raw("Filesystem ≠ agent action"),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" DETAILS ", false)),
        area,
    );
}
fn timeline(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let offset = if s.timeline {
        s.timeline_selection.saturating_sub(24)
    } else {
        0
    };
    let events: Vec<_> = s
        .data
        .events
        .iter()
        .rev()
        .skip(offset)
        .take(50)
        .map(|e| {
            ListItem::new(format!(
                "{} {} {}{}{}",
                clock(e.at),
                e.kind.marker(),
                e.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(no file)".into()),
                if e.source == Source::Agent {
                    " · Agent event · "
                } else {
                    " · Filesystem · "
                },
                e.details.as_deref().unwrap_or("")
            ))
        })
        .collect();
    let title = format!(
        " RECENT EVENTS · UTC · {} retained · t select ",
        s.data.events.len()
    );
    let mut state = ListState::default()
        .with_selected((s.timeline && !events.is_empty()).then_some(s.timeline_selection - offset));
    frame.render_stateful_widget(
        List::new(events)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(panel(title, s.timeline)),
        area,
        &mut state,
    );
}
fn summary(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let d = &s.data;
    let secs = d.started.elapsed().as_secs();
    let mut lines = vec![
        Line::raw(format!("Duration: {}m {}s", secs / 60, secs % 60)),
        Line::raw(format!(
            "Observed modified files: {}",
            d.touched
                .values()
                .filter(|t| t.filesystem.is_some())
                .count()
        )),
        Line::raw(format!(
            "Created files: {}",
            d.touched.values().filter(|t| t.created).count()
        )),
        Line::raw(format!(
            "Deleted files: {}",
            d.touched.values().filter(|t| t.deleted).count()
        )),
        Line::raw(format!(
            "Observed events: {} · Agent reports: {}",
            d.total_events, d.agent_events
        )),
        Line::raw("Most active directories (overlapping subtrees):"),
    ];
    let mut dirs: Vec<_> = d
        .directories(Filter::Modified)
        .into_iter()
        .filter(|(p, _)| !p.as_os_str().is_empty())
        .collect();
    dirs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    lines.extend(
        dirs.into_iter()
            .take(8)
            .map(|(p, n)| Line::raw(format!("  {}/  {n} files", p.display()))),
    );
    lines.extend([
        Line::raw(format!("Agent event source: {}", d.agent_status)),
        Line::raw(if d.incomplete {
            "Activity incomplete · counts cover retained observations"
        } else {
            "Initial state excluded; no agent attribution for filesystem events"
        }),
        Line::raw("s / Esc return · j/k scroll · m Map · o selected source"),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((s.summary_scroll as u16, 0))
            .block(panel(" SESSION SUMMARY ", true)),
        area,
    );
}

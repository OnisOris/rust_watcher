mod focus;
mod timeline;
use super::*;
use crate::activity::focus::{action, marker};
use crate::tui::{components, theme};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
fn panel(title: impl Into<String>, active: bool) -> Block<'static> {
    components::styled_panel("", active).title(Line::styled(
        title.into(),
        Style::default()
            .fg(if active { theme::ACCENT } else { theme::MUTED })
            .add_modifier(Modifier::BOLD),
    ))
}
fn color(e: &activity::Event) -> Color {
    if e.source == activity::Source::Agent {
        theme::WARNING
    } else {
        match e.kind {
            activity::Kind::Created => theme::SUCCESS,
            activity::Kind::Deleted => theme::ERROR,
            activity::Kind::Renamed => theme::NUMBER,
            _ => theme::ACCENT,
        }
    }
}
fn heat_style(s: &State, index: usize, score: u32) -> Style {
    let base = Style::default().fg(if s.heat {
        color(&s.data.events[index])
    } else {
        theme::TEXT
    });
    if !s.heat {
        return base;
    }
    match score {
        0 => base.add_modifier(Modifier::DIM),
        1..=4 => base,
        5..=15 => base.add_modifier(Modifier::BOLD),
        _ => base.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    }
}
fn age(at: Instant) -> String {
    let secs = at.elapsed().as_secs();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}
fn name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn label(p: &Path) -> String {
    if p.as_os_str().is_empty() {
        "(root)".into()
    } else {
        p.display().to_string()
    }
}
fn heading(s: impl Into<String>) -> Line<'static> {
    Line::styled(s.into(), Style::default().fg(theme::ACCENT).bold())
}
pub(in crate::tui) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let s = &app.activity;
    let d = &s.data;
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    let secs = d.started.elapsed().as_secs();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" AGENT FOCUS  ", Style::default().fg(theme::ACCENT).bold()),
                Span::raw(format!(
                    "{}  ·  {}  ·  {:02}:{:02}  ",
                    s.mode.label(),
                    if d.watching {
                        "Watching ●"
                    } else {
                        "Watch unavailable ?"
                    },
                    secs / 60,
                    secs % 60
                )),
                Span::styled(
                    if d.incomplete {
                        "Activity incomplete"
                    } else {
                        ""
                    },
                    Style::default().fg(theme::WARNING),
                ),
            ]),
            Line::raw(format!(
                " {} · {} · {} · {}",
                s.query.sources.label(),
                s.query.types.label(),
                s.query.scope.label(),
                s.query.window.label()
            )),
            Line::styled(
                format!(
                    " {} · {} · {} · {}",
                    s.query.files.label(),
                    s.auto.label(),
                    if s.follow {
                        "Following hotspot"
                    } else {
                        "Focus pinned"
                    },
                    d.agent_status
                ),
                Style::default().fg(theme::MUTED),
            ),
        ]),
        rows[0],
    );
    if s.mode == Mode::Map && !s.summary {
        timeline::map(frame, rows[1], s);
        return;
    }
    if s.mode == Mode::Timeline && !s.summary {
        timeline::timeline(frame, rows[1], s);
        return;
    }
    if area.width >= 110 {
        let cols = Layout::horizontal([
            Constraint::Percentage(29),
            Constraint::Percentage(44),
            Constraint::Percentage(27),
        ])
        .split(rows[1]);
        tree(frame, cols[0], s);
        focus::focus(frame, cols[1], s);
        focus::stats(frame, cols[2], s);
    } else {
        let cols = Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)])
            .split(rows[1]);
        let right = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(if rows[1].height >= 22 { 9 } else { 5 }),
        ])
        .split(cols[1]);
        tree(frame, cols[0], s);
        focus::focus(frame, right[0], s);
        focus::compact_stats(frame, right[1], s);
    }
}
fn tree(frame: &mut Frame<'_>, area: Rect, s: &State) {
    if s.rows.is_empty() {
        frame.render_widget(Paragraph::new("No matching files.\nf types · a source\nb files · w time · p path\nFilters apply to all panels.").wrap(Wrap{trim:false}).block(panel(" PROJECT HEATMAP ",false)),area);
        return;
    }
    let start = s
        .selected
        .saturating_sub(area.height.saturating_sub(3) as usize);
    let end = (start + area.height.saturating_sub(2) as usize).min(s.rows.len());
    let items: Vec<_> = s.rows[start..end]
        .iter()
        .map(|r| {
            let (score, index, count) = if r.dir {
                s.snapshot
                    .directories
                    .get(&r.path)
                    .map(|d| (d.score, d.latest, d.files))
                    .unwrap_or((0, 0, 0))
            } else {
                s.snapshot
                    .files
                    .get(&r.path)
                    .map(|f| (f.score, f.latest, f.events))
                    .unwrap_or((0, 0, 0))
            };
            let active = s.focus.as_ref() == Some(&r.path);
            let recent = index < s.data.events.len()
                && s.data.events[index].observed.elapsed().as_secs() < 15
                && count > 0;
            let dot = if active && recent {
                "●"
            } else if score >= 24 {
                "◉"
            } else if count > 0 {
                "·"
            } else {
                " "
            };
            let branch = if r.dir {
                if s.expanded.contains(&r.path) {
                    "▾"
                } else {
                    "▸"
                }
            } else {
                " "
            };
            let badge = if r.dir {
                if count > 0 {
                    format!(" [{count}]")
                } else if s.prior_dirs.contains(&r.path) {
                    " *".into()
                } else {
                    String::new()
                }
            } else if let Some(f) = s.snapshot.files.get(&r.path) {
                format!(" {}", marker(&s.data.events[f.latest]))
            } else if let Some(touched) = s.data.touched.get(&r.path) {
                let mark = match touched.filesystem {
                    Some(activity::Kind::Created) => "N",
                    Some(activity::Kind::Deleted) => "D",
                    Some(activity::Kind::Renamed) => "R",
                    Some(_) => "M",
                    None => "A",
                };
                format!(" {mark}*")
            } else {
                String::new()
            };
            let indent = "  ".repeat(r.depth.min(8));
            ListItem::new(format!(
                "{indent}{branch} {dot} {}{}{badge}",
                name(&r.path),
                if r.dir { "/" } else { "" }
            ))
            .style(if count > 0 {
                heat_style(s, index, score)
            } else {
                Style::default().fg(theme::MUTED)
            })
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(s.selected - start));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(
                panel(format!(" PROJECT HEATMAP · {} ", s.sort.label()), false).title_bottom(
                    if !s.prior_dirs.is_empty() && s.query == Query::default() {
                        " * prior session event "
                    } else {
                        ""
                    },
                ),
            ),
        area,
        &mut state,
    );
}

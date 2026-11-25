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
fn heat_style(s: &State, _index: usize, score: u32) -> Style {
    let col = if !s.heat || score == 0 {
        theme::MUTED
    } else {
        match score {
            1 => theme::HEAT1,
            2 => theme::HEAT2,
            3 => theme::HEAT3,
            4 => theme::HEAT4,
            _ => theme::HEAT5,
        }
    };
    let style = Style::default().fg(col);
    if score >= 4 && s.heat {
        style.bold()
    } else {
        style
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
fn measured(t: crate::activity::heat::Totals) -> String {
    if t.measured == 0 {
        if t.unknown > 0 {
            "?".into()
        } else {
            "not measured".into()
        }
    } else {
        format!("{}{}", t.churn(), if t.unknown > 0 { "+?" } else { "" })
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
                Span::styled(
                    " AGENT FOCUS · HEAT  ",
                    Style::default().fg(theme::ACCENT).bold(),
                ),
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
                " {} · {} · {} · {} · {}",
                s.query.heat_mode.label(),
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
                        if s.manual_until.is_some_and(|until| Instant::now() < until) {
                            "Manual · 15s pause"
                        } else {
                            "Smart Follow"
                        }
                    } else {
                        "Focus pinned"
                    },
                    if d.total_events == 0 {
                        "Waiting for file changes"
                    } else {
                        &d.agent_status
                    }
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
    if s.summary {
        focus::summary(frame, rows[1], s);
        return;
    }
    if area.width >= 110 {
        let cols = Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(rows[1]);
        tree(frame, cols[0], s);
        focus::inspector(frame, cols[1], s);
    } else {
        let bands = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(rows[1]);
        tree(frame, bands[0], s);
        frame.render_widget(
            Paragraph::new(format!(
                " {} files · {} Δ{} · {}
 +added + deleted = Δ · x LIVE/SESSION · s Inspector",
                s.snapshot.changed_files,
                measured(s.snapshot.totals),
                "",
                s.query.heat_mode.label()
            )),
            bands[1],
        );
    }
}
fn tree(frame: &mut Frame<'_>, area: Rect, s: &State) {
    if s.rows.is_empty() {
        frame.render_widget(Paragraph::new("Waiting for file changes\n\nb all files · f types · a source\nFiles outside current filters stay hidden.")
            .block(panel(" PROJECT HEATMAP ",true)),area);
        return;
    }
    let height = area.height.saturating_sub(2) as usize;
    // Center the camera with neighboring hot siblings rather than bottom-pinning it.
    let start = s
        .selected
        .saturating_sub(height / 2)
        .min(s.rows.len().saturating_sub(height));
    let end = (start + height).min(s.rows.len());
    let width = area.width.saturating_sub(4) as usize;
    let items: Vec<_> = s.rows[start..end]
        .iter()
        .map(|r| {
            let (score, totals, count) = if r.dir {
                s.snapshot
                    .directories
                    .get(&r.path)
                    .map(|d| (d.score, d.totals, d.changed_files))
                    .unwrap_or_default()
            } else {
                s.snapshot
                    .files
                    .get(&r.path)
                    .map(|f| (f.score, f.totals, 0))
                    .unwrap_or_default()
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
            let mark = if r.dir {
                ""
            } else if let Some(f) = s.snapshot.files.get(&r.path) {
                if f.event.source == activity::Source::Agent {
                    "A "
                } else {
                    match f.event.kind {
                        activity::Kind::Created => "+ ",
                        activity::Kind::Deleted => "× ",
                        activity::Kind::Renamed => "→ ",
                        _ => "~ ",
                    }
                }
            } else {
                ""
            };
            let prior = if !r.dir && !s.snapshot.files.contains_key(&r.path) {
                s.data
                    .touched
                    .get(&r.path)
                    .map(|t| match t.filesystem {
                        Some(activity::Kind::Created) => " N*",
                        Some(activity::Kind::Deleted) => " D*",
                        Some(activity::Kind::Renamed) => " R*",
                        Some(_) => " M*",
                        None => " A*",
                    })
                    .unwrap_or("")
            } else {
                ""
            };
            let suffix = if totals.churn() > 0 || totals.unknown > 0 {
                format!(
                    " {} {:>5} Δ{}{}",
                    crate::activity::heat::bar(score),
                    measured(totals),
                    "",
                    if s.density == 2 && totals.measured > 0 {
                        format!(" +{}/-{}", totals.added, totals.deleted)
                    } else if r.dir && s.density != 0 {
                        format!(" · {count}f")
                    } else {
                        String::new()
                    }
                )
            } else {
                String::new()
            };
            let prefix = format!(
                "{}{} {}{}{}{}",
                "  ".repeat(r.depth.min(10)),
                branch,
                mark,
                name(&r.path),
                if r.dir { "/" } else { "" },
                prior
            );
            let room = width.saturating_sub(suffix.chars().count());
            let mut title: String = prefix.chars().take(room).collect();
            let clipped = Span::raw(&prefix).width() > room;
            while Span::raw(&title).width() > room {
                title.pop();
            }
            if clipped && room > 0 {
                while Span::raw(&title).width() >= room {
                    title.pop();
                }
                title.push('…');
            }
            let padding = room.saturating_sub(Span::raw(&title).width());
            ListItem::new(format!("{title}{}{suffix}", " ".repeat(padding)))
                .style(heat_style(s, 0, score))
        })
        .collect();
    let selected_path = s
        .rows
        .get(s.selected)
        .map(|r| label(&r.path))
        .unwrap_or_default();
    let mut state = ListState::default().with_selected(Some(s.selected.saturating_sub(start)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(
                Style::default()
                    .bg(theme::HEAT_SELECTION)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .block(
                panel(
                    format!(
                        " PROJECT HEATMAP · {} · {} ",
                        s.query.heat_mode.label(),
                        s.sort.label()
                    ),
                    true,
                )
                .title_bottom(format!(" {} ", selected_path)),
            ),
        area,
        &mut state,
    );
}

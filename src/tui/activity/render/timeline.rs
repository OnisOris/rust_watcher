use super::*;
pub(super) fn timeline(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    let max = s.snapshot.buckets.iter().copied().max().unwrap_or(0).max(1);
    let bars: String = s
        .snapshot
        .buckets
        .iter()
        .rev()
        .map(|n| ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][n * 7 / max])
        .collect();
    frame.render_widget(
        Paragraph::new(format!(
            "  -60s {bars} now · 5s buckets · {} matching events",
            s.snapshot.events.len()
        ))
        .style(Style::default().fg(theme::ACCENT))
        .block(panel(" OBSERVED RHYTHM ", false)),
        parts[0],
    );
    let cols = Layout::horizontal([Constraint::Percentage(57), Constraint::Percentage(43)])
        .split(parts[1]);
    let height = cols[0].height.saturating_sub(2) as usize;
    let start = s
        .timeline_selection
        .saturating_sub(height.saturating_sub(1));
    let items: Vec<_> = s
        .snapshot
        .events
        .iter()
        .rev()
        .skip(start)
        .take(height.min(50))
        .map(|i| {
            let e = &s.data.events[*i];
            ListItem::new(vec![
                Line::styled(
                    format!(
                        "{}  {}  {}",
                        activity::clock(e.at),
                        marker(e),
                        e.path
                            .as_ref()
                            .map(|p| label(p))
                            .unwrap_or_else(|| "(no file)".into())
                    ),
                    Style::default().fg(color(e)),
                ),
                Line::styled(
                    format!(
                        "          {} · {}",
                        action(e),
                        if e.source == activity::Source::Agent {
                            "Agent event"
                        } else {
                            "Filesystem"
                        }
                    ),
                    Style::default().fg(theme::MUTED),
                ),
            ])
        })
        .collect();
    let mut state = ListState::default()
        .with_selected((!items.is_empty()).then_some(s.timeline_selection - start));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(panel(" TIMELINE · j/k moment · o source ", true)),
        cols[0],
        &mut state,
    );
    let mut lines = vec![heading("SELECTED MOMENT")];
    if let Some(e) = s.selected_event() {
        lines.extend([
            Line::raw(format!("{} UTC · {}", activity::clock(e.at), action(e))),
            Line::raw(
                e.path
                    .as_ref()
                    .map(|p| label(p))
                    .unwrap_or_else(|| "No file attached".into()),
            ),
            Line::raw(if e.source == activity::Source::Agent {
                "Agent-reported claim"
            } else {
                "Filesystem observed · editor unknown"
            }),
        ]);
        if let Some(m) = e.measurement {
            lines.push(Line::raw(
                m.delta
                    .map(|d| format!("+{} / -{} · {} Δ", d.added, d.deleted, d.churn()))
                    .unwrap_or_else(|| "Content delta unavailable · file changed".into()),
            ));
        }
        if let Some(details) = &e.details {
            lines.push(Line::raw(details.clone()));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw("Enter / o exact source"));
    }
    lines.push(Line::raw(""));
    lines.push(heading("FILTERED WINDOW"));
    lines.push(Line::raw(format!(
        "{} files · {} events",
        s.snapshot.files.len(),
        s.snapshot.events.len()
    )));
    lines.push(Line::raw("w time · f type · a source"));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" MOMENT / PROVENANCE ", false)),
        cols[1],
    );
}
pub(super) fn map(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(area);
    frame.render_widget(Paragraph::new("Physical directory clusters · containment only · j/k select · o source · v Tree\nPreview: red = measured line churn; no semantic or agent-intent edges").style(Style::default().fg(theme::MUTED)),rows[0]);
    if s.snapshot.clusters.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching activity clusters yet.")
                .block(panel(" ACTIVITY MAP ", true)),
            rows[1],
        );
        return;
    }
    let per_page = if area.width >= 110 { 6 } else { 4 };
    let cols = if area.width >= 110 { 3 } else { 2 };
    let start = s.map_selection / per_page * per_page;
    let bands =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    for (slot, path) in s
        .snapshot
        .clusters
        .iter()
        .enumerate()
        .skip(start)
        .take(per_page)
    {
        let relative = slot - start;
        let cells = Layout::horizontal(vec![Constraint::Ratio(1, cols as u32); cols])
            .split(bands[relative / cols]);
        let selected = slot == s.map_selection;
        let mut lines = vec![heading(format!(
            "{} {}",
            if selected { "> ◉" } else { "  ·" },
            label(path)
        ))];
        for p in s
            .snapshot
            .ranked_files
            .iter()
            .filter(|p| p.parent().unwrap_or(Path::new("")) == path)
            .take(3)
        {
            let f = &s.snapshot.files[p];
            let e = &f.event;
            lines.push(Line::styled(
                format!("  └─ {} {} · {} Δ", marker(e), name(p), measured(f.totals)),
                heat_style(s, f.latest, f.score),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(panel(
                    if selected {
                        " SELECTED CLUSTER "
                    } else {
                        " DIRECTORY "
                    },
                    selected,
                )),
            cells[relative % cols],
        );
    }
}

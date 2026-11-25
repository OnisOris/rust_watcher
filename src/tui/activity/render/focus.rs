use super::*;
pub(super) fn focus(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let mut lines = Vec::new();
    if s.summary {
        lines.push(heading("SESSION SUMMARY"));
        lines.push(Line::raw("Current filters · retained observations"));
        lines.push(Line::raw(""));
        lines.extend(stats_lines(s));
    } else if let Some(path) = &s.focus {
        if let Some(file) = s.snapshot.files.get(path) {
            let event = &s.data.events[file.latest];
            let hot = file.last.elapsed().as_secs() < 15;
            lines.push(heading(if hot {
                "●  RECENT HOTSPOT"
            } else {
                "○  LAST OBSERVED FOCUS"
            }));
            lines.push(Line::styled(
                name(path),
                heat_style(s, file.latest, file.score).add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::raw(label(path)));
            lines.push(Line::styled(
                format!("{} · {}", action(event), age(file.last)),
                Style::default().fg(color(event)),
            ));
            lines.push(Line::raw(format!(
                "{} events · {} filesystem / {} agent",
                file.events, file.filesystem, file.agent
            )));
            if area.height >= 13 {
                lines.push(Line::raw(""));
                lines.push(heading("PATH SPOTLIGHT"));
                lines.push(Line::styled(
                    path.components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" → "),
                    Style::default().fg(theme::ACTIVE),
                ));
                lines.push(Line::raw(""));
                lines.push(heading(format!(
                    "ACTIVE CLUSTER · {}",
                    label(path.parent().unwrap_or(Path::new("")))
                )));
                for p in s
                    .snapshot
                    .ranked_files
                    .iter()
                    .filter(|p| p.parent() == path.parent())
                    .take(if s.density == 0 { 2 } else { 4 })
                {
                    let f = &s.snapshot.files[p];
                    let e = &s.data.events[f.latest];
                    lines.push(Line::styled(
                        format!(
                            "{} {}  {} · {}",
                            if p == path { "●" } else { "·" },
                            name(p),
                            action(e),
                            age(f.last)
                        ),
                        heat_style(s, f.latest, f.score),
                    ));
                }
                if area.height >= 23 && s.density > 0 {
                    lines.push(Line::raw(""));
                    lines.push(heading("RECENT FLOW · OBSERVED PATHS"));
                    let mut flow: Vec<_> = s.snapshot.flow.iter().rev().take(4).collect();
                    flow.reverse();
                    for (i, p) in flow.iter().enumerate() {
                        lines.push(Line::styled(
                            format!(
                                "{} {}",
                                if i + 1 == flow.len() { "→" } else { "·" },
                                label(p)
                            ),
                            Style::default().fg(if i + 1 == flow.len() {
                                theme::ACCENT
                            } else {
                                theme::MUTED
                            }),
                        ));
                    }
                }
            }
            if s.density == 2 && area.height >= 28 {
                if let Some(details) = &event.details {
                    lines.push(Line::raw(""));
                    lines.push(Line::raw(details.clone()));
                }
            }
            if area.height >= 18 {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    if file.filesystem > 0 && file.agent > 0 {
                        "Both sources · no causal attribution"
                    } else if file.agent > 0 {
                        "Agent-reported · unverified claim"
                    } else {
                        "Filesystem observed · editor unknown"
                    },
                    Style::default().fg(theme::MUTED),
                ));
            }
        }
    } else {
        lines.extend([
            heading("WAITING FOR A SIGNAL"),
            Line::raw(""),
            Line::raw("Change a file in another terminal."),
            Line::raw("Active paths light up here."),
            Line::raw(""),
            Line::raw("Filesystem works without an agent."),
            Line::raw("No claim of current editing."),
            Line::raw(""),
            Line::raw("f types · a source · w time · p path"),
        ]);
    }
    let pulse = s
        .focus
        .as_ref()
        .and_then(|p| s.snapshot.files.get(p))
        .is_some_and(|f| {
            f.last.elapsed().as_secs() < 15 && s.data.started.elapsed().as_secs().is_multiple_of(2)
        });
    let block = panel(
        if s.summary {
            " SESSION SUMMARY "
        } else {
            match s.density {
                0 => " FOCUS · COMPACT ",
                1 => " FOCUS · OBSERVED ATTENTION ",
                _ => " FOCUS · DETAILED ",
            }
        },
        true,
    )
    .border_style(
        Style::default()
            .fg(if pulse { theme::ACTIVE } else { theme::ACCENT })
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((
                if s.summary {
                    s.summary_scroll as u16
                } else {
                    0
                },
                0,
            ))
            .block(block),
        area,
    );
}
fn stats_lines(s: &State) -> Vec<Line<'static>> {
    let snap = &s.snapshot;
    let secs = s.data.started.elapsed().as_secs();
    let mut lines = vec![
        heading("SESSION / FILTERED WINDOW"),
        Line::raw(format!("{:02}:{:02} elapsed", secs / 60, secs % 60)),
        Line::raw(format!(
            "{} events · {} files",
            snap.events.len(),
            snap.files.len()
        )),
        Line::raw(format!("{} directories", snap.directories.len())),
        Line::styled(
            "Directory counts overlap",
            Style::default().fg(theme::MUTED),
        ),
        Line::raw(""),
        heading("CHANGE MIX · EVENTS"),
    ];
    for (index, (mark, label, col)) in [
        ("M", "modified", theme::ACCENT),
        ("N", "new", theme::SUCCESS),
        ("D", "deleted", theme::ERROR),
        ("R", "renamed", theme::NUMBER),
        ("A", "agent-reported", theme::WARNING),
    ]
    .into_iter()
    .enumerate()
    {
        lines.push(Line::styled(
            format!("{mark} {:>4}  {label}", snap.mix[index]),
            Style::default().fg(col),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(heading("HOTTEST BRANCHES"));
    for p in snap.clusters.iter().take(3) {
        let (events, files) = snap
            .files
            .iter()
            .filter(|(f, _)| f.parent().unwrap_or(Path::new("")) == p)
            .fold((0, 0), |(e, n), (_, f)| (e + f.events, n + 1));
        lines.push(Line::raw(format!("{} · {files}f / {events}e", label(p))));
    }
    lines.push(Line::raw(""));
    lines.push(heading("LEADING FILES · EVENT COUNT"));
    let mut top: Vec<_> = snap.files.iter().collect();
    top.sort_by(|a, b| b.1.events.cmp(&a.1.events).then(a.0.cmp(b.0)));
    for (p, f) in top.into_iter().take(3) {
        lines.push(Line::raw(format!("{}  {}", f.events, label(p))));
    }
    lines.push(Line::raw(""));
    lines.push(heading("FOCUS PATTERN · LAST 20 / 60s"));
    lines.push(Line::raw(snap.stability));
    lines.push(Line::raw(""));
    lines.push(heading("PROVENANCE"));
    lines.push(Line::raw(format!(
        "{} observed / {} reported",
        snap.filesystem, snap.agent
    )));
    lines.push(Line::raw(if snap.agent == 0 {
        "Filesystem evidence only"
    } else if snap.filesystem == 0 {
        "Agent reports only"
    } else {
        "Merged evidence · no causal link"
    }));
    if s.data.incomplete {
        lines.push(Line::styled(
            "Activity incomplete",
            Style::default().fg(theme::WARNING),
        ));
    }
    lines
}
pub(super) fn stats(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let mut lines = stats_lines(s);
    if area.height < 34 {
        // Keep trust visible; detailed rankings live in Summary.
        lines = vec![
            heading("SESSION / WINDOW"),
            Line::raw(format!(
                "{} events · {} files",
                s.snapshot.events.len(),
                s.snapshot.files.len()
            )),
            Line::raw(format!("{} directories", s.snapshot.directories.len())),
            Line::raw(""),
            heading("CHANGE MIX"),
        ];
        for (i, label) in [
            "M modified",
            "N new",
            "D deleted",
            "R renamed",
            "A agent-reported",
        ]
        .iter()
        .enumerate()
        {
            lines.push(Line::raw(format!("{}  {label}", s.snapshot.mix[i])));
        }
        lines.push(Line::raw(""));
        lines.push(heading("HOTTEST CLUSTER"));
        lines.push(Line::raw(
            s.snapshot
                .clusters
                .first()
                .map(|p| label(p))
                .unwrap_or_else(|| "No events".into()),
        ));
        if area.height >= 25 {
            lines.push(heading("LEADING FILES"));
            let mut top: Vec<_> = s.snapshot.files.iter().collect();
            top.sort_by(|a, b| b.1.events.cmp(&a.1.events).then(a.0.cmp(b.0)));
            for (p, f) in top.into_iter().take(3) {
                lines.push(Line::raw(format!("{} · {}", f.events, name(p))));
            }
        }
        lines.push(Line::raw(""));
        lines.push(heading("FOCUS PATTERN"));
        lines.push(Line::raw(s.snapshot.stability));
        lines.push(heading("TRUST / PROVENANCE"));
        lines.push(Line::raw(format!(
            "{} observed · {} reported",
            s.snapshot.filesystem, s.snapshot.agent
        )));
        lines.push(Line::raw("Evidence ≠ agent intent"));
    }
    if let Some(index) = lines
        .iter()
        .position(|l| matches!(l.to_string().as_str(), "TRUST / PROVENANCE" | "PROVENANCE"))
    {
        lines.truncate(index);
    }
    let block = panel(" SIGNAL SUMMARY ", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(inner);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[0]);
    frame.render_widget(
        Paragraph::new(vec![
            heading("PROVENANCE"),
            Line::raw(format!(
                "{} observed · {} reported",
                s.snapshot.filesystem, s.snapshot.agent
            )),
            Line::styled("Evidence ≠ agent intent", Style::default().fg(theme::MUTED)),
        ]),
        parts[1],
    );
}

pub(super) fn compact_stats(frame: &mut Frame<'_>, area: Rect, s: &State) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(format!(
                "{} events · {} files · {} dirs",
                s.snapshot.events.len(),
                s.snapshot.files.len(),
                s.snapshot.directories.len()
            )),
            Line::raw(format!(
                "M {}  N {}  D {}  R {}  A {}",
                s.snapshot.mix[0],
                s.snapshot.mix[1],
                s.snapshot.mix[2],
                s.snapshot.mix[3],
                s.snapshot.mix[4]
            )),
            Line::raw(format!(
                "{} observed / {} reported",
                s.snapshot.filesystem, s.snapshot.agent
            )),
            Line::raw(s.snapshot.stability),
        ])
        .block(panel(" SIGNAL SUMMARY · s full ", false)),
        area,
    );
}

use super::*;
pub(super) fn render(frame: &mut Frame<'_>, area: Rect, s: &State) {
    let choices = s.visible_choices();
    let Some(center) = choices.first() else {
        return;
    };
    let entry = &s.index.nodes[&s.focus];
    let self_link = (s.mode != Mode::Hierarchy)
        .then(|| {
            if s.mode == Mode::Imports {
                &entry.outgoing
            } else {
                &entry.incoming
            }
        })
        .and_then(|links| links.iter().find(|l| l.id == s.focus));
    let regions = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!(
                    "{}[0] {}",
                    if s.selected == s.focus { "> " } else { "  " },
                    components::compact(
                        &center.qualified_path,
                        area.width.saturating_sub(6) as usize
                    )
                ),
                if s.selected == s.focus {
                    components::selectable_row()
                } else {
                    Style::default().fg(theme::ACCENT)
                },
            ),
            Line::styled(
                if let Some(l) = self_link {
                    format!(
                        "↺ self import · {} records · {} conditional",
                        l.records, l.conditional
                    )
                } else {
                    match s.mode {
                        Mode::Hierarchy => "│ declares child modules",
                        Mode::Imports => "│ imports → neighbors (syntax)",
                        Mode::UsedBy => "▲ imported by neighbors (syntax)",
                    }
                    .into()
                },
                Style::default().fg(theme::MUTED),
            ),
        ]),
        regions[0],
    );
    if choices.len() == 1 {
        frame.render_widget(Paragraph::new(if self_link.is_some(){"Verified self-import; no other neighbors.\nv switches views · o source"}else{match s.mode{Mode::Hierarchy=>"No known child modules. o opens source.",Mode::Imports=>"No verified outgoing imports.\nSome imports may be unclassified.\nv switches views · o source",Mode::UsedBy=>"No verified incoming imports.\nNot semantic usage analysis.\nv switches views · o source"}}).wrap(ratatui::widgets::Wrap{trim:false}),regions[1]);
        return;
    }
    let e = &s.index.nodes[&s.focus];
    let links = if s.mode == Mode::Imports {
        &e.outgoing
    } else {
        &e.incoming
    };
    let items: Vec<_> = choices
        .iter()
        .skip(1)
        .map(|n| {
            let name =
                components::compact(&n.qualified_path, area.width.saturating_sub(16) as usize);
            if s.mode == Mode::Hierarchy {
                return ListItem::new(Line::styled(
                    format!(
                        "├─mod▶ {}{}",
                        n.name,
                        if n.conditions.is_empty() {
                            ""
                        } else {
                            " [cfg]"
                        }
                    ),
                    Style::default().fg(theme::ACCENT),
                ));
            }
            let l = links.iter().find(|l| l.id == n.id).unwrap();
            ListItem::new(vec![
                Line::styled(
                    if s.mode == Mode::Imports {
                        format!("├─use▶ {name}")
                    } else {
                        format!("{name} ─use▶ [0]")
                    },
                    Style::default().fg(theme::NUMBER),
                ),
                Line::styled(
                    format!(
                        "  {} record{}{}",
                        l.records,
                        if l.records == 1 { "" } else { "s" },
                        if l.conditional > 0 {
                            format!(" · [cfg] {}", l.conditional)
                        } else {
                            String::new()
                        }
                    ),
                    Style::default().fg(if l.conditional > 0 {
                        theme::WARNING
                    } else {
                        theme::MUTED
                    }),
                ),
            ])
        })
        .collect();
    let selection = choices
        .iter()
        .position(|n| n.id == s.selected)
        .and_then(|i| i.checked_sub(1));
    let mut state = ListState::default().with_selected(selection);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row()),
        regions[1],
        &mut state,
    );
}

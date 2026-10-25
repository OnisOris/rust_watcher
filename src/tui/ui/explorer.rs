use super::*;
pub(super) fn render_explorer_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    render_explorer_tree(frame, panes[0], app);
    if app.explorer.file.as_ref().is_some_and(|file| {
        app.explorer
            .selected_entry()
            .is_some_and(|e| e.path != file.path)
    }) {
        render_tree_selection(frame, panes[1], app);
        return;
    }
    if app.explorer.right_mode == ExplorerRightMode::Inspector {
        render_inspector_block(frame, panes[1], app, app.pane == Pane::ExplorerFile);
    } else {
        render_explorer_file(frame, panes[1], app);
    }
}

pub(super) fn render_explorer_tree(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let title = match &app.explorer.tree {
        Some(tree) if tree.truncated => " PROJECT TREE · truncated ".to_owned(),
        Some(tree) if tree.skipped_errors > 0 => {
            format!(" PROJECT TREE · {} skipped ", tree.skipped_errors)
        }
        _ if app.explorer.loading => " PROJECT TREE · scanning… ".to_owned(),
        _ => " PROJECT TREE ".to_owned(),
    };
    if let Some(error) = &app.explorer.error {
        frame.render_widget(
            Paragraph::new(format!("Repository scan failed\n\n{error}"))
                .wrap(Wrap { trim: false })
                .block(pane_block(&title, app.pane == Pane::ExplorerTree)),
            area,
        );
        return;
    }
    let workspace = project_name(&app.project);
    let visible = app.explorer.visible_entries();
    // Style only the viewport, not thousands of off-screen physical tree rows.
    // Keep absolute selection in ExplorerState; this window is presentation only.
    let count = area.height.saturating_sub(2) as usize;
    let start = app
        .explorer
        .selected
        .saturating_sub(count / 2)
        .min(visible.len().saturating_sub(count));
    let rows: Vec<_> = visible
        .iter()
        .skip(start)
        .take(count)
        .map(|entry| {
            let depth = entry.depth.min(area.width as usize / 6);
            let indent = format!(
                "{}{}",
                "  ".repeat(depth),
                if depth < entry.depth { "… " } else { "" }
            );
            let name = if entry.path.as_os_str().is_empty() {
                workspace.to_owned()
            } else {
                entry
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?")
                    .to_owned()
            };
            let marker = match entry.kind {
                RepoEntryKind::Directory if entry.path.as_os_str().is_empty() => "▼ ",
                RepoEntryKind::Directory if app.explorer.is_expanded(&entry.path) => "▼ ",
                RepoEntryKind::Directory => "▶ ",
                RepoEntryKind::Symlink => "@ ",
                RepoEntryKind::File
                    if crate::metrics::classify(&entry.path).1
                        == crate::metrics::Category::Tests =>
                {
                    "T "
                }
                RepoEntryKind::File if entry.path.extension().is_some_and(|e| e == "rs") => "R ",
                RepoEntryKind::File
                    if matches!(
                        crate::metrics::classify(&entry.path).1,
                        crate::metrics::Category::Configuration
                    ) =>
                {
                    "C "
                }
                RepoEntryKind::File => "· ",
            };
            let role = file_role(&app.project, &entry.path)
                .map(|role| format!(" [{role}]"))
                .unwrap_or_default();
            let name = components::compact(
                &name,
                (area.width as usize)
                    .saturating_sub(indent.chars().count() + role.chars().count() + 6),
            );
            let line = format!("{indent}{marker}{name}{role}");
            ListItem::new(if entry.kind == RepoEntryKind::Directory {
                Line::styled(
                    line,
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Line::raw(line)
            })
        })
        .collect();
    let mut state =
        ListState::default().with_selected(Some(app.explorer.selected.saturating_sub(start)));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(components::selectable_row())
            .block(pane_block(&title, app.pane == Pane::ExplorerTree)),
        area,
        &mut state,
    );
}

fn render_tree_selection(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let Some(entry) = app.explorer.selected_entry() else {
        return;
    };
    let mut text = vec![
        heading("TREE SELECTION"),
        components::path_label(&entry.path, usize::MAX),
        Line::raw(""),
    ];
    if entry.kind == RepoEntryKind::Directory {
        let (dirs, files) = app.explorer.direct_child_counts(&entry.path);
        text.extend([
            components::metric_value("Directories", dirs),
            components::metric_value("Files", files),
        ]);
    } else {
        text.extend([
            components::metric_value("Type", file_type(&entry.path)),
            components::metric_value("Size", format_size(entry.size)),
        ]);
    }
    text.extend([
        Line::raw(""),
        Line::raw("Press Enter to open this item."),
        Line::raw("Previous Inspector is retained, not shown as this file."),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(pane_block(" FILE CONTEXT ", app.pane == Pane::ExplorerFile)),
        area,
    );
}

pub(super) fn render_explorer_file(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let active = app.pane == Pane::ExplorerFile;
    let Some(file) = &app.explorer.file else {
        let selected = app.explorer.selected_entry();
        let text = selected.map_or_else(
            || {
                vec![
                    heading("Repository"),
                    Line::raw(""),
                    Line::raw("Scanning repository…"),
                ]
            },
            |entry| {
                if entry.kind == RepoEntryKind::Directory {
                    let (directories, files) = app.explorer.direct_child_counts(&entry.path);
                    vec![
                        heading("Directory"),
                        Line::raw(""),
                        Line::raw(if entry.path.as_os_str().is_empty() {
                            ".".into()
                        } else {
                            entry.path.display().to_string()
                        }),
                        Line::raw(""),
                        Line::raw(format!("directories   {directories}")),
                        Line::raw(format!("files         {files}")),
                    ]
                } else {
                    vec![heading("Select a file and press Enter")]
                }
            },
        );
        frame.render_widget(
            Paragraph::new(text).block(pane_block(" FILE ", active)),
            area,
        );
        return;
    };

    let details_height = if file.anchor.is_some() || file.module_anchor.is_some() {
        12
    } else {
        9
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(details_height), Constraint::Min(3)])
        .split(area);
    let package = package_for_file(&app.project, &file.path)
        .map(|package| package.name.as_str())
        .unwrap_or("—");
    let role = file_role(&app.project, &file.path).unwrap_or("file");
    let mut file_details = vec![
        heading(&file.path.display().to_string()),
        Line::raw(""),
        Line::raw(format!("type      {}", file_type(&file.path))),
        Line::raw(format!("size      {}", format_size(file.size))),
        Line::raw(format!("package   {package}")),
        Line::raw(format!("role      {role}")),
    ];
    if let Some((name, line, col)) = &file.module_anchor {
        file_details.push(Line::raw(format!("module anchor {name} · {line}:{col}")));
        if let Some(notice) = &file.anchor_notice {
            file_details.push(Line::raw(notice.clone()));
        }
    }
    if let Some(anchor) = &file.anchor {
        file_details.push(Line::raw(format!(
            "anchor    {}:{} · {}",
            anchor.identifier_line, anchor.identifier_column, anchor.qualified_name
        )));
        if let Some(notice) = &file.anchor_notice {
            file_details.push(Line::raw(notice.clone()));
        }
    }
    frame.render_widget(
        Paragraph::new(file_details)
            .wrap(Wrap { trim: false })
            .block(pane_block(" FILE ", active)),
        rows[0],
    );
    if file
        .path
        .extension()
        .is_none_or(|extension| extension != "rs")
    {
        frame.render_widget(
            Paragraph::new("Semantic symbols unavailable for this file type")
                .wrap(Wrap { trim: false })
                .block(pane_block(" SYMBOLS ", active)),
            rows[1],
        );
        return;
    }
    if let Some(error) = &file.error {
        frame.render_widget(
            Paragraph::new(format!("error: {error}"))
                .wrap(Wrap { trim: false })
                .block(pane_block(" SYMBOLS ", active)),
            rows[1],
        );
        return;
    }
    if file.loading {
        frame.render_widget(
            Paragraph::new("Loading file symbols…").block(pane_block(" SYMBOLS ", active)),
            rows[1],
        );
        return;
    }
    let symbols: Vec<_> = file
        .symbols
        .iter()
        .map(|symbol| {
            ListItem::new(format!(
                "{:<10} {}",
                truncate(&symbol.kind, 10),
                qualified_symbol(symbol)
            ))
        })
        .collect();
    let title = if file.truncated {
        " SYMBOLS · first 200 "
    } else if file.symbols.is_empty() {
        " NO SYMBOLS "
    } else {
        " SYMBOLS "
    };
    let mut state =
        ListState::default().with_selected((!file.symbols.is_empty()).then_some(file.selection));
    frame.render_stateful_widget(
        List::new(symbols)
            .highlight_symbol("> ")
            .highlight_style(Style::default().fg(theme::ACTIVE))
            .block(pane_block(title, active)),
        rows[1],
        &mut state,
    );
}

pub(super) fn file_type(path: &std::path::Path) -> String {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => "Rust source".into(),
        Some("toml") => "TOML".into(),
        Some("md") => "Markdown".into(),
        Some("yaml" | "yml") => "YAML".into(),
        Some("json") => "JSON".into(),
        Some(extension) => format!("{extension} file"),
        None => "file".into(),
    }
}

pub(super) fn format_size(size: Option<u64>) -> String {
    match size {
        Some(size) if size >= 1024 => format!("{:.1} KiB", size as f64 / 1024.0),
        Some(size) => format!("{size} B"),
        None => "—".into(),
    }
}

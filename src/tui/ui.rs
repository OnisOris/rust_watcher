use super::app::{
    project_name, qualified_symbol, AnalyzerState, App, Inspector, Overlay, Pane, View,
};
use super::explorer::{file_role, package_for_file, ExplorerRightMode};
use crate::model::CallTarget;
use crate::repository::RepoEntryKind;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 20;

pub(super) fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small\nMinimum: {MIN_WIDTH}x{MIN_HEIGHT}"
            ))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" wt ")),
            area,
        );
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);
    render_header(frame, rows[0], app);
    match app.view {
        View::Overview => render_overview(frame, rows[1], app),
        View::Symbols => render_symbols_view(frame, rows[1], app),
        View::Calls => render_calls_view(frame, rows[1], app),
        View::Explorer => render_explorer_view(frame, rows[1], app),
    }
    frame.render_widget(
        Paragraph::new(footer_text(app))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL)),
        rows[2],
    );
    match app.overlay {
        Overlay::Help => render_help(frame, area),
        Overlay::Search => render_search(frame, area, app),
        Overlay::None => {}
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let workspace = app
        .project
        .workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace");
    let (status, color) = match &app.analyzer_state {
        AnalyzerState::Starting => ("◌ starting".to_owned(), Color::Yellow),
        AnalyzerState::Indexing => ("◐ indexing".to_owned(), Color::Yellow),
        AnalyzerState::Ready => ("● ready".to_owned(), Color::Green),
        AnalyzerState::Error(message) => (format!("× {}", truncate(message, 18)), Color::Red),
    };
    let active = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let idle = Style::default().fg(Color::DarkGray);
    let title = Line::from(vec![
        Span::styled(" wt ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("{}  ", truncate(workspace, 20))),
        Span::raw(if app.view == View::Overview {
            "▶"
        } else {
            " "
        }),
        Span::styled(
            "[1 Overview]",
            if app.view == View::Overview {
                active
            } else {
                idle
            },
        ),
        Span::raw(" "),
        Span::raw(if app.view == View::Symbols {
            "▶"
        } else {
            " "
        }),
        Span::styled(
            "[2 Symbols]",
            if app.view == View::Symbols {
                active
            } else {
                idle
            },
        ),
        Span::raw(" "),
        Span::raw(if app.view == View::Calls { "▶" } else { " " }),
        Span::styled(
            "[3 Calls]",
            if app.view == View::Calls {
                active
            } else {
                idle
            },
        ),
        Span::raw(" "),
        Span::raw(if app.view == View::Explorer {
            "▶"
        } else {
            " "
        }),
        Span::styled(
            "[4 Explorer]",
            if app.view == View::Explorer {
                active
            } else {
                idle
            },
        ),
        Span::raw("  "),
        Span::styled(format!("RA {status}"), Style::default().fg(color)),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_overview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);
    render_project(frame, panes[0], app);
    render_details(frame, panes[1], app);
}

fn render_symbols_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    render_symbols(frame, panes[0], app);
    render_inspector(frame, panes[1], app);
}

fn render_calls_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.calls.center.is_none() {
        frame.render_widget(
            Paragraph::new("No symbol selected\n\nOpen Symbols with 2,\nsearch for a symbol,\nthen press Enter or c.")
                .alignment(Alignment::Center)
                .block(Block::default().title(" CALLS ").borders(Borders::ALL)),
            area,
        );
        return;
    }
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(32),
            Constraint::Percentage(36),
            Constraint::Percentage(32),
        ])
        .split(area);
    render_call_list(frame, panes[0], app, true);
    render_call_current(frame, panes[1], app);
    render_call_list(frame, panes[2], app, false);
}

fn render_explorer_view(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    render_explorer_tree(frame, panes[0], app);
    if app.explorer.right_mode == ExplorerRightMode::Inspector {
        render_inspector_block(frame, panes[1], app, app.pane == Pane::ExplorerFile);
    } else {
        render_explorer_file(frame, panes[1], app);
    }
}

fn render_explorer_tree(frame: &mut Frame<'_>, area: Rect, app: &App) {
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
    let rows: Vec<_> = app
        .explorer
        .visible_entries()
        .iter()
        .map(|entry| {
            let indent = "  ".repeat(entry.depth);
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
                RepoEntryKind::File => "  ",
            };
            let role = file_role(&app.project, &entry.path)
                .map(|role| format!(" [{role}]"))
                .unwrap_or_default();
            let line = format!("{indent}{marker}{name}{role}");
            ListItem::new(if entry.kind == RepoEntryKind::Directory {
                Line::styled(line, Style::default().add_modifier(Modifier::BOLD))
            } else {
                Line::raw(line)
            })
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.explorer.selected));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(Style::default().fg(Color::Cyan))
            .block(pane_block(&title, app.pane == Pane::ExplorerTree)),
        area,
        &mut state,
    );
}

fn render_explorer_file(frame: &mut Frame<'_>, area: Rect, app: &App) {
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

    let details_height = 9;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(details_height), Constraint::Min(3)])
        .split(area);
    let package = package_for_file(&app.project, &file.path)
        .map(|package| package.name.as_str())
        .unwrap_or("—");
    let role = file_role(&app.project, &file.path).unwrap_or("file");
    frame.render_widget(
        Paragraph::new(vec![
            heading(&file.path.display().to_string()),
            Line::raw(""),
            Line::raw(format!("type      {}", file_type(&file.path))),
            Line::raw(format!("size      {}", format_size(file.size))),
            Line::raw(format!("package   {package}")),
            Line::raw(format!("role      {role}")),
        ])
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
                .block(Block::default().title(" SYMBOLS ").borders(Borders::ALL)),
            rows[1],
        );
        return;
    }
    if let Some(error) = &file.error {
        frame.render_widget(
            Paragraph::new(format!("error: {error}"))
                .wrap(Wrap { trim: false })
                .block(Block::default().title(" SYMBOLS ").borders(Borders::ALL)),
            rows[1],
        );
        return;
    }
    if file.loading {
        frame.render_widget(
            Paragraph::new("Loading file symbols…")
                .block(Block::default().title(" SYMBOLS ").borders(Borders::ALL)),
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
            .highlight_style(Style::default().fg(Color::Cyan))
            .block(Block::default().title(title).borders(Borders::ALL)),
        rows[1],
        &mut state,
    );
}

fn file_type(path: &std::path::Path) -> String {
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

fn format_size(size: Option<u64>) -> String {
    match size {
        Some(size) if size >= 1024 => format!("{:.1} KiB", size as f64 / 1024.0),
        Some(size) => format!("{size} B"),
        None => "—".into(),
    }
}

fn render_call_list(frame: &mut Frame<'_>, area: Rect, app: &App, incoming: bool) {
    let (items, selection, loading, truncated, error, pane, label) = if incoming {
        (
            &app.calls.callers,
            app.calls.callers_selection,
            app.calls.callers_loading,
            app.calls.callers_truncated,
            app.calls.callers_error.as_deref(),
            Pane::Callers,
            "CALLERS",
        )
    } else {
        (
            &app.calls.callees,
            app.calls.callees_selection,
            app.calls.callees_loading,
            app.calls.callees_truncated,
            app.calls.callees_error.as_deref(),
            Pane::Callees,
            "CALLEES",
        )
    };
    let title = if truncated {
        format!(" {label} (first 20) ")
    } else {
        format!(" {label} ")
    };
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(format!("error: {error}"))
                .wrap(Wrap { trim: false })
                .block(pane_block(&title, app.pane == pane)),
            area,
        );
        return;
    }
    if loading && items.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("Loading {}...", label.to_lowercase()))
                .block(pane_block(&title, app.pane == pane)),
            area,
        );
        return;
    }
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("No {}", label.to_lowercase()))
                .block(pane_block(&title, app.pane == pane)),
            area,
        );
        return;
    }
    let rows: Vec<_> = items
        .iter()
        .map(|target| call_target_row(target, &app.project.workspace_root))
        .collect();
    let mut state = ListState::default().with_selected(Some(selection));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(pane_block(&title, app.pane == pane)),
        area,
        &mut state,
    );
}

fn call_target_row(target: &CallTarget, workspace_root: &std::path::Path) -> ListItem<'static> {
    let external = !target.is_workspace_local(workspace_root);
    let name = if external {
        format!("[ext] {}", call_target_name(target))
    } else {
        call_target_name(target)
    };
    ListItem::new(vec![
        Line::raw(name),
        Line::styled(
            format!(
                "  {}:{}",
                compact_call_path(&target.file, workspace_root),
                target.selection_range.start.line + 1
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn render_call_current(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let target = app.calls.center.as_ref().expect("checked by calls view");
    let mut text = vec![heading(&call_target_name(target))];
    if target
        .detail
        .as_deref()
        .is_some_and(|detail| !detail.is_empty() && !detail_is_container(detail))
    {
        text.push(Line::styled(
            target.detail.clone().unwrap_or_default(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    text.extend([
        Line::raw(""),
        Line::raw(format!("kind      {}", target.kind)),
        Line::raw(format!(
            "location  {}:{}",
            compact_call_path(&target.file, &app.project.workspace_root),
            target.selection_range.start.line + 1
        )),
        Line::raw(""),
        Line::raw(format!(
            "callers   {}",
            call_count(
                app.calls.callers_loading,
                app.calls.callers_error.as_ref(),
                app.calls.callers.len()
            )
        )),
        Line::raw(format!(
            "callees   {}",
            call_count(
                app.calls.callees_loading,
                app.calls.callees_error.as_ref(),
                app.calls.callees.len()
            )
        )),
        Line::raw(""),
        Line::raw(format!("history   {}", app.calls.history.len())),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(pane_block(
                &format!(" CURRENT · {} ", app.calls.scope.label()),
                app.pane == Pane::CallCurrent,
            )),
        area,
    );
}

fn compact_call_path(path: &std::path::Path, workspace_root: &std::path::Path) -> String {
    if let Ok(relative) = path.strip_prefix(workspace_root) {
        return relative.display().to_string();
    }
    if !path.is_absolute() {
        return path.display().to_string();
    }
    let components: Vec<_> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    if let Some(index) = components
        .iter()
        .position(|component| *component == "library")
    {
        return components[index + 1..].join("/");
    }
    if let Some(registry) = components
        .iter()
        .position(|component| *component == "registry")
    {
        if let Some(source) = components[registry + 1..]
            .iter()
            .position(|component| *component == "src")
        {
            let start = registry + source + 3;
            if start < components.len() {
                return components[start..].join("/");
            }
        }
    }
    components[components.len().saturating_sub(4)..].join("/")
}

fn call_target_name(target: &CallTarget) -> String {
    target
        .detail
        .as_deref()
        .filter(|detail| detail_is_container(detail))
        .map_or_else(
            || target.name.clone(),
            |detail| format!("{detail}::{}", target.name),
        )
}

fn detail_is_container(detail: &str) -> bool {
    detail
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, '_' | ':'))
}

fn call_count(loading: bool, error: Option<&String>, count: usize) -> String {
    if error.is_some() {
        "error".into()
    } else if loading {
        "loading...".into()
    } else {
        count.to_string()
    }
}

fn render_project(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut items = vec![ListItem::new(vec![
        Line::styled("▼ Project", Style::default().add_modifier(Modifier::BOLD)),
        Line::raw(format!("  {}", project_name(&app.project))),
    ])];
    for entrypoint in app.project.binary_entrypoints() {
        items.push(ListItem::new(format!(
            "Entrypoint\n  {}",
            entrypoint.display()
        )));
    }
    for package in &app.project.packages {
        items.push(ListItem::new(format!("Crate\n  {}", package.name)));
    }
    let mut state = ListState::default().with_selected(Some(app.project_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
            .block(pane_block(" PROJECT ", app.pane == Pane::Project)),
        area,
        &mut state,
    );
}

fn render_details(frame: &mut Frame<'_>, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(overview_details(app))
            .wrap(Wrap { trim: false })
            .block(pane_block(" DETAILS ", app.pane == Pane::Details)),
        area,
    );
}

fn overview_details(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        heading("Workspace"),
        Line::raw(""),
        Line::raw(format!("crates       {}", app.project.packages.len())),
        Line::raw(format!("rust files   {}", app.project.rust_files.len())),
        Line::raw(""),
        heading("Entrypoints"),
    ];
    lines.extend(
        app.project
            .binary_entrypoints()
            .into_iter()
            .map(|path| Line::raw(path.display().to_string())),
    );
    lines.push(Line::raw(""));
    lines.push(heading("Analyzer"));
    lines.push(Line::raw(match &app.analyzer_state {
        AnalyzerState::Starting => "starting...".into(),
        AnalyzerState::Indexing => "indexing...".into(),
        AnalyzerState::Ready => "ready".into(),
        AnalyzerState::Error(message) => format!("error: {message}"),
    }));
    if let Some(notice) = &app.notice {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            notice.clone(),
            Style::default().fg(Color::Yellow),
        ));
    }
    lines
}

fn render_symbols(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let active = app.pane == Pane::Symbols;
    if app.symbols_query.is_empty() && app.symbols_results.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                heading("Search-first symbol browser"),
                Line::raw(""),
                Line::raw("Press / and start typing"),
                Line::raw(""),
                Line::raw("Examples:"),
                Line::raw("  LspClient"),
                Line::raw("  Engine::run"),
                Line::raw("  request"),
            ])
            .block(pane_block(" SYMBOLS ", active)),
            area,
        );
        return;
    }

    if app.symbols_results.is_empty() {
        let message = if app.symbols_loading {
            "Searching..."
        } else {
            "No symbols found"
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw(format!("query: {}", app.symbols_query)),
                Line::raw(""),
                Line::raw(message),
            ])
            .block(pane_block(" SYMBOLS ", active)),
            area,
        );
        return;
    }

    let inner_width = area.width.saturating_sub(4) as usize;
    let items: Vec<_> = app
        .symbols_results
        .iter()
        .map(|symbol| {
            let kind = truncate(&symbol.kind, 8);
            let file = format!(
                "{}:{}",
                symbol.file.display(),
                symbol.selection_range.start.line + 1
            );
            let fixed = 8 + 2 + file.chars().count().min(18) + 1;
            let name_width = inner_width.saturating_sub(fixed).max(8);
            ListItem::new(format!(
                "{kind:<8} {:<name_width$} {}",
                truncate(&qualified_symbol(symbol), name_width),
                truncate(&file, 18)
            ))
        })
        .collect();
    let title = if app.symbols_loading {
        format!(" SYMBOLS · {} · searching… ", app.symbols_query)
    } else if app.symbols_truncated {
        format!(" SYMBOLS · {} · first 50 ", app.symbols_query)
    } else {
        format!(" SYMBOLS · {} ", app.symbols_query)
    };
    let mut state = ListState::default().with_selected(Some(app.symbols_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(pane_block(&title, active)),
        area,
        &mut state,
    );
}

fn render_inspector(frame: &mut Frame<'_>, area: Rect, app: &App) {
    render_inspector_block(frame, area, app, app.pane == Pane::Inspector);
}

fn render_inspector_block(frame: &mut Frame<'_>, area: Rect, app: &App, active: bool) {
    let (text, scroll) = app.inspector.as_ref().map_or_else(
        || {
            (
                vec![
                    heading("No symbol selected"),
                    Line::raw(""),
                    Line::raw("Choose a symbol and press Enter."),
                ],
                0,
            )
        },
        |inspector| (inspector_text(inspector), inspector.scroll as u16),
    );
    frame.render_widget(
        Paragraph::new(text)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false })
            .block(pane_block(" INSPECTOR ", active)),
        area,
    );
}

fn inspector_text(inspector: &Inspector) -> Vec<Line<'static>> {
    let definition = match &inspector.definition {
        None => "loading...".into(),
        Some(None) => "—".into(),
        Some(Some(location)) => format!(
            "{}:{}",
            location.file.display(),
            location.range.start.line + 1
        ),
    };
    let mut lines = vec![
        heading(&qualified_symbol(&inspector.symbol)),
        Line::raw(format!("Kind         {}", inspector.symbol.kind)),
        Line::raw(format!(
            "Location     {}:{}",
            inspector.symbol.file.display(),
            inspector.symbol.selection_range.start.line + 1
        )),
        Line::raw(format!(
            "Container    {}",
            inspector.symbol.container.as_deref().unwrap_or("—")
        )),
        Line::raw(format!("Definition   {definition}")),
        Line::raw(""),
        Line::raw(format!(
            "References   {}",
            section_count(inspector, "references", inspector.references)
        )),
        Line::raw(format!(
            "Callers      {}",
            section_count(inspector, "calls", inspector.callers)
        )),
        Line::raw(format!(
            "Callees      {}",
            section_count(inspector, "calls", inspector.callees)
        )),
        Line::raw(format!(
            "Problems     {}",
            section_count(inspector, "diagnostics", inspector.diagnostics)
        )),
        Line::raw(""),
        heading("Hover"),
    ];
    lines.extend(
        inspector
            .hover
            .as_ref()
            .map(|value| value.as_deref().unwrap_or("—"))
            .unwrap_or("loading...")
            .lines()
            .map(|line| Line::raw(line.to_owned())),
    );
    lines.push(Line::raw(""));
    lines.push(heading("Source"));
    lines.extend(
        inspector
            .source
            .as_deref()
            .unwrap_or("loading...")
            .lines()
            .map(|line| Line::raw(line.to_owned())),
    );
    if !inspector.errors.is_empty() {
        lines.push(Line::raw(""));
        lines.push(heading("Errors"));
        lines.extend(inspector.errors.iter().map(|(section, error)| {
            Line::styled(
                format!("{section}: {error}"),
                Style::default().fg(Color::Red),
            )
        }));
    }
    lines
}

fn footer_text(app: &App) -> &'static str {
    match app.overlay {
        Overlay::Search => "type search   ↑/↓ or Ctrl+j/k results   Enter inspect   Esc close",
        Overlay::Help => "Esc / ? close help",
        Overlay::None
            if app.pane == Pane::ExplorerFile
                && app.explorer.right_mode == ExplorerRightMode::Inspector =>
        {
            "j/k scroll   c calls   Ctrl+h tree   Esc file   1/2/3/4 views   q quit"
        }
        Overlay::None if app.pane == Pane::ExplorerFile => {
            "j/k move   Enter inspect   Ctrl+h tree   Esc back   1/2/3/4 views   q quit"
        }
        Overlay::None if app.pane == Pane::ExplorerTree => {
            "j/k move   h/l collapse/expand   Enter open   Ctrl+l file   1/2/3/4 views   q quit"
        }
        Overlay::None if app.pane == Pane::Inspector => {
            "j/k scroll   c calls   Ctrl+h symbols   Esc back   1/2/3/4 views   q quit"
        }
        Overlay::None if app.pane == Pane::Symbols => {
            "j/k move   Enter inspect   c calls   / search   Ctrl+l inspector   1/2/3/4 views   q quit"
        }
        Overlay::None if app.pane == Pane::Callers => {
            if app.calls.scope == crate::model::CallScope::Workspace {
                "j/k move   Enter follow   e external   Ctrl+l current   Esc center   q quit"
            } else {
                "j/k move   Enter follow   e workspace-only   Ctrl+l current   Esc center   q quit"
            }
        }
        Overlay::None if app.pane == Pane::CallCurrent => {
            if app.calls.scope == crate::model::CallScope::Workspace {
                "Ctrl+h callers   Ctrl+l callees   e external   Esc back   1/2/3/4 views   q quit"
            } else {
                "Ctrl+h callers   Ctrl+l callees   e workspace-only   Esc back   q quit"
            }
        }
        Overlay::None if app.pane == Pane::Callees => {
            if app.calls.scope == crate::model::CallScope::Workspace {
                "j/k move   Enter follow   e external   Ctrl+h current   Esc center   q quit"
            } else {
                "j/k move   Enter follow   e workspace-only   Ctrl+h current   Esc center   q quit"
            }
        }
        Overlay::None => {
            "1 Overview   2 Symbols   3 Calls   4 Explorer   / search   Ctrl+h/l panes   q quit"
        }
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered(area, 62, 90);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(
            "Views\n\n  1              Overview\n  2              Symbols\n  3              Calls\n  4              Explorer\n\nNavigation\n\n  j / k          move or scroll\n  Ctrl+h/l       focus left/right\n  Tab / Shift+Tab next / previous pane\n\nExplorer\n\n  j / k          move\n  h / l          collapse / expand\n  Enter          open directory / file / symbol\n\nActions\n\n  c              calls for inspected symbol\n  e              toggle external calls\n  Esc            back\n  /              search\n  ?              close help\n  q / Ctrl+C     quit",
        )
        .block(Block::default().title(" Help ").borders(Borders::ALL)),
        popup,
    );
}

fn render_search(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let popup = centered(area, 72, 70);
    frame.render_widget(Clear, popup);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(popup);
    let status = if app.symbols_loading {
        "  searching…"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(format!("> {}{status}", app.symbols_query)).block(
            Block::default()
                .title(" Search symbols ")
                .borders(Borders::ALL),
        ),
        rows[0],
    );
    let items: Vec<_> = app
        .symbols_results
        .iter()
        .map(|symbol| {
            ListItem::new(format!(
                "{:<10} {:<30} {}:{}",
                symbol.kind,
                truncate(&qualified_symbol(symbol), 30),
                symbol.file.display(),
                symbol.selection_range.start.line + 1
            ))
        })
        .collect();
    let title = if app.symbols_truncated {
        " Results (first 50) "
    } else if app.symbols_results.is_empty()
        && !app.symbols_query.is_empty()
        && !app.symbols_loading
    {
        " No symbols found "
    } else {
        " Results "
    };
    let mut state = ListState::default()
        .with_selected((!app.symbols_results.is_empty()).then_some(app.symbols_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(Style::default().fg(Color::Cyan))
            .block(Block::default().title(title).borders(Borders::ALL)),
        rows[1],
        &mut state,
    );
}

fn pane_block<'a>(title: &'a str, active: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { Color::Cyan } else { Color::DarkGray }))
}

fn heading(value: &str) -> Line<'static> {
    Line::styled(
        value.to_owned(),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

fn centered(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn section_count(inspector: &Inspector, section: &str, value: Option<usize>) -> String {
    if inspector.errors.iter().any(|(name, _)| name == section) {
        return "error".into();
    }
    value.map_or_else(|| "loading...".into(), |value| value.to_string())
}

fn truncate(value: &str, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        return value.to_owned();
    }
    let mut result: String = value.chars().take(maximum.saturating_sub(1)).collect();
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CallScope, Location, Position, Range, Symbol};
    use crate::project::{Package, Project, Target};
    use crate::repository::{RepoEntry, RepoEntryKind, RepositoryTree};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> App {
        App::new(Project {
            workspace_root: "/workspace/rust_watcher".into(),
            packages: Vec::new(),
            rust_files: Vec::new(),
        })
    }

    fn symbol() -> Symbol {
        Symbol {
            id: "lsp-client".into(),
            name: "request".into(),
            kind: "method".into(),
            file: "src/lsp.rs".into(),
            range: Range {
                start: Position {
                    line: 118,
                    character: 0,
                },
                end: Position {
                    line: 122,
                    character: 1,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 118,
                    character: 17,
                },
                end: Position {
                    line: 118,
                    character: 24,
                },
            },
            container: Some("LspClient".into()),
        }
    }

    fn render_text(app: &App) -> String {
        let backend = TestBackend::new(120, 34);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                output.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            output.push('\n');
        }
        output
    }

    fn inspected_app() -> App {
        let mut app = app();
        app.analyzer_state = AnalyzerState::Ready;
        app.switch_view(View::Symbols);
        app.symbols_query = "Lsp".into();
        app.symbols_results = vec![symbol()];
        app.pane = Pane::Inspector;
        app.inspector = Some(Inspector {
            request_id: 1,
            symbol: symbol(),
            source: Some(
                " 118 | impl LspClient {\n>119 |     async fn request() {}\n 120 | }".into(),
            ),
            hover: Some(Some("async fn LspClient::request".into())),
            definition: Some(Some(Location {
                file: "src/lsp.rs".into(),
                range: Range {
                    start: Position {
                        line: 118,
                        character: 0,
                    },
                    end: Position {
                        line: 118,
                        character: 7,
                    },
                },
            })),
            references: Some(11),
            callers: Some(9),
            callees: Some(4),
            diagnostics: Some(0),
            errors: Vec::new(),
            scroll: 0,
        });
        app
    }

    fn call_target(name: &str, detail: Option<&str>, line: u32) -> CallTarget {
        CallTarget {
            name: name.into(),
            kind: "function".into(),
            file: "src/lsp.rs".into(),
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line: line + 2,
                    character: 1,
                },
            },
            selection_range: Range {
                start: Position { line, character: 3 },
                end: Position {
                    line,
                    character: 10,
                },
            },
            detail: detail.map(str::to_owned),
        }
    }

    fn calls_app(center: CallTarget) -> App {
        let mut app = inspected_app();
        app.switch_view(View::Calls);
        app.calls.center = Some(center);
        app.calls.callers = vec![
            call_target("execute", None, 80),
            call_target("run_command", None, 90),
        ];
        app.calls.callees = vec![
            call_target("write_message", None, 490),
            call_target("timeout", None, 510),
        ];
        app.calls.callers_loading = false;
        app.calls.callees_loading = false;
        app
    }

    fn explorer_symbol(name: &str, kind: &str, line: u32, container: Option<&str>) -> Symbol {
        Symbol {
            id: format!("worker-{line}-{name}"),
            name: name.into(),
            kind: kind.into(),
            file: "src/tui/worker.rs".into(),
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line: line + 3,
                    character: 1,
                },
            },
            selection_range: Range {
                start: Position { line, character: 4 },
                end: Position {
                    line,
                    character: 4 + name.len() as u32,
                },
            },
            container: container.map(str::to_owned),
        }
    }

    fn explorer_app() -> App {
        let project = Project {
            workspace_root: "/workspace/rust_watcher".into(),
            packages: vec![Package {
                name: "rust_watcher".into(),
                manifest_path: "/workspace/rust_watcher/Cargo.toml".into(),
                targets: vec![Target {
                    name: "wt".into(),
                    crate_root: "/workspace/rust_watcher/src/main.rs".into(),
                    kinds: vec!["bin".into()],
                }],
                dependencies: Vec::new(),
            }],
            rust_files: Vec::new(),
        };
        let mut app = App::new(project);
        app.analyzer_state = AnalyzerState::Ready;
        app.explorer.apply_tree(RepositoryTree {
            entries: vec![
                RepoEntry {
                    path: ".github".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: ".github/workflows".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: ".github/workflows/ci.yml".into(),
                    kind: RepoEntryKind::File,
                    size: Some(300),
                },
                RepoEntry {
                    path: "src".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/main.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(9000),
                },
                RepoEntry {
                    path: "src/tui".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/tui/app.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(22000),
                },
                RepoEntry {
                    path: "src/tui/worker.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(14000),
                },
                RepoEntry {
                    path: "tests".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "Cargo.toml".into(),
                    kind: RepoEntryKind::File,
                    size: Some(800),
                },
                RepoEntry {
                    path: "README.md".into(),
                    kind: RepoEntryKind::File,
                    size: Some(7000),
                },
            ],
            truncated: false,
            skipped_errors: 0,
        });
        app.explorer.set_expanded("src".into(), true);
        app.explorer.set_expanded("src/tui".into(), true);
        app.explorer.selected = app
            .explorer
            .visible_entries()
            .iter()
            .position(|entry| entry.path == std::path::Path::new("src/tui/worker.rs"))
            .unwrap();
        let (id, file) = app.explorer.open_selected().unwrap();
        app.explorer.apply_file_symbols(
            id,
            &file,
            vec![
                explorer_symbol("AnalyzerCommand", "enum", 8, None),
                explorer_symbol("AnalyzerEvent", "enum", 20, None),
                explorer_symbol("Scheduler", "struct", 100, None),
                explorer_symbol("push", "method", 110, Some("Scheduler")),
                explorer_symbol("analyzer_worker", "function", 160, None),
                explorer_symbol("run_phase", "function", 220, None),
            ],
        );
        app.switch_view(View::Explorer);
        app
    }

    #[test]
    fn symbols_and_inspector_render_targeted_content() {
        let text = render_text(&inspected_app());
        for expected in [
            "SYMBOLS",
            "INSPECTOR",
            "LspClient::request",
            "src/lsp.rs",
            "method",
            "Definition",
            "References",
            "Callers",
            "Callees",
            "Source",
        ] {
            assert!(text.contains(expected), "missing {expected} in render");
        }
    }

    #[test]
    fn overview_and_help_render_views_and_navigation() {
        let mut app = app();
        app.analyzer_state = AnalyzerState::Ready;
        app.inspector = inspected_app().inspector;
        let overview = render_text(&app);
        assert!(overview.contains("[1 Overview]"));
        assert!(overview.contains("[2 Symbols]"));
        assert!(overview.contains("Workspace"));
        assert!(overview.contains("Entrypoints"));
        assert!(!overview.contains("LspClient::request"));
        app.overlay = Overlay::Help;
        let help = render_text(&app);
        for expected in [
            "1",
            "Overview",
            "2",
            "Symbols",
            "Ctrl+h",
            "Shift+Tab",
            "Esc",
        ] {
            assert!(help.contains(expected), "missing {expected} in help");
        }
    }

    #[test]
    fn symbols_empty_state_and_small_terminal_are_safe() {
        let mut app = app();
        app.switch_view(View::Symbols);
        assert!(render_text(&app).contains("Press / and start typing"));
        let backend = TestBackend::new(70, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Terminal too small"));
    }

    #[test]
    fn calls_render_populated_and_empty_states() {
        let populated_app = calls_app(call_target("request", Some("LspClient"), 134));
        let populated = render_text(&populated_app);
        for expected in [
            "CALLERS",
            "CURRENT",
            "CALLEES",
            "execute",
            "LspClient::request",
            "write_message",
        ] {
            assert!(
                populated.contains(expected),
                "missing {expected} in calls render"
            );
        }
        let mut empty = app();
        empty.switch_view(View::Calls);
        assert!(render_text(&empty).contains("No symbol selected"));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &populated_app))
            .unwrap();
    }

    #[test]
    fn calls_scope_hides_external_targets_by_default_and_compacts_them_in_all_mode() {
        let workspace = calls_app(call_target("request", Some("LspClient"), 134));
        let workspace_render = render_text(&workspace);
        assert!(workspace_render.contains("CURRENT · workspace"));
        assert!(!workspace_render.contains("[ext]"));

        let mut all = workspace;
        all.calls.scope = CallScope::All;
        let mut external = call_target("unwrap", Some("Option"), 123);
        external.file =
            "/home/poke/.rustup/toolchains/stable/lib/rustlib/src/rust/library/core/src/option.rs"
                .into();
        all.calls.callees = vec![external];
        let all_render = render_text(&all);
        assert!(all_render.contains("CURRENT · all"));
        assert!(all_render.contains("[ext] Option::unwrap"));
        assert!(all_render.contains("core/src/option.rs"));
        assert!(!all_render.contains("/home/poke/.rustup"));
    }

    #[test]
    fn symbol_results_render_relevant_request_names_before_late_substrings() {
        let mut app = app();
        app.switch_view(View::Symbols);
        app.symbols_query = "req".into();
        let mut exact = symbol();
        exact.name = "request".into();
        let mut weak = symbol();
        weak.name = "exports_symbols_review_frames_when_requested".into();
        weak.id = "weak".into();
        weak.selection_range.start.line += 10;
        app.symbols_results = vec![exact, weak];
        let rendered = render_text(&app);
        assert!(rendered.find("request").unwrap() < rendered.find("exports").unwrap());
    }

    #[test]
    fn explorer_renders_tree_file_symbols_non_rust_and_truncation() {
        let mut app = explorer_app();
        app.pane = Pane::ExplorerFile;
        let populated = render_text(&app);
        for expected in [
            "PROJECT TREE",
            "FILE",
            "src",
            "worker.rs",
            "Cargo.toml",
            "AnalyzerCommand",
            "AnalyzerEvent",
            "Scheduler",
            "analyzer_worker",
            "run_phase",
        ] {
            assert!(populated.contains(expected), "missing {expected}");
        }

        app.explorer.tree.as_mut().unwrap().truncated = true;
        assert!(render_text(&app).contains("truncated"));
        app.explorer.selected = app
            .explorer
            .visible_entries()
            .iter()
            .position(|entry| entry.path == std::path::Path::new("README.md"))
            .unwrap();
        assert!(app.explorer.open_selected().is_none());
        assert!(render_text(&app).contains("Semantic symbols unavailable"));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
    }

    #[test]
    fn explorer_tree_remains_available_when_analyzer_failed() {
        let mut app = explorer_app();
        app.analyzer_state = AnalyzerState::Error("rust-analyzer unavailable".into());
        app.pane = Pane::ExplorerTree;
        let rendered = render_text(&app);
        assert!(matches!(app.analyzer_state, AnalyzerState::Error(_)));
        assert!(rendered.contains("PROJECT TREE"));
        assert!(rendered.contains("worker.rs"));
    }

    #[test]
    fn exports_symbols_review_frames_when_requested() {
        let Some(directory) = std::env::var_os("WT_TUI_REVIEW_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut overview = app();
        overview.analyzer_state = AnalyzerState::Ready;
        std::fs::write(directory.join("01-overview.txt"), render_text(&overview)).unwrap();

        let mut symbols = inspected_app();
        symbols.pane = Pane::Symbols;
        std::fs::write(directory.join("02-symbols.txt"), render_text(&symbols)).unwrap();
        symbols.pane = Pane::Inspector;
        std::fs::write(directory.join("03-inspector.txt"), render_text(&symbols)).unwrap();

        let mut initial = calls_app(call_target("request", Some("LspClient"), 134));
        initial.pane = Pane::CallCurrent;
        std::fs::write(
            directory.join("09-calls-initial.txt"),
            render_text(&initial),
        )
        .unwrap();
        let mut caller = calls_app(call_target("execute", None, 80));
        caller.calls.callers = vec![call_target("run_command", None, 70)];
        caller.calls.callees = vec![call_target("request", Some("LspClient"), 134)];
        caller.calls.history = vec![call_target("request", Some("LspClient"), 134)];
        caller.pane = Pane::Callers;
        std::fs::write(
            directory.join("10-calls-follow-caller.txt"),
            render_text(&caller),
        )
        .unwrap();
        let mut callee = calls_app(call_target("write_message", None, 490));
        callee.calls.callers = vec![call_target("request", Some("LspClient"), 134)];
        callee.calls.callees = vec![call_target("serialize", None, 500)];
        callee.calls.history = vec![
            call_target("request", Some("LspClient"), 134),
            call_target("execute", None, 80),
        ];
        callee.pane = Pane::Callees;
        std::fs::write(
            directory.join("11-calls-follow-callee.txt"),
            render_text(&callee),
        )
        .unwrap();

        let mut explorer = explorer_app();
        explorer.pane = Pane::ExplorerTree;
        std::fs::write(
            directory.join("12-explorer-tree.txt"),
            render_text(&explorer),
        )
        .unwrap();
        explorer.pane = Pane::ExplorerFile;
        std::fs::write(
            directory.join("13-explorer-file-symbols.txt"),
            render_text(&explorer),
        )
        .unwrap();
        explorer.explorer.file.as_mut().unwrap().selection = 2;
        let (_, selected) = explorer.begin_explorer_symbol_inspect().unwrap();
        explorer.inspector = Some(Inspector {
            request_id: explorer.inspect_request,
            symbol: selected,
            source: Some(
                "> 101 | struct Scheduler {\n  102 |     work: Option<InteractiveWork>,\n  103 | }"
                    .into(),
            ),
            hover: Some(Some("struct Scheduler".into())),
            definition: Some(None),
            references: Some(4),
            callers: Some(0),
            callees: Some(0),
            diagnostics: Some(0),
            errors: Vec::new(),
            scroll: 0,
        });
        std::fs::write(
            directory.join("14-explorer-inspector.txt"),
            render_text(&explorer),
        )
        .unwrap();

        let mut relevance = app();
        relevance.switch_view(View::Symbols);
        relevance.symbols_query = "req".into();
        let mut request = symbol();
        request.container = Some("LspClient".into());
        let mut request_handler = symbol();
        request_handler.name = "request_handler".into();
        request_handler.container = None;
        request_handler.id = "request-handler".into();
        request_handler.selection_range.start.line += 10;
        let mut server_request = symbol();
        server_request.name = "server_request".into();
        server_request.container = None;
        server_request.id = "server-request".into();
        server_request.selection_range.start.line += 20;
        let mut weak = symbol();
        weak.name = "exports_symbols_review_frames_when_requested".into();
        weak.container = None;
        weak.id = "weak-request".into();
        weak.selection_range.start.line += 30;
        relevance.symbols_results = vec![request, request_handler, server_request, weak];
        std::fs::write(
            directory.join("15-symbol-relevance.txt"),
            render_text(&relevance),
        )
        .unwrap();

        let workspace_calls = calls_app(call_target("request", Some("LspClient"), 134));
        std::fs::write(
            directory.join("16-calls-workspace.txt"),
            render_text(&workspace_calls),
        )
        .unwrap();
        let mut external_calls = workspace_calls;
        external_calls.calls.scope = CallScope::All;
        let mut external = call_target("unwrap", Some("Option"), 123);
        external.file =
            "/home/poke/.rustup/toolchains/stable/lib/rustlib/src/rust/library/core/src/option.rs"
                .into();
        external_calls.calls.callees.push(external);
        std::fs::write(
            directory.join("17-calls-external.txt"),
            render_text(&external_calls),
        )
        .unwrap();

        let mut clean_overview = inspected_app();
        clean_overview.switch_view(View::Overview);
        std::fs::write(
            directory.join("18-overview-after-inspector.txt"),
            render_text(&clean_overview),
        )
        .unwrap();
    }
}

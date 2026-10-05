use super::app::{
    project_name, qualified_symbol, AnalyzerState, App, Inspector, Overlay, Pane, View,
};
use crate::model::CallTarget;
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
    let rows: Vec<_> = items.iter().map(call_target_row).collect();
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

fn call_target_row(target: &CallTarget) -> ListItem<'static> {
    ListItem::new(vec![
        Line::raw(call_target_name(target)),
        Line::styled(
            format!(
                "  {}:{}",
                target.file.display(),
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
            target.file.display(),
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
            .block(pane_block(" CURRENT ", app.pane == Pane::CallCurrent)),
        area,
    );
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
    let text = if let Some(inspector) = &app.inspector {
        inspector_text(inspector)
    } else {
        overview_details(app)
    };
    frame.render_widget(
        Paragraph::new(text)
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
    let active = app.pane == Pane::Inspector;
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
        Overlay::None if app.pane == Pane::Inspector => {
            "j/k scroll   c calls   Ctrl+h symbols   Esc back   1/2/3 views   q quit"
        }
        Overlay::None if app.pane == Pane::Symbols => {
            "j/k move   Enter inspect   c calls   / search   Ctrl+l inspector   1/2/3 views   q quit"
        }
        Overlay::None if app.pane == Pane::Callers => {
            "j/k move   Enter follow   Ctrl+l current   Esc center   1/2/3 views   q quit"
        }
        Overlay::None if app.pane == Pane::CallCurrent => {
            "Ctrl+h callers   Ctrl+l callees   Esc back   1/2/3 views   q quit"
        }
        Overlay::None if app.pane == Pane::Callees => {
            "j/k move   Enter follow   Ctrl+h current   Esc center   1/2/3 views   q quit"
        }
        Overlay::None => "1 Overview   2 Symbols   3 Calls   / search   Ctrl+h/l panes   q quit",
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered(area, 62, 90);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(
            "Views\n\n  1              Overview\n  2              Symbols\n  3              Calls\n\nNavigation\n\n  j / ↓          move or scroll down\n  k / ↑          move or scroll up\n  Ctrl+h/l       focus left/right\n  Tab            next pane\n  Shift+Tab      previous pane\n\nActions\n\n  Enter          inspect / follow call\n  c              calls for inspected symbol\n  Esc            back / call history back\n  /              search\n  ?              close help\n  q / Ctrl+C     quit",
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
    use crate::model::{Location, Position, Range, Symbol};
    use crate::project::Project;
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
        let overview = render_text(&app);
        assert!(overview.contains("[1 Overview]"));
        assert!(overview.contains("[2 Symbols]"));
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
    }
}

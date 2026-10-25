mod calls;
mod explorer;
mod symbols;
use super::app::{
    project_name, qualified_symbol, AnalyzerState, App, Inspector, Overlay, Pane, View,
};
use super::explorer::{file_role, package_for_file, ExplorerRightMode};
use super::{components, theme};
use crate::model::CallTarget;
use crate::repository::RepoEntryKind;
use calls::*;
use explorer::*;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use symbols::*;

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 20;

pub(super) fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme::TEXT)),
        area,
    );
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small\nMinimum: {MIN_WIDTH}x{MIN_HEIGHT}"
            ))
            .alignment(Alignment::Center)
            .block(pane_block(" wt ", true)),
            area,
        );
        theme::apply(frame.buffer_mut(), app.color_mode);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(area);
    render_header(frame, rows[0], app);
    match app.view {
        View::Overview => render_overview(frame, rows[1], app),
        View::Symbols => render_symbols_view(frame, rows[1], app),
        View::Calls => render_calls_view(frame, rows[1], app),
        View::Explorer => render_explorer_view(frame, rows[1], app),
        View::Map => super::map::render(frame, rows[1], app),
    }
    frame.render_widget(
        Paragraph::new(footer_text(app))
            .alignment(Alignment::Left)
            .style(Style::default().fg(theme::MUTED)),
        rows[2],
    );
    match app.overlay {
        Overlay::Help => render_help(frame, area),
        Overlay::Search => render_search(frame, area, app),
        Overlay::None => {}
    }
    super::journey::render_picker(frame, area, app);
    theme::apply(frame.buffer_mut(), app.color_mode);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let views = [
        (View::Overview, "1 Overview", "1 OVR"),
        (View::Symbols, "2 Symbols", "2 SYM"),
        (View::Calls, "3 Calls", "3 CALL"),
        (View::Explorer, "4 Explorer", "4 FILE"),
        (View::Map, "5 Map", "5 MAP"),
    ];
    let short = area.width < 110;
    let mut tabs = vec![components::status_label(" wt ", theme::ACCENT)];
    tabs.push(Span::styled(
        format!(
            "{}  ",
            truncate(project_name(&app.project), if short { 12 } else { 24 })
        ),
        Style::default().fg(theme::TEXT),
    ));
    for (view, full, compact) in views {
        tabs.push(Span::styled(
            format!(
                "{}{}  ",
                if app.view == view { ">" } else { " " },
                if short { compact } else { full }
            ),
            if app.view == view {
                components::selectable_row()
            } else {
                Style::default().fg(theme::MUTED)
            },
        ));
    }
    let (ra, color) = match &app.analyzer_state {
        AnalyzerState::Starting => ("RA ◌ Starting", theme::ACCENT),
        AnalyzerState::Indexing => ("RA ◐ Indexing", theme::WARNING),
        AnalyzerState::Ready => ("RA ● Ready", theme::SUCCESS),
        AnalyzerState::Error(_) => ("RA × Error · see Analysis", theme::ERROR),
    };
    let (git, gcolor) = match &app.hotspots {
        None => ("Git ◐ Analyzing", theme::ACCENT),
        Some(r) if !r.available => ("Git – Unavailable", theme::MUTED),
        Some(r) if r.history_partial => ("Git ◐ Partial", theme::WARNING),
        _ => ("Git ● Complete", theme::SUCCESS),
    };
    let mut statuses = vec![
        components::status_label(ra, color),
        Span::raw("   "),
        components::status_label(git, gcolor),
    ];
    if app
        .hotspots
        .as_ref()
        .is_some_and(|r| r.available && r.co_change_partial)
    {
        statuses.push(components::status_label(
            "   Co-change ◐ Partial",
            theme::WARNING,
        ));
    }
    if let Some(notice) = &app.notice {
        statuses.push(Span::styled(
            format!("   {notice}"),
            Style::default().fg(theme::WARNING),
        ));
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(tabs), Line::from(statuses)]).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme::BORDER)),
        ),
        area,
    );
}

fn render_overview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);
    super::overview::render_summary(frame, rows[0], app);
    let panes = Layout::horizontal([
        Constraint::Percentage(if area.width < 100 { 42 } else { 36 }),
        Constraint::Min(25),
    ])
    .split(rows[1]);
    super::overview::render_project(frame, panes[0], app);
    super::overview::render_details(frame, panes[1], app);
}

fn footer_text(app: &App) -> &'static str {
    match app.overlay {
        Overlay::Search => " type search · ↑/↓ results · Enter inspect · Esc close · Ctrl+c quit",
        Overlay::Help => " ? / Esc close help · q quit",
        Overlay::None => match app.view {
            View::Map => {
                " j/k select · PgUp/PgDn pages · Enter focus · g/v · p review · o · Esc · q quit"
            }
            View::Overview if app.pane == Pane::Details => {
                " j/k scroll · Ctrl+h exploration · 1–5 views · ? help · q quit"
            }
            View::Overview => {
                " j/k select · Enter expand/open · Tab pane · 1–5 views · ? help · q quit"
            }
            View::Symbols => {
                " / search · j/k select · Enter inspect · c calls · Tab pane · ? help · q quit"
            }
            View::Calls => {
                " j/k · Enter follow · o source · Tab pane · e scope · Esc back · ? · q quit"
            }
            View::Explorer
                if app.explorer.right_mode == ExplorerRightMode::Inspector
                    && super::journey::source_returns_to_calls(app) =>
            {
                " j/k scroll · c calls · Esc Calls · Tab pane · ? help · q quit"
            }
            View::Explorer if app.explorer.right_mode == ExplorerRightMode::Inspector => {
                " j/k scroll · c calls · Esc file symbols · Tab pane · ? help · q quit"
            }
            View::Explorer => {
                " j/k select · h/l tree · Enter open/inspect · Tab pane · ? help · q quit"
            }
        },
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered(area, 90, 90);
    frame.render_widget(Clear, popup);
    let text = vec![
        heading("Views"),
        Line::raw("1 Overview · 2 Symbols · 3 Calls · 4 Explorer · 5 Map"),
        heading("Navigation"),
        Line::raw("j/k move or scroll · Ctrl+h/l focus panes"),
        Line::raw("Tab / Shift+Tab next / previous pane"),
        heading("Explore"),
        Line::raw("Overview: Enter expands a group or opens a file"),
        Line::raw("Symbols: / search · Enter Inspector · c Calls"),
        Line::raw("Calls: Enter follow · o exact source/Inspector · e scope · Esc back"),
        Line::raw("Explorer: h/l directories · Enter file / symbol"),
        Line::raw("Map: j/k nodes · Enter focus · m modules · o exact source"),
        Line::raw("Modules: v Hierarchy/Imports/Used by · f files · a limitations · t target"),
        Line::raw("Visual Map: g Diagram/List · p actionable priorities · c exact Calls"),
        Line::raw("Module pages: PgUp/PgDn or [/] · j/k select visible nodes only"),
        Line::raw("Map: t module target · Esc previous scope / Cargo map"),
        heading("Session"),
        Line::raw("? / Esc close help · q / Ctrl+C quit"),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(pane_block(" HELP · keyboard reference ", true)),
        popup,
    );
}

fn render_search(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let active = app.overlay == Overlay::Search;
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
        Paragraph::new(format!("> {}{status}", app.symbols_query))
            .block(pane_block(" Search symbols ", active)),
        rows[0],
    );
    let items: Vec<_> = app
        .symbols_results
        .iter()
        .map(|s| symbol_row(s, popup.width.saturating_sub(6) as usize))
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
            .highlight_style(Style::default().fg(theme::ACTIVE))
            .block(pane_block(title, active)),
        rows[1],
        &mut state,
    );
}

pub(super) fn pane_block<'a>(title: &'a str, active: bool) -> Block<'a> {
    components::styled_panel(title, active)
}

pub(super) fn heading(value: &str) -> Line<'static> {
    components::section_header(value)
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
            architecture: Default::default(),
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
            scroll_limit: Default::default(),
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
            architecture: Default::default(),
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
        assert!(overview.contains("1 Overview"));
        assert!(overview.contains("2 Symbols"));
        assert!(overview.contains("Workspace"));
        assert!(overview.contains("ENTRYPOINTS"));
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
    fn metrics_loading_ready_partial_and_errors_render_independently() {
        let mut app = app();
        assert!(render_text(&app).contains("Repository Metrics: loading..."));
        app.metrics_error = Some("cannot read repository".into());
        assert!(render_text(&app).contains("Metrics error: cannot read repository"));
        app.metrics_error = None;
        app.metrics = Some(crate::metrics::RepositoryMetrics {
            total_files: 42,
            code_nonblank_lines: 100,
            partial: true,
            ..Default::default()
        });
        app.analyzer_state = AnalyzerState::Error("analyzer unavailable".into());
        let text = render_text(&app);
        assert!(text.contains("Metrics ready · PARTIAL"));
        assert!(text.contains("Files") && text.contains("42"));
        assert!(text.contains("Source nonblank LOC") && text.contains("100"));
        assert!(text.contains("analyzer unavailable"));
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Files") && text.contains("42"));
        assert!(text.contains("RA"));
    }

    #[test]
    #[ignore = "local Overview rendering timing"]
    fn metrics_render_timing() {
        let mut app = app();
        let project = Project::discover(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let tree = crate::repository::scan(&project.workspace_root).unwrap();
        app.metrics = Some(crate::metrics::analyze(
            &project.workspace_root,
            &tree,
            &project,
        ));
        app.maintainability = Some(crate::maintainability::analyze(
            &project.workspace_root,
            &tree,
            &project,
        ));
        app.hotspots = Some(crate::git_analysis::correlate(
            crate::git_analysis::analyze(
                &project.workspace_root,
                &Default::default(),
                &Default::default(),
            ),
            app.metrics.as_ref().unwrap(),
            app.maintainability.as_ref().unwrap(),
            &tree,
        ));
        app.project = project;
        let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            terminal.draw(|f| render(f, &app)).unwrap();
        }
        eprintln!("Overview render average {:?}", start.elapsed() / 100);
        app.select_overview(2);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            terminal.draw(|f| render(f, &app)).unwrap();
        }
        eprintln!("Hotspot details render average {:?}", start.elapsed() / 100);
    }

    #[test]
    fn symbols_empty_state_and_small_terminal_are_safe() {
        let mut app = app();
        app.switch_view(View::Symbols);
        assert!(render_text(&app).contains("START EXPLORING SYMBOLS"));
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
        assert!(render_text(&empty).contains("No callable symbol selected"));

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
            scroll_limit: Default::default(),
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

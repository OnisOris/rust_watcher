use crate::model::{Diagnostic, Symbol, SymbolSearchResult};
use crate::project::Project;
use crate::rust::RustAnalyzer;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io::{self, Stdout};
use std::path::Path;
use std::sync::Once;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 20;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzerState {
    Starting,
    Indexing,
    Ready,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Project,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Search,
}

#[derive(Debug, Clone)]
struct Inspector {
    symbol: Symbol,
    source: Option<String>,
    hover: Option<String>,
    references: Option<usize>,
    callers: Option<usize>,
    callees: Option<usize>,
    diagnostics: Option<usize>,
    error: Option<String>,
}

impl Inspector {
    fn loading(symbol: Symbol) -> Self {
        Self {
            symbol,
            source: None,
            hover: None,
            references: None,
            callers: None,
            callees: None,
            diagnostics: None,
            error: None,
        }
    }
}

struct App {
    project: Project,
    analyzer_state: AnalyzerState,
    pane: Pane,
    overlay: Overlay,
    project_selection: usize,
    search_query: String,
    search_results: Vec<Symbol>,
    search_selection: usize,
    search_dirty_at: Option<Instant>,
    search_request: u64,
    search_truncated: bool,
    search_loading: bool,
    inspector: Option<Inspector>,
    notice: Option<String>,
}

impl App {
    fn new(project: Project) -> Self {
        Self {
            project,
            analyzer_state: AnalyzerState::Starting,
            pane: Pane::Project,
            overlay: Overlay::None,
            project_selection: 0,
            search_query: String::new(),
            search_results: Vec::new(),
            search_selection: 0,
            search_dirty_at: None,
            search_request: 0,
            search_truncated: false,
            search_loading: false,
            inspector: None,
            notice: None,
        }
    }

    fn project_item_count(&self) -> usize {
        1 + self.project.binary_entrypoints().len() + self.project.packages.len()
    }

    fn move_selection(&mut self, delta: isize) {
        if self.overlay == Overlay::Search {
            self.search_selection =
                move_index(self.search_selection, self.search_results.len(), delta);
        } else if self.pane == Pane::Project {
            self.project_selection =
                move_index(self.project_selection, self.project_item_count(), delta);
        }
    }

    fn switch_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Project => Pane::Details,
            Pane::Details => Pane::Project,
        };
    }

    fn activate_project_item(&mut self) {
        let entrypoints = self.project.binary_entrypoints();
        let label = if self.project_selection == 0 {
            format!("Workspace: {}", project_name(&self.project))
        } else if let Some(path) = entrypoints.get(self.project_selection - 1) {
            format!("Entrypoint: {}", path.display())
        } else {
            let package_index = self.project_selection - 1 - entrypoints.len();
            self.project
                .packages
                .get(package_index)
                .map(|package| format!("Crate: {}", package.name))
                .unwrap_or_else(|| "Workspace".into())
        };
        self.pane = Pane::Details;
        self.notice = Some(label);
    }

    fn open_search(&mut self) {
        self.overlay = Overlay::Search;
        self.search_query.clear();
        self.search_results.clear();
        self.search_selection = 0;
        self.search_dirty_at = None;
        self.search_loading = false;
    }

    fn edit_search(&mut self) {
        self.search_dirty_at = Some(Instant::now());
        self.search_loading = !self.search_query.is_empty();
    }

    fn apply_event(&mut self, event: AnalyzerEvent) {
        match event {
            AnalyzerEvent::Indexing => self.analyzer_state = AnalyzerState::Indexing,
            AnalyzerEvent::Ready => self.analyzer_state = AnalyzerState::Ready,
            AnalyzerEvent::Error(message) => self.analyzer_state = AnalyzerState::Error(message),
            AnalyzerEvent::SearchResults { id, result } if id == self.search_request => {
                self.search_results = result.items;
                self.search_truncated = result.truncated;
                self.search_selection = 0;
                self.search_loading = false;
            }
            AnalyzerEvent::SearchFailed { id, message } if id == self.search_request => {
                self.search_results.clear();
                self.search_loading = false;
                self.notice = Some(message);
            }
            AnalyzerEvent::Source(value) => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.source = Some(value);
                }
            }
            AnalyzerEvent::Hover(value) => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.hover = value;
                }
            }
            AnalyzerEvent::References(count) => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.references = Some(count);
                }
            }
            AnalyzerEvent::Calls { callers, callees } => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.callers = Some(callers);
                    inspector.callees = Some(callees);
                }
            }
            AnalyzerEvent::Diagnostics(values) => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.diagnostics = Some(values.len());
                }
            }
            AnalyzerEvent::InspectFailed(message) => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.error = Some(message);
                }
            }
            AnalyzerEvent::SearchResults { .. } | AnalyzerEvent::SearchFailed { .. } => {}
        }
    }
}

fn move_index(current: usize, count: usize, delta: isize) -> usize {
    if count == 0 {
        return 0;
    }
    current.saturating_add_signed(delta).min(count - 1)
}

enum AnalyzerCommand {
    Search { id: u64, query: String },
    Inspect(Symbol),
    Shutdown,
}

enum AnalyzerEvent {
    Indexing,
    Ready,
    Error(String),
    SearchResults { id: u64, result: SymbolSearchResult },
    SearchFailed { id: u64, message: String },
    Source(String),
    Hover(Option<String>),
    References(usize),
    Calls { callers: usize, callees: usize },
    Diagnostics(Vec<Diagnostic>),
    InspectFailed(String),
}

pub async fn run(project: Project) -> Result<()> {
    install_panic_hook();
    let mut terminal = TerminalSession::enter()?;
    let mut app = App::new(project.clone());

    // The first frame intentionally precedes rust-analyzer startup.
    terminal.draw(&app)?;

    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, mut event_rx) = mpsc::channel(32);
    let analyzer_task = tokio::spawn(analyzer_worker(project, command_rx, event_tx));
    let result = event_loop(&mut terminal, &mut app, &command_tx, &mut event_rx).await;
    let _ = command_tx.send(AnalyzerCommand::Shutdown).await;
    let _ = analyzer_task.await;
    result
}

async fn event_loop(
    terminal: &mut TerminalSession,
    app: &mut App,
    commands: &mpsc::Sender<AnalyzerCommand>,
    events: &mut mpsc::Receiver<AnalyzerEvent>,
) -> Result<()> {
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            event = events.recv() => {
                if let Some(event) = event {
                    app.apply_event(event);
                    terminal.draw(app)?;
                }
            }
            _ = tick.tick() => {
                while event::poll(Duration::ZERO)? {
                    match event::read()? {
                        Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                            if handle_key(app, key, commands).await? {
                                return Ok(());
                            }
                            terminal.draw(app)?;
                        }
                        Event::Resize(_, _) => terminal.draw(app)?,
                        _ => {}
                    }
                }
                if app.search_dirty_at.is_some_and(|at| at.elapsed() >= SEARCH_DEBOUNCE) {
                    app.search_dirty_at = None;
                    app.search_request += 1;
                    let id = app.search_request;
                    let query = app.search_query.clone();
                    if query.is_empty() {
                        app.search_results.clear();
                        app.search_loading = false;
                    } else {
                        commands.send(AnalyzerCommand::Search { id, query }).await?;
                    }
                    terminal.draw(app)?;
                }
            }
        }
    }
}

async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    commands: &mpsc::Sender<AnalyzerCommand>,
) -> Result<bool> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }
    match app.overlay {
        Overlay::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('?') => app.overlay = Overlay::None,
            _ => {}
        },
        Overlay::Search => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Down => app.move_selection(1),
            KeyCode::Up => app.move_selection(-1),
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_selection(1)
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_selection(-1)
            }
            KeyCode::Backspace => {
                app.search_query.pop();
                app.edit_search();
            }
            KeyCode::Enter => {
                if let Some(symbol) = app.search_results.get(app.search_selection).cloned() {
                    app.inspector = Some(Inspector::loading(symbol.clone()));
                    app.overlay = Overlay::None;
                    app.pane = Pane::Details;
                    commands.send(AnalyzerCommand::Inspect(symbol)).await?;
                }
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                app.search_query.push(character);
                app.edit_search();
            }
            _ => {}
        },
        Overlay::None => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => app.overlay = Overlay::Help,
            KeyCode::Char('/') => app.open_search(),
            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
            KeyCode::Tab | KeyCode::BackTab => app.switch_pane(),
            KeyCode::Enter => app.activate_project_item(),
            KeyCode::Esc => app.notice = None,
            _ => {}
        },
    }
    Ok(false)
}

async fn analyzer_worker(
    project: Project,
    mut commands: mpsc::Receiver<AnalyzerCommand>,
    events: mpsc::Sender<AnalyzerEvent>,
) {
    let mut analyzer = match RustAnalyzer::start(project, Path::new("rust-analyzer")).await {
        Ok(analyzer) => analyzer,
        Err(error) => {
            let _ = events
                .send(AnalyzerEvent::Error(format!("{error:#}")))
                .await;
            return;
        }
    };
    let _ = events.send(AnalyzerEvent::Indexing).await;
    let mut ready_sent = false;
    let mut error_sent = false;
    loop {
        if !error_sent {
            if let Some(error) = analyzer.error() {
                error_sent = true;
                let _ = events.send(AnalyzerEvent::Error(error)).await;
            }
        }
        if !ready_sent && analyzer.is_ready() {
            ready_sent = true;
            let _ = events.send(AnalyzerEvent::Ready).await;
        }
        tokio::select! {
            command = commands.recv() => match command {
                Some(AnalyzerCommand::Search { id, query }) => {
                    match analyzer.symbols(&query).await {
                        Ok(result) => { let _ = events.send(AnalyzerEvent::SearchResults { id, result }).await; }
                        Err(error) => { let _ = events.send(AnalyzerEvent::SearchFailed { id, message: format!("{error:#}") }).await; }
                    }
                }
                Some(AnalyzerCommand::Inspect(symbol)) => {
                    inspect_symbol(&mut analyzer, symbol, &events).await;
                }
                Some(AnalyzerCommand::Shutdown) | None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
    analyzer.shutdown().await;
}

async fn inspect_symbol(
    analyzer: &mut RustAnalyzer,
    symbol: Symbol,
    events: &mpsc::Sender<AnalyzerEvent>,
) {
    match analyzer.source_for_symbol(&symbol) {
        Ok(source) => {
            let _ = events.send(AnalyzerEvent::Source(source)).await;
        }
        Err(error) => {
            let _ = events
                .send(AnalyzerEvent::InspectFailed(format!("{error:#}")))
                .await;
        }
    }
    match analyzer.hover(&symbol).await {
        Ok(hover) => {
            let _ = events.send(AnalyzerEvent::Hover(hover)).await;
        }
        Err(error) => {
            let _ = events
                .send(AnalyzerEvent::InspectFailed(format!("{error:#}")))
                .await;
        }
    }
    match analyzer.references(&symbol).await {
        Ok(references) => {
            let _ = events
                .send(AnalyzerEvent::References(references.len()))
                .await;
        }
        Err(error) => {
            let _ = events
                .send(AnalyzerEvent::InspectFailed(format!("{error:#}")))
                .await;
        }
    }
    let callers = analyzer.calls(&symbol, 1, true).await;
    let callees = analyzer.calls(&symbol, 1, false).await;
    match (callers, callees) {
        (Ok(callers), Ok(callees)) => {
            let _ = events
                .send(AnalyzerEvent::Calls {
                    callers: callers.nodes.len(),
                    callees: callees.nodes.len(),
                })
                .await;
        }
        (Err(error), _) | (_, Err(error)) => {
            let _ = events
                .send(AnalyzerEvent::InspectFailed(format!("{error:#}")))
                .await;
        }
    }
    match analyzer.diagnostics_for_file(&symbol.file).await {
        Ok(values) => {
            let _ = events.send(AnalyzerEvent::Diagnostics(values)).await;
        }
        Err(error) => {
            let _ = events
                .send(AnalyzerEvent::InspectFailed(format!("{error:#}")))
                .await;
        }
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        let terminal = match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
                return Err(error.into());
            }
        };
        Ok(Self { terminal })
    }

    fn draw(&mut self, app: &App) -> Result<()> {
        self.terminal.draw(|frame| render(frame, app))?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
            previous(info);
        }));
    });
}

fn render(frame: &mut Frame<'_>, app: &App) {
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
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[1]);
    render_project(frame, panes[0], app);
    render_details(frame, panes[1], app);
    frame.render_widget(
        Paragraph::new("/ search   Tab pane   Enter inspect   ? help   q quit")
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
        AnalyzerState::Error(message) => (format!("× {}", truncate(message, 24)), Color::Red),
    };
    let available = area.width.saturating_sub(24) as usize;
    let title = Line::from(vec![
        Span::styled(" wt ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(truncate(workspace, available)),
        Span::raw("  "),
        Span::styled(format!("RA {status}"), Style::default().fg(color)),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        area,
    );
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
    let border = if app.pane == Pane::Project {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
            .block(
                Block::default()
                    .title(" PROJECT ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border)),
            ),
        area,
        &mut state,
    );
}

fn render_details(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let border = if app.pane == Pane::Details {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let text = if let Some(inspector) = &app.inspector {
        inspector_text(inspector)
    } else {
        let mut lines = vec![
            Line::styled("Workspace", Style::default().add_modifier(Modifier::BOLD)),
            Line::raw(""),
            Line::raw(format!("crates       {}", app.project.packages.len())),
            Line::raw(format!("rust files   {}", app.project.rust_files.len())),
            Line::raw(""),
            Line::styled("Entrypoints", Style::default().add_modifier(Modifier::BOLD)),
        ];
        lines.extend(
            app.project
                .binary_entrypoints()
                .into_iter()
                .map(|path| Line::raw(path.display().to_string())),
        );
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Analyzer",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(match &app.analyzer_state {
            AnalyzerState::Starting => "starting...".into(),
            AnalyzerState::Indexing => "indexing...".into(),
            AnalyzerState::Ready => "ready".into(),
            AnalyzerState::Error(message) => format!("error: {message}"),
        }));
        if let Some(notice) = &app.notice {
            lines.push(Line::raw(""));
            lines.push(Line::styled(notice, Style::default().fg(Color::Yellow)));
        }
        lines
    };
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" DETAILS ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        ),
        area,
    );
}

fn inspector_text(inspector: &Inspector) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::styled(
            qualified_symbol(&inspector.symbol),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(format!("kind       {}", inspector.symbol.kind)),
        Line::raw(format!(
            "location   {}:{}",
            inspector.symbol.file.display(),
            inspector.symbol.selection_range.start.line + 1
        )),
        Line::raw(format!(
            "container  {}",
            inspector.symbol.container.as_deref().unwrap_or("—")
        )),
        Line::raw(""),
        Line::raw(format!(
            "refs       {}",
            loading_count(inspector.references)
        )),
        Line::raw(format!("callers    {}", loading_count(inspector.callers))),
        Line::raw(format!("callees    {}", loading_count(inspector.callees))),
        Line::raw(format!(
            "problems   {}",
            loading_count(inspector.diagnostics)
        )),
        Line::raw(""),
        Line::styled("Hover", Style::default().add_modifier(Modifier::BOLD)),
    ];
    lines.extend(
        inspector
            .hover
            .as_deref()
            .unwrap_or("loading...")
            .lines()
            .map(Line::raw),
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Source",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.extend(
        inspector
            .source
            .as_deref()
            .unwrap_or("loading...")
            .lines()
            .map(Line::raw),
    );
    if let Some(error) = &inspector.error {
        lines.push(Line::styled(error, Style::default().fg(Color::Red)));
    }
    lines
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered(area, 48, 13);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(
            "Navigation\n\nj / ↓       down\nk / ↑       up\nTab         next pane\nEnter       inspect\n/           search\nq           quit\n? / Esc     close help",
        )
        .block(Block::default().title(" Help ").borders(Borders::ALL)),
        popup,
    );
}

fn render_search(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let popup = centered(area, 70, 70);
    frame.render_widget(Clear, popup);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(popup);
    let status = if app.search_loading {
        "  searching…"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(format!("> {}{status}", app.search_query)).block(
            Block::default()
                .title(" Search symbol ")
                .borders(Borders::ALL),
        ),
        rows[0],
    );
    let items: Vec<_> = app
        .search_results
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
    let title = if app.search_truncated {
        " Results (first 50) "
    } else {
        " Results "
    };
    let mut state = ListState::default()
        .with_selected((!app.search_results.is_empty()).then_some(app.search_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(Style::default().fg(Color::Cyan))
            .block(Block::default().title(title).borders(Borders::ALL)),
        rows[1],
        &mut state,
    );
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

fn project_name(project: &Project) -> &str {
    project
        .workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
}

fn qualified_symbol(symbol: &Symbol) -> String {
    symbol.container.as_ref().map_or_else(
        || symbol.name.clone(),
        |container| format!("{container}::{}", symbol.name),
    )
}

fn loading_count(value: Option<usize>) -> String {
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
    use crate::model::{Position, Range};
    use crate::project::{Package, Target};
    use ratatui::backend::TestBackend;

    fn project() -> Project {
        Project {
            workspace_root: "/home/poke/code/rust_watcher".into(),
            packages: vec![Package {
                name: "rust_watcher".into(),
                manifest_path: "/home/poke/code/rust_watcher/Cargo.toml".into(),
                targets: vec![Target {
                    name: "wt".into(),
                    crate_root: "/home/poke/code/rust_watcher/src/main.rs".into(),
                    kinds: vec!["bin".into()],
                }],
                dependencies: Vec::new(),
            }],
            rust_files: [
                "src/cli.rs",
                "src/lsp.rs",
                "src/main.rs",
                "src/model.rs",
                "src/output.rs",
                "src/project.rs",
                "src/rust.rs",
                "src/tui.rs",
                "tests/cli.rs",
            ]
            .into_iter()
            .map(|path| std::path::PathBuf::from("/home/poke/code/rust_watcher").join(path))
            .collect(),
        }
    }

    #[test]
    fn navigation_is_bounded_and_panes_switch() {
        let mut app = App::new(project());
        app.move_selection(-1);
        assert_eq!(app.project_selection, 0);
        app.move_selection(99);
        assert_eq!(app.project_selection, app.project_item_count() - 1);
        app.switch_pane();
        assert_eq!(app.pane, Pane::Details);
        app.switch_pane();
        assert_eq!(app.pane, Pane::Project);
        app.project_selection = 1;
        app.activate_project_item();
        assert_eq!(app.pane, Pane::Details);
        assert_eq!(app.notice.as_deref(), Some("Entrypoint: src/main.rs"));
    }

    #[test]
    fn help_search_and_analyzer_states_are_explicit() {
        let mut app = App::new(project());
        app.overlay = Overlay::Help;
        assert_eq!(app.overlay, Overlay::Help);
        app.open_search();
        assert_eq!(app.overlay, Overlay::Search);
        app.search_query.push_str("LspClient");
        app.edit_search();
        assert!(app.search_dirty_at.is_some());
        app.apply_event(AnalyzerEvent::Indexing);
        assert_eq!(app.analyzer_state, AnalyzerState::Indexing);
        app.apply_event(AnalyzerEvent::Ready);
        assert_eq!(app.analyzer_state, AnalyzerState::Ready);
        app.apply_event(AnalyzerEvent::Error("failed".into()));
        assert_eq!(app.analyzer_state, AnalyzerState::Error("failed".into()));
    }

    #[test]
    fn overview_renders_at_minimum_supported_size() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new(project());
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for expected in ["wt", "Project", "Crate", "RA"] {
            assert!(text.contains(expected), "missing {expected} in render");
        }
    }

    #[test]
    fn exports_review_frames_when_requested() {
        let Some(directory) = std::env::var_os("WT_TUI_REVIEW_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut starting = App::new(project());
        starting.analyzer_state = AnalyzerState::Indexing;
        std::fs::write(directory.join("01-indexing.txt"), render_text(&starting)).unwrap();

        let mut ready = App::new(project());
        ready.analyzer_state = AnalyzerState::Ready;
        std::fs::write(directory.join("02-ready.txt"), render_text(&ready)).unwrap();

        let symbol = Symbol {
            id: "src/lsp.rs:44:11:struct:LspClient".into(),
            name: "LspClient".into(),
            kind: "struct".into(),
            file: "src/lsp.rs".into(),
            range: Range {
                start: Position {
                    line: 44,
                    character: 0,
                },
                end: Position {
                    line: 54,
                    character: 1,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 44,
                    character: 11,
                },
                end: Position {
                    line: 44,
                    character: 20,
                },
            },
            container: None,
        };
        let mut inspected = App::new(project());
        inspected.analyzer_state = AnalyzerState::Ready;
        inspected.pane = Pane::Details;
        inspected.search_query = "LspClient".into();
        inspected.inspector = Some(Inspector {
            symbol,
            source: Some(
                "  43 | }\n  44 |\n  45 | pub struct LspClient {\n  46 |     child: Child,\n  47 |     input: Arc<Mutex<ChildStdin>>,".into(),
            ),
            hover: Some("pub struct LspClient".into()),
            references: Some(8),
            callers: Some(0),
            callees: Some(0),
            diagnostics: Some(0),
            error: None,
        });
        std::fs::write(directory.join("03-inspector.txt"), render_text(&inspected)).unwrap();
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
}

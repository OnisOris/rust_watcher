use super::app::{App, FocusDirection, Overlay, Pane, View};
use super::explorer::ExplorerRightMode;
use super::ui;
use super::worker::{AnalyzerCommand, AnalyzerEvent};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::sync::Once;
use std::time::Duration;
use tokio::sync::mpsc;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(200);

pub(super) async fn run_loop(
    terminal: &mut TerminalSession,
    app: &mut App,
    commands: &mpsc::Sender<AnalyzerCommand>,
    events: &mut mpsc::Receiver<AnalyzerEvent>,
    repository: &mut mpsc::Receiver<std::result::Result<crate::repository::RepositoryTree, String>>,
) -> Result<()> {
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let mut repository_open = true;
    let mut analyzer_events_open = true;
    loop {
        tokio::select! {
            event = events.recv(), if analyzer_events_open => {
                match event {
                    Some(event) => app.apply_event(event),
                    None => {
                        analyzer_events_open = false;
                        if !matches!(app.analyzer_state, super::app::AnalyzerState::Error(_)) {
                            app.apply_event(AnalyzerEvent::Error(
                                "rust-analyzer worker stopped unexpectedly".into(),
                            ));
                        }
                    }
                }
                terminal.draw(app)?;
            }
            result = repository.recv(), if repository_open => {
                repository_open = false;
                match result {
                    Some(Ok(tree)) => app.explorer.apply_tree(tree),
                    Some(Err(message)) => app.explorer.fail(message),
                    None => app.explorer.fail("repository scanner stopped".into()),
                }
                terminal.draw(app)?;
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
                if app.symbols_dirty_at.is_some_and(|at| at.elapsed() >= SEARCH_DEBOUNCE) {
                    app.symbols_dirty_at = None;
                    app.search_request += 1;
                    let id = app.search_request;
                    let query = app.symbols_query.clone();
                    if query.is_empty() {
                        app.symbols_results.clear();
                        app.symbols_selection = 0;
                        app.symbols_truncated = false;
                        app.symbols_loading = false;
                    } else {
                        send_command(app, commands, AnalyzerCommand::Search { id, query }).await;
                    }
                    terminal.draw(app)?;
                }
            }
        }
    }
}

pub(super) async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    commands: &mpsc::Sender<AnalyzerCommand>,
) -> Result<bool> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }
    if key.code == KeyCode::Esc {
        if let Some((id, target, scope)) = app.escape() {
            send_command(
                app,
                commands,
                AnalyzerCommand::LoadCalls { id, target, scope },
            )
            .await;
        }
        return Ok(false);
    }
    match app.overlay {
        Overlay::Help => {
            if key.code == KeyCode::Char('?') {
                app.overlay = Overlay::None;
            }
        }
        Overlay::Search => match key.code {
            KeyCode::Down => app.move_selection(1),
            KeyCode::Up => app.move_selection(-1),
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_selection(1)
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_selection(-1)
            }
            KeyCode::Backspace => {
                app.symbols_query.pop();
                app.edit_search();
            }
            KeyCode::Enter => begin_inspect(app, commands).await?,
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                app.symbols_query.push(character);
                app.edit_search();
            }
            _ => {}
        },
        Overlay::None => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('1') => app.switch_view(View::Overview),
            KeyCode::Char('2') => app.switch_view(View::Symbols),
            KeyCode::Char('3') => open_calls(app, commands).await?,
            KeyCode::Char('4') => app.switch_view(View::Explorer),
            KeyCode::Char('c')
                if app.view == View::Symbols
                    || (app.view == View::Explorer
                        && app.explorer.right_mode == ExplorerRightMode::Inspector) =>
            {
                open_calls(app, commands).await?
            }
            KeyCode::Char('e') if app.view == View::Calls => {
                reload_calls_scope(app, commands).await?
            }
            KeyCode::Char('?') => app.overlay = Overlay::Help,
            KeyCode::Char('/') => app.open_search(),
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.focus_direction(FocusDirection::Left)
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.focus_direction(FocusDirection::Down)
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.focus_direction(FocusDirection::Up)
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.focus_direction(FocusDirection::Right)
            }
            KeyCode::Left | KeyCode::Char('h')
                if app.view == View::Explorer && app.pane == Pane::ExplorerTree =>
            {
                app.explorer.collapse_or_parent()
            }
            KeyCode::Right | KeyCode::Char('l')
                if app.view == View::Explorer && app.pane == Pane::ExplorerTree =>
            {
                app.explorer.expand_or_child()
            }
            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_selection(10)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_selection(-10)
            }
            KeyCode::Tab => app.next_pane(),
            KeyCode::BackTab => app.previous_pane(),
            KeyCode::Enter if app.view == View::Symbols => begin_inspect(app, commands).await?,
            KeyCode::Enter if app.view == View::Calls => follow_call(app, commands).await?,
            KeyCode::Enter if app.view == View::Explorer => {
                activate_explorer(app, commands).await?
            }
            KeyCode::Enter => app.activate_project_item(),
            _ => {}
        },
    }
    Ok(false)
}

async fn begin_inspect(app: &mut App, commands: &mpsc::Sender<AnalyzerCommand>) -> Result<()> {
    if let Some((id, symbol)) = app.begin_selected_inspect() {
        send_command(app, commands, AnalyzerCommand::Inspect { id, symbol }).await;
    }
    Ok(())
}

async fn open_calls(app: &mut App, commands: &mpsc::Sender<AnalyzerCommand>) -> Result<()> {
    if let Some((id, target, scope)) = app.open_calls_for_inspector() {
        send_command(
            app,
            commands,
            AnalyzerCommand::LoadCalls { id, target, scope },
        )
        .await;
    }
    Ok(())
}

async fn follow_call(app: &mut App, commands: &mpsc::Sender<AnalyzerCommand>) -> Result<()> {
    if let Some((id, target, scope)) = app.follow_selected_call() {
        send_command(
            app,
            commands,
            AnalyzerCommand::LoadCalls { id, target, scope },
        )
        .await;
    }
    Ok(())
}

async fn reload_calls_scope(app: &mut App, commands: &mpsc::Sender<AnalyzerCommand>) -> Result<()> {
    if let Some((id, target, scope)) = app.toggle_call_scope() {
        send_command(
            app,
            commands,
            AnalyzerCommand::LoadCalls { id, target, scope },
        )
        .await;
    }
    Ok(())
}

async fn activate_explorer(app: &mut App, commands: &mpsc::Sender<AnalyzerCommand>) -> Result<()> {
    match app.pane {
        Pane::ExplorerTree => {
            let opens_file = app
                .explorer
                .selected_entry()
                .is_some_and(|entry| entry.kind != crate::repository::RepoEntryKind::Directory);
            if let Some((id, file)) = app.explorer.open_selected() {
                app.pane = Pane::ExplorerFile;
                send_command(app, commands, AnalyzerCommand::InspectFile { id, file }).await;
            } else if opens_file {
                app.pane = Pane::ExplorerFile;
            }
        }
        Pane::ExplorerFile if app.explorer.right_mode == ExplorerRightMode::File => {
            if let Some((id, symbol)) = app.begin_explorer_symbol_inspect() {
                send_command(app, commands, AnalyzerCommand::Inspect { id, symbol }).await;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn send_command(
    app: &mut App,
    commands: &mpsc::Sender<AnalyzerCommand>,
    command: AnalyzerCommand,
) {
    let failed = command.clone();
    if commands.send(command).await.is_ok() {
        return;
    }

    let message = "rust-analyzer worker is unavailable".to_owned();
    app.apply_event(AnalyzerEvent::Error(message.clone()));
    match failed {
        AnalyzerCommand::Search { id, .. } => {
            app.apply_event(AnalyzerEvent::SearchFailed { id, message });
        }
        AnalyzerCommand::Inspect { id, .. } => {
            app.apply_event(AnalyzerEvent::InspectFailed {
                id,
                section: "analyzer",
                message,
            });
        }
        AnalyzerCommand::InspectFile { id, file } => {
            app.apply_event(AnalyzerEvent::FileSymbolsFailed { id, file, message });
        }
        AnalyzerCommand::LoadCalls { id, .. } => {
            app.apply_event(AnalyzerEvent::CallsFailed {
                id,
                incoming: true,
                message: message.clone(),
            });
            app.apply_event(AnalyzerEvent::CallsFailed {
                id,
                incoming: false,
                message,
            });
        }
        AnalyzerCommand::Shutdown => {}
    }
}

pub(super) struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    pub(super) fn enter() -> Result<Self> {
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

    pub(super) fn draw(&mut self, app: &App) -> Result<()> {
        self.terminal.draw(|frame| ui::render(frame, app))?;
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

pub(super) fn install_panic_hook() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Position, Range, Symbol};
    use crate::project::Project;
    use crate::repository::{RepoEntry, RepoEntryKind, RepositoryTree};

    fn app() -> App {
        App::new(Project {
            workspace_root: "/workspace/project".into(),
            packages: Vec::new(),
            rust_files: Vec::new(),
        })
    }

    fn symbol(name: &str) -> Symbol {
        Symbol {
            id: name.into(),
            name: name.into(),
            kind: "struct".into(),
            file: "src/lib.rs".into(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 0,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            container: None,
        }
    }

    #[tokio::test]
    async fn search_ctrl_jk_moves_results_without_switching_panes() {
        let mut app = app();
        app.switch_view(View::Symbols);
        app.overlay = Overlay::Search;
        app.symbols_results = vec![symbol("First"), symbol("Second")];
        let (commands, _receiver) = mpsc::channel(1);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.symbols_selection, 1);
        assert_eq!(app.pane, super::super::app::Pane::Symbols);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.symbols_selection, 0);
    }

    #[tokio::test]
    async fn number_keys_switch_views_and_calls_use_the_inspected_symbol() {
        let mut app = app();
        let (commands, mut receiver) = mpsc::channel(1);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.view, View::Symbols);
        assert!(receiver.try_recv().is_err());
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.view, View::Overview);
        assert!(receiver.try_recv().is_err());
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.view, View::Calls);
        assert!(app.calls.center.is_none());
        assert!(receiver.try_recv().is_err());

        app.inspector = Some(super::super::app::Inspector::loading(1, symbol("run")));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.view, View::Calls);
        assert_eq!(app.calls.center.as_ref().unwrap().name, "run");
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AnalyzerCommand::LoadCalls { .. }
        ));

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.view, View::Explorer);
        assert_eq!(app.pane, Pane::ExplorerTree);
    }

    #[tokio::test]
    async fn explorer_opens_files_then_inspects_exact_file_symbols() {
        let mut app = app();
        app.explorer.apply_tree(RepositoryTree {
            entries: vec![
                RepoEntry {
                    path: "src".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/main.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(10),
                },
            ],
            truncated: false,
            skipped_errors: 0,
        });
        app.explorer.set_expanded("src".into(), true);
        app.switch_view(View::Explorer);
        app.explorer.selected = 2;
        let (commands, mut receiver) = mpsc::channel(2);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        let (id, file) = match receiver.try_recv().unwrap() {
            AnalyzerCommand::InspectFile { id, file } => (id, file),
            command => panic!("unexpected command: {command:?}"),
        };
        let mut exact = symbol("main");
        exact.file = file.clone();
        exact.selection_range.start.line = 12;
        app.apply_event(AnalyzerEvent::FileSymbols {
            id,
            file,
            symbols: vec![exact.clone()],
        });
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        match receiver.try_recv().unwrap() {
            AnalyzerCommand::Inspect { symbol, .. } => {
                assert_eq!(symbol.selection_range, exact.selection_range)
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[tokio::test]
    async fn explorer_tree_navigation_does_not_request_semantics() {
        let mut app = app();
        app.explorer.apply_tree(RepositoryTree {
            entries: vec![
                RepoEntry {
                    path: "src".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/main.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(10),
                },
            ],
            truncated: false,
            skipped_errors: 0,
        });
        app.switch_view(View::Explorer);
        let (commands, mut receiver) = mpsc::channel(2);
        for code in [KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Char('j')] {
            handle_key(&mut app, KeyEvent::new(code, KeyModifiers::NONE), &commands)
                .await
                .unwrap();
        }
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn calls_external_toggle_reloads_current_target_with_new_scope() {
        let mut app = app();
        app.switch_view(View::Calls);
        app.calls.center = Some(crate::model::CallTarget::from_symbol(&symbol("run")));
        let (commands, mut receiver) = mpsc::channel(1);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            &commands,
        )
        .await
        .unwrap();
        assert_eq!(app.calls.scope, crate::model::CallScope::All);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            AnalyzerCommand::LoadCalls {
                scope: crate::model::CallScope::All,
                ..
            }
        ));
    }
    #[tokio::test]
    async fn closed_analyzer_channel_degrades_without_returning_an_error() {
        let mut app = app();
        app.search_request = 41;
        app.symbols_loading = true;
        let (commands, receiver) = mpsc::channel(1);
        drop(receiver);

        send_command(
            &mut app,
            &commands,
            AnalyzerCommand::Search {
                id: 41,
                query: "camera".into(),
            },
        )
        .await;

        assert!(matches!(
            app.analyzer_state,
            super::super::app::AnalyzerState::Error(_)
        ));
        assert!(!app.symbols_loading);
        assert!(app
            .notice
            .as_deref()
            .is_some_and(|message| message.contains("unavailable")));
    }}

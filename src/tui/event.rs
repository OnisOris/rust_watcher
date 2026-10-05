use super::app::{App, FocusDirection, Overlay, View};
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
                        commands.send(AnalyzerCommand::Search { id, query }).await?;
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
        app.escape();
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
            KeyCode::Enter => app.activate_project_item(),
            _ => {}
        },
    }
    Ok(false)
}

async fn begin_inspect(app: &mut App, commands: &mpsc::Sender<AnalyzerCommand>) -> Result<()> {
    if let Some((id, symbol)) = app.begin_selected_inspect() {
        commands
            .send(AnalyzerCommand::Inspect { id, symbol })
            .await?;
    }
    Ok(())
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
    async fn number_keys_switch_views_without_analyzer_commands() {
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
    }
}

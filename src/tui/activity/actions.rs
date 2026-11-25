use super::*;
pub(in crate::tui) async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>,
) -> bool {
    let s = &mut app.activity;
    match key.code {
        K::Char('v') => {
            s.mode = s.mode.next();
            s.summary = false;
        }
        K::Char('t') => {
            s.mode = if s.mode == Mode::Timeline {
                Mode::Tree
            } else {
                Mode::Timeline
            };
            s.summary = false;
        }
        K::Char('s') => {
            s.summary = !s.summary;
            s.summary_scroll = 0;
        }
        K::Char('x') => {
            s.query.heat_mode = s.query.heat_mode.next();
            s.refresh();
        }
        K::Char('h') => s.heat = !s.heat,
        K::Char('z') => {
            s.follow = s.manual_until.is_some() || !s.follow;
            s.manual_until = None;
            s.auto = Auto::Follow;
            s.refresh();
        }
        K::Char('r') => {
            s.follow = true;
            s.manual_until = None;
            s.candidate = None;
            s.focus = None;
            s.auto = Auto::Follow;
            s.refresh();
        }
        K::Char('e') => {
            s.auto = s.auto.next();
            s.revision = u64::MAX;
            s.refresh();
        }
        K::Char('c') => s.density = (s.density + 1) % 3,
        K::Char('a') => {
            s.query.sources = s.query.sources.next();
            s.refresh();
        }
        K::Char('f') => {
            s.query.types = s.query.types.next();
            s.refresh();
        }
        K::Char('w') => {
            s.query.window = s.query.window.next();
            s.refresh();
        }
        K::Char('p') => {
            s.query.scope = s.query.scope.next();
            s.refresh();
        }
        K::Char('b') => {
            s.query.files = s.query.files.next();
            s.refresh();
        }
        K::Char('d') => {
            s.directories_only = !s.directories_only;
            s.refresh();
        }
        K::Char('n') => {
            s.sort = s.sort.next();
            s.refresh();
        }
        K::Char(' ') => {
            if let Some(p) = s.selected_path() {
                s.focus = Some(p);
                s.follow = false;
            }
        }
        K::Char('j') | K::Down => s.down(1),
        K::Char('k') | K::Up => s.down(-1),
        K::PageDown => s.down(10),
        K::PageUp => s.down(-10),
        K::Left => s.parent(),
        K::Right | K::Char('l') => s.expand(),
        K::Esc if s.summary => s.summary = false,
        K::Esc if s.mode != Mode::Tree => s.mode = Mode::Tree,
        K::Esc => s.parent(),
        K::Enter if s.mode == Mode::Tree && s.rows.get(s.selected).is_some_and(|r| r.dir) => {
            s.expand()
        }
        K::Enter | K::Char('o') => source::open_source(app, tx).await,
        K::Char('m') => {
            let origin = crate::tui::journey::point(app, View::Map);
            app.switch_view(View::Map);
            crate::tui::journey::remember(app, origin);
        }
        _ => return false,
    }
    true
}

use super::worker::AnalyzerEvent;
use crate::model::{CallTarget, CallTargets, Location, Symbol};
use crate::project::Project;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AnalyzerState {
    Starting,
    Indexing,
    Ready,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Overview,
    Symbols,
    Calls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pane {
    Project,
    Details,
    Symbols,
    Inspector,
    Callers,
    CallCurrent,
    Callees,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusDirection {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Overlay {
    None,
    Help,
    Search,
}

#[derive(Debug, Clone)]
pub(super) struct Inspector {
    pub request_id: u64,
    pub symbol: Symbol,
    pub source: Option<String>,
    pub hover: Option<Option<String>>,
    pub definition: Option<Option<Location>>,
    pub references: Option<usize>,
    pub callers: Option<usize>,
    pub callees: Option<usize>,
    pub diagnostics: Option<usize>,
    pub errors: Vec<(String, String)>,
    pub scroll: usize,
}

impl Inspector {
    pub(super) fn loading(request_id: u64, symbol: Symbol) -> Self {
        Self {
            request_id,
            symbol,
            source: None,
            hover: None,
            definition: None,
            references: None,
            callers: None,
            callees: None,
            diagnostics: None,
            errors: Vec::new(),
            scroll: 0,
        }
    }

    fn line_count(&self) -> usize {
        17 + self
            .hover
            .as_ref()
            .and_then(|value| value.as_deref())
            .map_or(1, |value| value.lines().count())
            + self
                .source
                .as_deref()
                .map_or(1, |value| value.lines().count())
            + self.errors.len()
    }

    fn scroll_by(&mut self, delta: isize) {
        self.scroll = move_index(self.scroll, self.line_count(), delta);
    }
}

const CALL_HISTORY_LIMIT: usize = 50;

#[derive(Debug, Clone, Default)]
pub(super) struct CallsState {
    pub center: Option<CallTarget>,
    pub callers: Vec<CallTarget>,
    pub callees: Vec<CallTarget>,
    pub callers_selection: usize,
    pub callees_selection: usize,
    pub callers_loading: bool,
    pub callees_loading: bool,
    pub callers_truncated: bool,
    pub callees_truncated: bool,
    pub callers_error: Option<String>,
    pub callees_error: Option<String>,
    pub history: Vec<CallTarget>,
    pub request_id: u64,
}

pub(super) struct App {
    pub project: Project,
    pub analyzer_state: AnalyzerState,
    pub view: View,
    pub pane: Pane,
    pub overlay: Overlay,
    pub project_selection: usize,
    pub symbols_query: String,
    pub symbols_results: Vec<Symbol>,
    pub symbols_selection: usize,
    pub symbols_dirty_at: Option<Instant>,
    pub search_request: u64,
    pub symbols_truncated: bool,
    pub symbols_loading: bool,
    pub inspector: Option<Inspector>,
    pub inspect_request: u64,
    pub calls: CallsState,
    pub notice: Option<String>,
}

impl App {
    pub(super) fn new(project: Project) -> Self {
        Self {
            project,
            analyzer_state: AnalyzerState::Starting,
            view: View::Overview,
            pane: Pane::Project,
            overlay: Overlay::None,
            project_selection: 0,
            symbols_query: String::new(),
            symbols_results: Vec::new(),
            symbols_selection: 0,
            symbols_dirty_at: None,
            search_request: 0,
            symbols_truncated: false,
            symbols_loading: false,
            inspector: None,
            inspect_request: 0,
            calls: CallsState::default(),
            notice: None,
        }
    }

    fn project_item_count(&self) -> usize {
        1 + self.project.binary_entrypoints().len() + self.project.packages.len()
    }

    pub(super) fn switch_view(&mut self, view: View) {
        self.view = view;
        self.overlay = Overlay::None;
        self.pane = match view {
            View::Overview => Pane::Project,
            View::Symbols => Pane::Symbols,
            View::Calls => Pane::CallCurrent,
        };
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.overlay == Overlay::Search {
            self.symbols_selection =
                move_index(self.symbols_selection, self.symbols_results.len(), delta);
            return;
        }
        match self.pane {
            Pane::Project => {
                self.project_selection =
                    move_index(self.project_selection, self.project_item_count(), delta);
            }
            Pane::Symbols => {
                self.symbols_selection =
                    move_index(self.symbols_selection, self.symbols_results.len(), delta);
            }
            Pane::Inspector => {
                if let Some(inspector) = &mut self.inspector {
                    inspector.scroll_by(delta);
                }
            }
            Pane::Callers => {
                self.calls.callers_selection = move_index(
                    self.calls.callers_selection,
                    self.calls.callers.len(),
                    delta,
                );
            }
            Pane::Callees => {
                self.calls.callees_selection = move_index(
                    self.calls.callees_selection,
                    self.calls.callees.len(),
                    delta,
                );
            }
            Pane::Details | Pane::CallCurrent => {}
        }
    }

    pub(super) fn next_pane(&mut self) {
        self.pane = match (self.view, self.pane) {
            (View::Overview, Pane::Project) => Pane::Details,
            (View::Overview, Pane::Details) => Pane::Project,
            (View::Symbols, Pane::Symbols) => Pane::Inspector,
            (View::Symbols, Pane::Inspector) => Pane::Symbols,
            (View::Calls, Pane::Callers) => Pane::CallCurrent,
            (View::Calls, Pane::CallCurrent) => Pane::Callees,
            (View::Calls, Pane::Callees) => Pane::Callers,
            (_, pane) => pane,
        };
    }

    pub(super) fn previous_pane(&mut self) {
        self.pane = match (self.view, self.pane) {
            (View::Overview, Pane::Project) => Pane::Details,
            (View::Overview, Pane::Details) => Pane::Project,
            (View::Symbols, Pane::Symbols) => Pane::Inspector,
            (View::Symbols, Pane::Inspector) => Pane::Symbols,
            (View::Calls, Pane::Callers) => Pane::Callees,
            (View::Calls, Pane::CallCurrent) => Pane::Callers,
            (View::Calls, Pane::Callees) => Pane::CallCurrent,
            (_, pane) => pane,
        };
    }

    pub(super) fn focus_direction(&mut self, direction: FocusDirection) {
        self.pane = match (self.view, self.pane, direction) {
            (View::Overview, Pane::Project, FocusDirection::Right) => Pane::Details,
            (View::Overview, Pane::Details, FocusDirection::Left) => Pane::Project,
            (View::Symbols, Pane::Symbols, FocusDirection::Right) => Pane::Inspector,
            (View::Symbols, Pane::Inspector, FocusDirection::Left) => Pane::Symbols,
            (View::Calls, Pane::Callers, FocusDirection::Right) => Pane::CallCurrent,
            (View::Calls, Pane::CallCurrent, FocusDirection::Left) => Pane::Callers,
            (View::Calls, Pane::CallCurrent, FocusDirection::Right) => Pane::Callees,
            (View::Calls, Pane::Callees, FocusDirection::Left) => Pane::CallCurrent,
            _ => self.pane,
        };
    }

    pub(super) fn escape(&mut self) -> Option<(u64, CallTarget)> {
        match self.overlay {
            Overlay::Help | Overlay::Search => self.overlay = Overlay::None,
            Overlay::None if self.pane == Pane::Details => self.pane = Pane::Project,
            Overlay::None if self.pane == Pane::Inspector => self.pane = Pane::Symbols,
            Overlay::None if matches!(self.pane, Pane::Callers | Pane::Callees) => {
                self.pane = Pane::CallCurrent;
            }
            Overlay::None if self.pane == Pane::CallCurrent => {
                if let Some(target) = self.calls.history.pop() {
                    self.notice = None;
                    return Some(self.load_calls(target, false));
                }
                self.switch_view(View::Symbols);
            }
            Overlay::None => {}
        }
        self.notice = None;
        None
    }

    pub(super) fn activate_project_item(&mut self) {
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

    pub(super) fn open_search(&mut self) {
        self.overlay = Overlay::Search;
        self.symbols_dirty_at = None;
        self.symbols_loading = false;
    }

    pub(super) fn edit_search(&mut self) {
        self.symbols_dirty_at = Some(Instant::now());
        self.symbols_loading = !self.symbols_query.is_empty();
    }

    pub(super) fn begin_selected_inspect(&mut self) -> Option<(u64, Symbol)> {
        let symbol = self.symbols_results.get(self.symbols_selection)?.clone();
        self.inspect_request += 1;
        let id = self.inspect_request;
        self.inspector = Some(Inspector::loading(id, symbol.clone()));
        self.overlay = Overlay::None;
        self.pane = match self.view {
            View::Overview => Pane::Details,
            View::Symbols => Pane::Inspector,
            View::Calls => Pane::CallCurrent,
        };
        Some((id, symbol))
    }

    pub(super) fn open_calls_for_inspector(&mut self) -> Option<(u64, CallTarget)> {
        self.switch_view(View::Calls);
        let target = CallTarget::from_symbol(&self.inspector.as_ref()?.symbol);
        self.calls.history.clear();
        Some(self.load_calls(target, false))
    }

    pub(super) fn follow_selected_call(&mut self) -> Option<(u64, CallTarget)> {
        let target = match self.pane {
            Pane::Callers => self
                .calls
                .callers
                .get(self.calls.callers_selection)?
                .clone(),
            Pane::Callees => self
                .calls
                .callees
                .get(self.calls.callees_selection)?
                .clone(),
            _ => return None,
        };
        Some(self.load_calls(target, true))
    }

    fn load_calls(&mut self, target: CallTarget, remember: bool) -> (u64, CallTarget) {
        if remember {
            if let Some(center) = self.calls.center.take() {
                self.calls.history.push(center);
                if self.calls.history.len() > CALL_HISTORY_LIMIT {
                    self.calls.history.remove(0);
                }
            }
        }
        self.calls.request_id += 1;
        self.calls.center = Some(target.clone());
        self.calls.callers.clear();
        self.calls.callees.clear();
        self.calls.callers_selection = 0;
        self.calls.callees_selection = 0;
        self.calls.callers_loading = true;
        self.calls.callees_loading = true;
        self.calls.callers_truncated = false;
        self.calls.callees_truncated = false;
        self.calls.callers_error = None;
        self.calls.callees_error = None;
        self.pane = Pane::CallCurrent;
        (self.calls.request_id, target)
    }

    pub(super) fn apply_event(&mut self, event: AnalyzerEvent) {
        match event {
            AnalyzerEvent::Indexing => self.analyzer_state = AnalyzerState::Indexing,
            AnalyzerEvent::Ready => self.analyzer_state = AnalyzerState::Ready,
            AnalyzerEvent::Error(message) => self.analyzer_state = AnalyzerState::Error(message),
            AnalyzerEvent::SearchResults { id, result } if id == self.search_request => {
                self.symbols_results = result.items;
                self.symbols_truncated = result.truncated;
                self.symbols_selection = 0;
                self.symbols_loading = false;
            }
            AnalyzerEvent::SearchFailed { id, message } if id == self.search_request => {
                self.symbols_results.clear();
                self.symbols_loading = false;
                self.notice = Some(message);
            }
            AnalyzerEvent::Source { id, value } => self.with_inspector(id, |item| {
                item.source = Some(value);
            }),
            AnalyzerEvent::Hover { id, value } => self.with_inspector(id, |item| {
                item.hover = Some(value);
            }),
            AnalyzerEvent::Definition { id, value } => self.with_inspector(id, |item| {
                item.definition = Some(value);
            }),
            AnalyzerEvent::References { id, count } => self.with_inspector(id, |item| {
                item.references = Some(count);
            }),
            AnalyzerEvent::InspectorCallers { id, count } => {
                self.with_inspector(id, |item| item.callers = Some(count));
            }
            AnalyzerEvent::InspectorCallees { id, count } => {
                self.with_inspector(id, |item| item.callees = Some(count));
            }
            AnalyzerEvent::Diagnostics { id, values } => self.with_inspector(id, |item| {
                item.diagnostics = Some(values.len());
            }),
            AnalyzerEvent::CallersLoaded { id, result } if id == self.calls.request_id => {
                self.apply_callers(result);
            }
            AnalyzerEvent::CalleesLoaded { id, result } if id == self.calls.request_id => {
                self.apply_callees(result);
            }
            AnalyzerEvent::CallsFailed {
                id,
                incoming,
                message,
            } if id == self.calls.request_id => {
                if incoming {
                    self.calls.callers_loading = false;
                    self.calls.callers_error = Some(message);
                } else {
                    self.calls.callees_loading = false;
                    self.calls.callees_error = Some(message);
                }
            }
            AnalyzerEvent::InspectFailed {
                id,
                section,
                message,
            } => self.with_inspector(id, |item| {
                item.errors.push((section.into(), message));
            }),
            AnalyzerEvent::SearchResults { .. }
            | AnalyzerEvent::SearchFailed { .. }
            | AnalyzerEvent::CallersLoaded { .. }
            | AnalyzerEvent::CalleesLoaded { .. }
            | AnalyzerEvent::CallsFailed { .. } => {}
        }
    }

    fn apply_callers(&mut self, result: Option<CallTargets>) {
        self.calls.callers_loading = false;
        match result {
            Some(result) => {
                self.calls.callers = result.items;
                self.calls.callers_truncated = result.truncated;
            }
            None => {
                self.calls.callers_error = Some("Call hierarchy unavailable for this symbol".into())
            }
        }
    }

    fn apply_callees(&mut self, result: Option<CallTargets>) {
        self.calls.callees_loading = false;
        match result {
            Some(result) => {
                self.calls.callees = result.items;
                self.calls.callees_truncated = result.truncated;
            }
            None => {
                self.calls.callees_error = Some("Call hierarchy unavailable for this symbol".into())
            }
        }
    }

    fn with_inspector(&mut self, id: u64, update: impl FnOnce(&mut Inspector)) {
        if let Some(inspector) = &mut self.inspector {
            if inspector.request_id == id {
                update(inspector);
            }
        }
    }
}

pub(super) fn move_index(current: usize, count: usize, delta: isize) -> usize {
    if count == 0 {
        return 0;
    }
    current.saturating_add_signed(delta).min(count - 1)
}

pub(super) fn project_name(project: &Project) -> &str {
    project
        .workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
}

pub(super) fn qualified_symbol(symbol: &Symbol) -> String {
    symbol.container.as_ref().map_or_else(
        || symbol.name.clone(),
        |container| format!("{container}::{}", symbol.name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Position, Range, SymbolSearchResult};
    use crate::project::{Package, Target};

    pub(super) fn project() -> Project {
        Project {
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
        }
    }

    pub(super) fn symbol(name: &str) -> Symbol {
        Symbol {
            id: format!("src/lsp.rs:44:11:struct:{name}"),
            name: name.into(),
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
        }
    }

    fn call_target(name: &str, line: u32) -> CallTarget {
        let mut symbol = symbol(name);
        symbol.kind = "function".into();
        symbol.selection_range.start.line = line;
        symbol.selection_range.end.line = line;
        CallTarget::from_symbol(&symbol)
    }

    #[test]
    fn views_and_spatial_navigation_are_explicit() {
        let mut app = App::new(project());
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::Details);
        let _ = app.escape();
        assert_eq!(app.pane, Pane::Project);

        app.switch_view(View::Symbols);
        assert_eq!(app.pane, Pane::Symbols);
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::Inspector);
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::Inspector);
        app.focus_direction(FocusDirection::Left);
        assert_eq!(app.pane, Pane::Symbols);
        app.focus_direction(FocusDirection::Up);
        assert_eq!(app.pane, Pane::Symbols);
        app.next_pane();
        assert_eq!(app.pane, Pane::Inspector);
        app.previous_pane();
        assert_eq!(app.pane, Pane::Symbols);

        app.switch_view(View::Calls);
        assert_eq!(app.pane, Pane::CallCurrent);
        app.focus_direction(FocusDirection::Left);
        assert_eq!(app.pane, Pane::Callers);
        app.focus_direction(FocusDirection::Right);
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::Callees);
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::Callees);
        assert!(app.escape().is_none());
        assert_eq!(app.pane, Pane::CallCurrent);
        app.next_pane();
        assert_eq!(app.pane, Pane::Callees);
        app.previous_pane();
        assert_eq!(app.pane, Pane::CallCurrent);
        app.pane = Pane::Callers;
        app.next_pane();
        assert_eq!(app.pane, Pane::CallCurrent);
        app.next_pane();
        assert_eq!(app.pane, Pane::Callees);
        app.next_pane();
        assert_eq!(app.pane, Pane::Callers);
        app.previous_pane();
        assert_eq!(app.pane, Pane::Callees);
    }

    #[test]
    fn symbol_selection_and_inspector_scroll_are_bounded() {
        let mut app = App::new(project());
        app.switch_view(View::Symbols);
        app.symbols_results = vec![symbol("First"), symbol("Second")];
        app.move_selection(-10);
        assert_eq!(app.symbols_selection, 0);
        app.move_selection(10);
        assert_eq!(app.symbols_selection, 1);

        let (id, _) = app.begin_selected_inspect().unwrap();
        app.apply_event(AnalyzerEvent::Source {
            id,
            value: (0..30)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n"),
        });
        app.move_selection(-10);
        assert_eq!(app.inspector.as_ref().unwrap().scroll, 0);
        app.move_selection(10_000);
        let inspector = app.inspector.as_ref().unwrap();
        assert_eq!(inspector.scroll, inspector.line_count() - 1);
    }

    #[test]
    fn symbols_state_survives_view_switches() {
        let mut app = App::new(project());
        app.switch_view(View::Symbols);
        app.symbols_query = "Lsp".into();
        app.symbols_results = vec![symbol("LspClient")];
        app.symbols_selection = 0;
        app.switch_view(View::Overview);
        app.switch_view(View::Symbols);
        assert_eq!(app.symbols_query, "Lsp");
        assert_eq!(app.symbols_results[0].name, "LspClient");
    }

    #[test]
    fn stale_search_and_inspector_events_are_ignored() {
        let mut app = App::new(project());
        app.search_request = 2;
        app.apply_event(AnalyzerEvent::SearchResults {
            id: 1,
            result: SymbolSearchResult {
                items: vec![symbol("Old")],
                truncated: false,
            },
        });
        assert!(app.symbols_results.is_empty());

        app.symbols_results = vec![symbol("Foo")];
        let (first, _) = app.begin_selected_inspect().unwrap();
        app.symbols_results = vec![symbol("Bar")];
        let (second, _) = app.begin_selected_inspect().unwrap();
        app.apply_event(AnalyzerEvent::Source {
            id: first,
            value: "foo".into(),
        });
        assert!(app.inspector.as_ref().unwrap().source.is_none());
        app.apply_event(AnalyzerEvent::Source {
            id: second,
            value: "bar".into(),
        });
        assert_eq!(
            app.inspector.as_ref().unwrap().source.as_deref(),
            Some("bar")
        );
    }

    #[test]
    fn calls_selection_history_and_stale_events_are_bounded() {
        let mut app = App::new(project());
        app.inspector = Some(Inspector::loading(1, symbol("A")));
        let (first, _) = app.open_calls_for_inspector().unwrap();
        let caller_b = call_target("B", 10);
        let callee_c = call_target("C", 20);
        app.apply_event(AnalyzerEvent::CallersLoaded {
            id: first,
            result: Some(CallTargets {
                items: vec![caller_b.clone()],
                truncated: false,
            }),
        });
        app.pane = Pane::Callers;
        app.move_selection(99);
        assert_eq!(app.calls.callers_selection, 0);
        let (second, target) = app.follow_selected_call().unwrap();
        assert_eq!(target.name, "B");
        assert_eq!(app.calls.history.len(), 1);

        app.apply_event(AnalyzerEvent::CalleesLoaded {
            id: first,
            result: Some(CallTargets {
                items: vec![call_target("stale", 30)],
                truncated: false,
            }),
        });
        assert!(app.calls.callees.is_empty());
        app.apply_event(AnalyzerEvent::CalleesLoaded {
            id: second,
            result: Some(CallTargets {
                items: vec![callee_c, call_target("D", 21)],
                truncated: false,
            }),
        });
        app.pane = Pane::Callees;
        app.move_selection(99);
        assert_eq!(app.calls.callees_selection, 1);
        app.move_selection(-99);
        assert_eq!(app.calls.callees_selection, 0);
        let (_third, target) = app.follow_selected_call().unwrap();
        assert_eq!(target.name, "C");
        assert_eq!(app.calls.history.len(), 2);

        let (_, target) = app.escape().unwrap();
        assert_eq!(target.name, "B");
        let (_, target) = app.escape().unwrap();
        assert_eq!(target.name, "A");
    }

    #[test]
    fn call_history_drops_oldest_entry_at_limit() {
        let mut app = App::new(project());
        app.calls.center = Some(call_target("root", 0));
        for line in 1..=CALL_HISTORY_LIMIT as u32 + 1 {
            app.load_calls(call_target(&format!("target_{line}"), line), true);
        }
        assert_eq!(app.calls.history.len(), CALL_HISTORY_LIMIT);
        assert_eq!(app.calls.history[0].name, "target_1");
    }
}

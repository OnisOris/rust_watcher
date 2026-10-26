use super::explorer::{ExplorerRightMode, ExplorerState};
use super::worker::AnalyzerEvent;
use crate::model::{CallScope, CallTarget, CallTargets, Location, Symbol};
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
    Explorer,
    Map,
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
    ExplorerTree,
    ExplorerFile,
    MapGraph,
    MapInspector,
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
    pub scroll_limit: std::cell::Cell<Option<usize>>,
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
            scroll_limit: Default::default(),
        }
    }

    fn line_count(&self) -> usize {
        30 + self
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
        let max = self
            .scroll_limit
            .get()
            .unwrap_or_else(|| self.line_count().saturating_sub(1));
        self.scroll = self.scroll.min(max).saturating_add_signed(delta).min(max);
    }
}

const CALL_HISTORY_LIMIT: usize = 50;

#[derive(Debug, Clone, Default)]
pub(super) struct CallsState {
    pub graph: super::call_graph::State,
    pub context_scroll: usize,
    pub context_scroll_limit: std::cell::Cell<Option<usize>>,
    pub unavailable: Option<String>,
    pub scope: CallScope,
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
    pub callers_unsupported: bool,
    pub callees_unsupported: bool,
    pub history: Vec<CallVisit>,
    pub request_id: u64,
}
#[derive(Debug, Clone)]
pub(super) struct CallVisit {
    pub target: CallTarget,
    pub state: Box<CallsState>,
}
impl std::ops::Deref for CallVisit {
    type Target = CallTarget;
    fn deref(&self) -> &CallTarget {
        &self.target
    }
}
impl From<CallTarget> for CallVisit {
    fn from(target: CallTarget) -> Self {
        Self {
            target,
            state: Box::default(),
        }
    }
}

pub(super) struct App {
    pub graph_request: u64,
    pub graph_expansions: usize,
    pub journey: super::journey::State,
    pub module_map: super::module_map::State,
    pub map: super::map::State,
    pub color_mode: super::theme::ColorMode,
    pub hotspots: Option<crate::git_analysis::HotspotReport>,
    pub hotspots_error: Option<String>,
    pub maintainability: Option<crate::maintainability::MaintainabilityReport>,
    pub maintainability_error: Option<String>,
    pub metrics: Option<crate::metrics::RepositoryMetrics>,
    pub metrics_error: Option<String>,
    pub metrics_scroll: usize,
    pub metrics_scroll_limit: std::cell::Cell<Option<usize>>,
    pub project: Project,
    pub analyzer_state: AnalyzerState,
    pub view: View,
    pub pane: Pane,
    pub overlay: Overlay,
    pub overview: super::overview::State,
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
    pub explorer: ExplorerState,
    pub notice: Option<String>,
}

impl App {
    pub(super) fn new(project: Project) -> Self {
        let explorer = ExplorerState::new(&project);
        Self {
            journey: Default::default(),
            graph_request: 0,
            graph_expansions: 0,
            module_map: super::module_map::State::new(&project.architecture),
            map: super::map::State::new(&project.architecture),
            color_mode: super::theme::ColorMode::detect(),
            hotspots: None,
            hotspots_error: None,
            maintainability: None,
            maintainability_error: None,
            metrics: None,
            metrics_error: None,
            metrics_scroll: 0,
            metrics_scroll_limit: Default::default(),
            project,
            analyzer_state: AnalyzerState::Starting,
            view: View::Overview,
            pane: Pane::Project,
            overlay: Overlay::None,
            overview: super::overview::State::default(),
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
            explorer,
            notice: None,
        }
    }

    pub(super) fn select_overview(&mut self, index: usize) {
        if let Some(target) = super::overview::targets(self).get(index) {
            self.overview.selected = target.id();
        }
    }

    pub(super) fn switch_view(&mut self, view: View) {
        if self.view == View::Calls && view != View::Calls {
            self.calls.graph.cancel();
        }
        super::journey::cancel(self);
        self.view = view;
        self.overlay = Overlay::None;
        self.pane = match view {
            View::Overview => Pane::Project,
            View::Symbols => Pane::Symbols,
            View::Calls => Pane::CallCurrent,
            View::Explorer => Pane::ExplorerTree,
            View::Map => Pane::MapGraph,
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
                self.metrics_scroll = 0;
                let next = move_index(
                    super::overview::index(self).unwrap_or(0),
                    super::overview::targets(self).len(),
                    delta,
                );
                self.select_overview(next);
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
            Pane::ExplorerTree => {
                self.explorer.move_tree_selection(delta);
                self.explorer.right_mode = ExplorerRightMode::File;
            }
            Pane::ExplorerFile => {
                if self.explorer.right_mode == ExplorerRightMode::Inspector {
                    if let Some(inspector) = &mut self.inspector {
                        inspector.scroll_by(delta);
                    }
                } else {
                    self.explorer.move_file_selection(delta);
                }
            }
            Pane::Details => {
                let max = self
                    .metrics_scroll_limit
                    .get()
                    .unwrap_or_else(|| super::overview::details(self).len().saturating_sub(1));
                self.metrics_scroll = self
                    .metrics_scroll
                    .min(max)
                    .saturating_add_signed(delta)
                    .min(max);
            }
            Pane::CallCurrent => {
                self.calls.context_scroll = self
                    .calls
                    .context_scroll
                    .min(self.calls.context_scroll_limit.get().unwrap_or(0))
                    .saturating_add_signed(delta)
                    .min(self.calls.context_scroll_limit.get().unwrap_or(0))
            }
            Pane::MapGraph if self.module_map.active => {
                self.module_map.move_by(delta);
                self.map.inspector_scroll = 0;
            }
            Pane::MapGraph => self.map.move_by(&self.project.architecture, delta),
            Pane::MapInspector => {
                self.map.inspector_scroll = self
                    .map
                    .inspector_scroll
                    .min(self.map.scroll_limit.get().unwrap_or(0))
                    .saturating_add_signed(delta)
                    .min(self.map.scroll_limit.get().unwrap_or(0))
            }
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
            (View::Explorer, Pane::ExplorerTree) => Pane::ExplorerFile,
            (View::Explorer, Pane::ExplorerFile) => Pane::ExplorerTree,
            (View::Map, Pane::MapGraph) => Pane::MapInspector,
            (View::Map, Pane::MapInspector) => Pane::MapGraph,
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
            (View::Explorer, Pane::ExplorerTree) => Pane::ExplorerFile,
            (View::Explorer, Pane::ExplorerFile) => Pane::ExplorerTree,
            (View::Map, Pane::MapGraph) => Pane::MapInspector,
            (View::Map, Pane::MapInspector) => Pane::MapGraph,
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
            (View::Explorer, Pane::ExplorerTree, FocusDirection::Right) => Pane::ExplorerFile,
            (View::Explorer, Pane::ExplorerFile, FocusDirection::Left) => Pane::ExplorerTree,
            (View::Map, Pane::MapGraph, FocusDirection::Right) => Pane::MapInspector,
            (View::Map, Pane::MapInspector, FocusDirection::Left) => Pane::MapGraph,
            _ => self.pane,
        };
    }

    pub(super) fn escape(&mut self) -> Option<(u64, CallTarget, CallScope)> {
        if self.journey.pending.is_some() {
            super::journey::cancel(self);
            self.notice =
                Some("Pending semantic navigation cancelled; exact file anchor retained".into());
            return None;
        }
        match self.overlay {
            Overlay::None if self.view == View::Map => {
                if self.module_map.active {
                    self.module_map.back();
                    self.map.inspector_scroll = 0;
                } else {
                    self.map.back();
                }
                self.pane = Pane::MapGraph;
            }
            Overlay::Help | Overlay::Search => self.overlay = Overlay::None,
            Overlay::None
                if self.view == View::Explorer
                    && self.explorer.right_mode == ExplorerRightMode::Inspector =>
            {
                if super::journey::source_returns_to_calls(self) {
                    super::journey::back(self);
                    return None;
                }
                self.explorer.right_mode = ExplorerRightMode::File;
                self.pane = Pane::ExplorerFile;
            }
            Overlay::None if self.pane == Pane::Details => self.pane = Pane::Project,
            Overlay::None if self.pane == Pane::Inspector => self.pane = Pane::Symbols,
            Overlay::None if matches!(self.pane, Pane::Callers | Pane::Callees) => {
                if self.calls.graph.enabled {
                    self.pane = Pane::CallCurrent;
                    return self.escape();
                }
                self.pane = Pane::CallCurrent;
            }
            Overlay::None if self.pane == Pane::CallCurrent => {
                self.calls.graph.cancel();
                if let Some(visit) = self.calls.history.pop() {
                    self.notice = None;
                    if visit.state.center.is_some()
                        && !visit.state.callers_loading
                        && !visit.state.callees_loading
                    {
                        let id = self.calls.request_id + 1;
                        let history = std::mem::take(&mut self.calls.history);
                        self.calls = *visit.state;
                        self.calls.history = history;
                        self.calls.request_id = id;
                        return None;
                    }
                    self.calls.scope = visit.state.scope;
                    let graph = visit.state.graph;
                    let command = self.load_calls(visit.target, false);
                    if graph.focus.is_some() {
                        self.calls.graph = graph;
                    }
                    return Some(command);
                }
                self.calls.request_id += 1;
                if super::journey::back(self) {
                    return None;
                }
                self.switch_view(View::Symbols);
            }
            Overlay::None => {
                super::journey::back(self);
            }
        }
        self.notice = None;
        None
    }

    pub(super) fn activate_project_item(&mut self) {
        use super::overview::Target;
        let label = match super::overview::selected(self) {
            Some(Target::Group(group)) => {
                if !self.overview.expanded.remove(&group) {
                    self.overview.expanded.insert(group);
                }
                return;
            }
            Some(Target::Entrypoint(path)) => format!("Entrypoint: {}", path.display()),
            Some(Target::Package(name)) => format!("Crate: {name}"),
            _ => format!("Workspace: {}", project_name(&self.project)),
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
        self.search_request += 1; // Invalidate responses for the previous query immediately.
        if self.symbols_query.is_empty() {
            self.symbols_results.clear();
            self.symbols_selection = 0;
            self.symbols_truncated = false;
            self.symbols_loading = false;
            self.symbols_dirty_at = None;
            return;
        }
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
            View::Map => Pane::MapInspector,
            View::Explorer => {
                self.explorer.right_mode = ExplorerRightMode::Inspector;
                Pane::ExplorerFile
            }
        };
        Some((id, symbol))
    }

    pub(super) fn open_calls_for_inspector(&mut self) -> Option<(u64, CallTarget, CallScope)> {
        let from_calls_source =
            self.view == View::Explorer && super::journey::source_returns_to_calls(self);
        let symbol = match self.view {
            View::Symbols => self
                .symbols_results
                .get(self.symbols_selection)
                .cloned()
                .or_else(|| self.inspector.as_ref().map(|i| i.symbol.clone())),
            View::Explorer if self.explorer.right_mode == ExplorerRightMode::File => {
                if self.explorer.file.as_ref().is_some_and(|f| {
                    self.explorer
                        .selected_entry()
                        .is_some_and(|e| e.path == f.path)
                }) {
                    self.explorer.selected_symbol()
                } else {
                    None
                }
            }
            _ => self.inspector.as_ref().map(|i| i.symbol.clone()),
        };
        self.switch_view(View::Calls);
        let symbol = symbol?;
        let target = CallTarget::from_symbol(&symbol);
        if !crate::model::is_callable_kind(&target.kind) {
            let id = self.calls.request_id + 1;
            self.calls=CallsState { request_id:id,unavailable:Some(format!("{} is a {}.\n\nCall hierarchy is available for functions and methods.\nSelect a callable symbol to explore execution flow.",qualified_symbol(&symbol),target.kind)),center:Some(target),..Default::default() };
            return None;
        }
        if !from_calls_source {
            self.calls.history.clear();
        }
        Some(self.load_calls(target, from_calls_source))
    }

    pub(super) fn begin_explorer_symbol_inspect(&mut self) -> Option<(u64, Symbol)> {
        if self.explorer.file.as_ref().is_none_or(|f| {
            self.explorer
                .selected_entry()
                .is_none_or(|e| e.path != f.path)
        }) {
            return None;
        }
        let symbol = self.explorer.selected_symbol()?;
        self.inspect_request += 1;
        let id = self.inspect_request;
        self.inspector = Some(Inspector::loading(id, symbol.clone()));
        self.explorer.right_mode = ExplorerRightMode::Inspector;
        self.pane = Pane::ExplorerFile;
        Some((id, symbol))
    }

    pub(super) fn follow_selected_call(&mut self) -> Option<(u64, CallTarget, CallScope)> {
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

    pub(super) fn toggle_call_scope(&mut self) -> Option<(u64, CallTarget, CallScope)> {
        if self.calls.unavailable.is_some() {
            return None;
        }
        let target = self.calls.center.clone()?;
        self.calls.scope = self.calls.scope.toggled();
        Some(self.load_calls(target, false))
    }

    pub(super) fn load_calls(
        &mut self,
        target: CallTarget,
        remember: bool,
    ) -> (u64, CallTarget, CallScope) {
        self.calls.unavailable = None;
        self.calls.context_scroll = 0;
        if remember {
            if let Some(center) = self.calls.center.clone() {
                self.calls.graph.cancel();
                let mut history = std::mem::take(&mut self.calls.history);
                let state = Box::new(self.calls.clone());
                history.push(CallVisit {
                    target: center,
                    state,
                });
                self.calls.history = history;
                if self.calls.history.len() > CALL_HISTORY_LIMIT {
                    self.calls.history.remove(0);
                }
            }
        }
        self.calls.request_id += 1;
        self.calls.graph = super::call_graph::State::new(&target, self.calls.graph.enabled);
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
        self.calls.callers_unsupported = false;
        self.calls.callees_unsupported = false;
        self.pane = Pane::CallCurrent;
        (self.calls.request_id, target, self.calls.scope)
    }

    pub(super) fn apply_event(&mut self, event: AnalyzerEvent) {
        let advance = matches!(
            &event,
            AnalyzerEvent::FileSymbols { .. }
                | AnalyzerEvent::FileSymbolsFailed { .. }
                | AnalyzerEvent::Error(_)
        );
        match event {
            AnalyzerEvent::CallGraphExpanded { request, result } => {
                super::call_graph::apply(self, *request, result)
            }
            AnalyzerEvent::Indexing => self.analyzer_state = AnalyzerState::Indexing,
            AnalyzerEvent::Ready => self.analyzer_state = AnalyzerState::Ready,
            AnalyzerEvent::Error(message) => {
                self.calls.graph.cancel();
                if let Some(focus) = self.calls.graph.focus.clone() {
                    for incoming in [true, false] {
                        let loading = if incoming {
                            self.calls.callers_loading
                        } else {
                            self.calls.callees_loading
                        };
                        if loading {
                            self.calls
                                .graph
                                .ingest(&focus, incoming, Err(message.clone()));
                        }
                    }
                }
                self.calls.callers_loading = false;
                self.calls.callees_loading = false;
                if self.calls.center.is_some() {
                    self.calls.callers_error = Some(message.clone());
                    self.calls.callees_error = Some(message.clone());
                }
                self.analyzer_state = AnalyzerState::Error(message);
            }
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
                if let Some(focus) = self.calls.graph.focus.clone() {
                    self.calls.graph.ingest(&focus, true, Ok(result.clone()));
                }
                self.apply_callers(result);
            }
            AnalyzerEvent::CalleesLoaded { id, result } if id == self.calls.request_id => {
                if let Some(focus) = self.calls.graph.focus.clone() {
                    self.calls.graph.ingest(&focus, false, Ok(result.clone()));
                }
                self.apply_callees(result);
            }
            AnalyzerEvent::CallsFailed {
                id,
                incoming,
                message,
            } if id == self.calls.request_id => {
                if let Some(focus) = self.calls.graph.focus.clone() {
                    self.calls
                        .graph
                        .ingest(&focus, incoming, Err(message.clone()));
                }
                if incoming {
                    self.calls.callers_loading = false;
                    self.calls.callers_unsupported = false;
                    self.calls.callers_error = Some(message);
                } else {
                    self.calls.callees_loading = false;
                    self.calls.callees_unsupported = false;
                    self.calls.callees_error = Some(message);
                }
            }
            AnalyzerEvent::FileSymbols { id, file, symbols } => {
                super::journey::prepare_file_symbols(self, id, &file, &symbols);
                self.explorer.apply_file_symbols(id, &file, symbols);
            }
            AnalyzerEvent::FileSymbolsFailed { id, file, message } => {
                self.explorer.fail_file(id, &file, message)
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
        if advance {
            super::journey::advance(self);
        }
    }

    fn apply_callers(&mut self, result: Option<CallTargets>) {
        self.calls.callers_loading = false;
        self.calls.callers_unsupported = result.is_none();
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
        self.calls.callees_unsupported = result.is_none();
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
    use crate::repository::{RepoEntry, RepoEntryKind, RepositoryTree};

    pub(super) fn project() -> Project {
        Project {
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

    fn repository_tree() -> RepositoryTree {
        RepositoryTree {
            entries: vec![
                RepoEntry {
                    path: "src".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/tui".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/tui/worker.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(100),
                },
            ],
            truncated: false,
            skipped_errors: 0,
        }
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

        app.switch_view(View::Explorer);
        assert_eq!(app.pane, Pane::ExplorerTree);
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::ExplorerFile);
        app.focus_direction(FocusDirection::Right);
        assert_eq!(app.pane, Pane::ExplorerFile);
        app.focus_direction(FocusDirection::Left);
        assert_eq!(app.pane, Pane::ExplorerTree);
        app.next_pane();
        assert_eq!(app.pane, Pane::ExplorerFile);
        app.next_pane();
        assert_eq!(app.pane, Pane::ExplorerTree);
        app.previous_pane();
        assert_eq!(app.pane, Pane::ExplorerFile);
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
        let mut callable = symbol("A");
        callable.kind = "function".into();
        app.inspector = Some(Inspector::loading(1, callable));
        let (first, _, _) = app.open_calls_for_inspector().unwrap();
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
        let (second, target, _) = app.follow_selected_call().unwrap();
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
        let (_third, target, _) = app.follow_selected_call().unwrap();
        assert_eq!(target.name, "C");
        assert_eq!(app.calls.history.len(), 2);

        let (_, target, _) = app.escape().unwrap();
        assert_eq!(target.name, "B");
        let (_, target, _) = app.escape().unwrap();
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

    #[test]
    fn call_scope_toggle_reloads_neighbors_and_rejects_stale_events() {
        let mut app = App::new(project());
        app.calls.center = Some(call_target("root", 0));
        app.calls.callers = vec![call_target("old", 1)];
        app.calls.history = vec![call_target("history", 2).into()];
        let old_id = app.calls.request_id;
        let (new_id, target, scope) = app.toggle_call_scope().unwrap();
        assert_eq!(target.name, "root");
        assert_eq!(scope, CallScope::All);
        assert!(new_id > old_id);
        assert!(app.calls.callers.is_empty());
        assert_eq!(app.calls.history.len(), 1);

        app.apply_event(AnalyzerEvent::CallersLoaded {
            id: old_id,
            result: Some(CallTargets {
                items: vec![call_target("stale", 3)],
                truncated: false,
            }),
        });
        assert!(app.calls.callers.is_empty());
    }

    #[test]
    fn explorer_state_persists_and_file_events_and_inspector_are_exact() {
        let mut app = App::new(project());
        app.explorer.apply_tree(repository_tree());
        app.switch_view(View::Explorer);
        assert_eq!(app.pane, Pane::ExplorerTree);
        assert!(app.explorer.is_expanded(std::path::Path::new("src")));
        app.explorer.set_expanded("src/tui".into(), true);
        app.explorer.selected = app
            .explorer
            .visible_entries()
            .iter()
            .position(|entry| entry.path == std::path::Path::new("src/tui/worker.rs"))
            .unwrap();
        let (id, file) = app.explorer.open_selected().unwrap();
        let mut exact = symbol("Scheduler");
        exact.file = file.clone();
        exact.selection_range.start.line = 77;
        exact.selection_range.end.line = 77;
        app.apply_event(AnalyzerEvent::FileSymbols {
            id: id.saturating_sub(1),
            file: file.clone(),
            symbols: vec![symbol("stale")],
        });
        assert!(app.explorer.file.as_ref().unwrap().symbols.is_empty());
        app.apply_event(AnalyzerEvent::FileSymbols {
            id,
            file,
            symbols: vec![exact.clone()],
        });
        let (_, inspected) = app.begin_explorer_symbol_inspect().unwrap();
        assert_eq!(inspected.selection_range, exact.selection_range);
        assert_eq!(app.inspector.as_ref().unwrap().symbol, exact);
        assert_eq!(app.explorer.right_mode, ExplorerRightMode::Inspector);
        app.pane = Pane::ExplorerTree;
        assert!(app.escape().is_none());
        assert_eq!(app.explorer.right_mode, ExplorerRightMode::File);
        assert_eq!(app.pane, Pane::ExplorerFile);

        app.switch_view(View::Symbols);
        app.switch_view(View::Explorer);
        assert!(app.explorer.is_expanded(std::path::Path::new("src/tui")));
        assert_eq!(
            app.explorer.file.as_ref().unwrap().path,
            std::path::Path::new("src/tui/worker.rs")
        );
    }
}

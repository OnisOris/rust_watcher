//! Visual focus and manual selection remain separate; ingestion stays in activity/.
mod actions;
mod render;
pub(super) use actions::handle_key;
mod follow;
mod source;
#[cfg(test)]
mod tests;
mod tree;
use super::{
    app::{App, View},
    worker::AnalyzerCommand,
};
use crate::activity::{
    self,
    focus::{Query, Snapshot},
    watcher::{Message, Observation},
};
use crossterm::event::{KeyCode as K, KeyEvent};
pub(super) use render::render;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Tree,
    Timeline,
    Map,
}
impl Mode {
    fn next(self) -> Self {
        match self {
            Self::Tree => Self::Timeline,
            Self::Timeline => Self::Map,
            Self::Map => Self::Tree,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Tree => "Tree",
            Self::Timeline => "Timeline",
            Self::Map => "Map preview",
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Auto {
    Manual,
    #[default]
    Expand,
    Follow,
}
impl Auto {
    fn next(self) -> Self {
        match self {
            Self::Manual => Self::Expand,
            Self::Expand => Self::Follow,
            Self::Follow => Self::Manual,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Expand => "Expand active",
            Self::Follow => "Auto-focus",
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Sort {
    Name,
    Recent,
    #[default]
    Heat,
}
impl Sort {
    fn next(self) -> Self {
        match self {
            Self::Name => Self::Recent,
            Self::Recent => Self::Heat,
            Self::Heat => Self::Name,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Recent => "Recent",
            Self::Heat => "Heat",
        }
    }
}
#[derive(Clone)]
struct Row {
    path: PathBuf,
    dir: bool,
    depth: usize,
}
pub(super) struct State {
    pub data: activity::State,
    query: Query,
    snapshot: Snapshot,
    mode: Mode,
    auto: Auto,
    sort: Sort,
    heat: bool,
    follow: bool,
    density: usize,
    focus: Option<PathBuf>,
    selected: usize,
    rows: Vec<Row>,
    expanded: BTreeSet<PathBuf>,
    prior_dirs: BTreeSet<PathBuf>,
    directories_only: bool,
    timeline_selection: usize,
    map_selection: usize,
    summary: bool,
    summary_scroll: usize,
    revision: u64,
    refreshed: Instant,
    manual_until: Option<Instant>,
    candidate: Option<(PathBuf, Instant)>,
    cache: (Query, Sort, bool),
}
impl Default for State {
    fn default() -> Self {
        Self {
            data: Default::default(),
            query: Default::default(),
            snapshot: Default::default(),
            mode: Mode::Tree,
            auto: Auto::Follow,
            sort: Sort::Heat,
            heat: true,
            follow: true,
            density: 1,
            focus: None,
            selected: 0,
            rows: Vec::new(),
            expanded: BTreeSet::new(),
            prior_dirs: BTreeSet::new(),
            directories_only: false,
            timeline_selection: 0,
            map_selection: 0,
            summary: false,
            summary_scroll: 0,
            revision: u64::MAX,
            refreshed: Instant::now(),
            manual_until: None,
            candidate: None,
            cache: (Query::default(), Sort::Heat, false),
        }
    }
}
impl State {
    fn selected_event(&self) -> Option<&activity::Event> {
        let i = self
            .snapshot
            .events
            .iter()
            .rev()
            .nth(self.timeline_selection)?;
        self.data.events.get(*i)
    }
    fn selected_path(&self) -> Option<PathBuf> {
        if self.mode == Mode::Map {
            let cluster = self.snapshot.clusters.get(self.map_selection)?;
            self.snapshot
                .ranked_files
                .iter()
                .find(|p| p.parent().unwrap_or(Path::new("")) == cluster)
                .cloned()
        } else if self.mode == Mode::Timeline {
            self.selected_event()?.path.clone()
        } else {
            self.rows
                .get(self.selected)
                .filter(|r| !r.dir)
                .map(|r| r.path.clone())
        }
    }
    fn refresh(&mut self) {
        self.refresh_at(Instant::now());
    }
    fn refresh_at(&mut self, now: Instant) {
        let mut selected = self.rows.get(self.selected).map(|r| r.path.clone());
        for e in self
            .data
            .events
            .iter()
            .filter(|e| e.observed >= self.refreshed)
        {
            if e.kind == activity::Kind::Renamed && e.old_path == selected {
                selected = e.path.clone();
            }
            if e.kind == activity::Kind::Renamed && e.old_path == self.focus {
                self.focus = e.path.clone();
            }
        }
        let anchor = self.selected_event().map(|e| e.observed);
        let snapshot = Snapshot::build(&self.data, self.query, now);
        let rebuild = self.revision != self.data.revision
            || self.cache != (self.query, self.sort, self.directories_only)
            || !self.snapshot.files.keys().eq(snapshot.files.keys());
        self.snapshot = snapshot;
        self.update_focus(now);
        let mut expanded_changed = false;
        if self.auto != Auto::Manual {
            // Retain manual collapses until the next model revision, not every tick.
            if self.revision != self.data.revision {
                for p in self.snapshot.files.keys() {
                    for parent in p.ancestors().skip(1) {
                        expanded_changed |= self.expanded.insert(parent.to_path_buf());
                    }
                }
            }
            if self.camera_active(now) {
                if let Some(p) = &self.focus {
                    for parent in p.ancestors().skip(1) {
                        expanded_changed |= self.expanded.insert(parent.to_path_buf());
                    }
                }
            }
        }
        if rebuild || expanded_changed {
            self.rebuild_rows();
        }
        self.cache = (self.query, self.sort, self.directories_only);
        self.map_selection = self
            .map_selection
            .min(self.snapshot.clusters.len().saturating_sub(1));
        let restore = if self.camera_active(now) {
            self.focus.as_ref()
        } else {
            selected.as_ref()
        };
        if let Some(p) = restore {
            if let Some(i) = self.rows.iter().position(|r| &r.path == p) {
                self.selected = i;
            }
        }
        self.timeline_selection = anchor
            .and_then(|at| {
                self.snapshot
                    .events
                    .iter()
                    .rev()
                    .position(|i| self.data.events[*i].observed == at)
            })
            .unwrap_or(
                self.timeline_selection
                    .min(self.snapshot.events.len().saturating_sub(1)),
            );
        self.revision = self.data.revision;
        self.refreshed = now;
    }
    fn down(&mut self, delta: isize) {
        if self.summary {
            self.summary_scroll = self.summary_scroll.saturating_add_signed(delta).min(40);
        } else if self.mode == Mode::Map {
            self.map_selection = self
                .map_selection
                .saturating_add_signed(delta)
                .min(self.snapshot.clusters.len().saturating_sub(1));
        } else if self.mode == Mode::Timeline {
            self.timeline_selection = self
                .timeline_selection
                .saturating_add_signed(delta)
                .min(self.snapshot.events.len().saturating_sub(1));
        } else {
            self.selected = self
                .selected
                .saturating_add_signed(delta)
                .min(self.rows.len().saturating_sub(1));
            self.pause_camera(Instant::now());
        }
    }
    fn expand(&mut self) {
        self.pause_camera(Instant::now());
        if let Some(r) = self.rows.get(self.selected).cloned() {
            if r.dir {
                if !self.expanded.insert(r.path.clone()) {
                    self.expanded.remove(&r.path);
                }
                self.rebuild_rows();
            }
        }
    }
    fn parent(&mut self) {
        self.pause_camera(Instant::now());
        if let Some(r) = self.rows.get(self.selected).cloned() {
            if self.expanded.remove(&r.path) {
                self.rebuild_rows();
            } else if let Some(p) = r.path.parent() {
                if let Some(i) = self.rows.iter().position(|r| r.path == p) {
                    self.selected = i;
                }
            }
        }
    }
}
pub(super) fn poll(app: &mut App, observation: Option<&Observation>) -> bool {
    let s = &mut app.activity;
    let mut changed = false;
    let anchor = s.selected_event().map(|e| e.observed);
    if let Some(o) = observation {
        if o.lost.swap(false, std::sync::atomic::Ordering::Relaxed) {
            s.data.incomplete = true;
            changed = true;
        }
        for message in o.rx.try_iter().take(128) {
            changed = true;
            match message {
                Message::Snapshot(tree) => s.data.snapshot(tree),
                Message::Scope(tree) => s.data.refresh_scope(tree),
                Message::Event(event) => s.data.apply(event),
                Message::Status(status, watching) => {
                    s.data.status = status;
                    s.data.watching = watching;
                }
                Message::AgentStatus(status) => s.data.agent_status = status,
                Message::MeasurementStatus(status) => s.data.measurement_status = status,
            }
        }
    }
    let due = s.refreshed.elapsed() >= Duration::from_secs(1);
    if s.revision != s.data.revision || due {
        s.refresh();
        changed = true;
    }
    if let Some(at) = anchor {
        if let Some(i) = s
            .snapshot
            .events
            .iter()
            .rev()
            .position(|i| s.data.events[*i].observed == at)
        {
            s.timeline_selection = i;
        }
    }
    changed && app.view == View::Activity
}

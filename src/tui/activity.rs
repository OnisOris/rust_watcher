//! Agent Focus navigation caches physical paths; no semantic or graph layout state.
mod render;
#[cfg(test)]
mod tests;
use super::{
    app::{App, View},
    worker::AnalyzerCommand,
};
use crate::activity::{
    self,
    watcher::{Message, Observation},
    Filter,
};
use crossterm::event::{KeyCode as K, KeyEvent};
pub(super) use render::render;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
#[derive(Clone)]
struct Row {
    path: PathBuf,
    dir: bool,
    depth: usize,
}
struct Column {
    parent: PathBuf,
    selected: usize,
}
pub(super) struct State {
    pub data: activity::State,
    filter: Filter,
    directories_only: bool,
    columns: Vec<Column>,
    column: usize,
    children: BTreeMap<PathBuf, Vec<Row>>,
    counts: BTreeMap<PathBuf, usize>,
    rows: Vec<Row>,
    expanded: BTreeSet<PathBuf>,
    compact_selection: usize,
    compact: std::cell::Cell<bool>,
    timeline: bool,
    timeline_selection: usize,
    summary: bool,
    summary_scroll: usize,
    revision: u64,
    refreshed: Instant,
}
impl Default for State {
    fn default() -> Self {
        Self {
            data: Default::default(),
            filter: Filter::Touched,
            directories_only: false,
            columns: vec![Column {
                parent: PathBuf::new(),
                selected: 0,
            }],
            column: 0,
            children: BTreeMap::new(),
            counts: BTreeMap::new(),
            rows: Vec::new(),
            expanded: BTreeSet::new(),
            compact_selection: 0,
            compact: Default::default(),
            timeline: false,
            timeline_selection: 0,
            summary: false,
            summary_scroll: 0,
            revision: u64::MAX,
            refreshed: Instant::now(),
        }
    }
}
impl State {
    fn selected(&self) -> Option<&Row> {
        if self.compact.get() {
            self.rows.get(self.compact_selection)
        } else {
            let c = &self.columns[self.column];
            self.children.get(&c.parent)?.get(c.selected)
        }
    }
    fn selected_path(&self) -> Option<PathBuf> {
        if self.timeline {
            self.data
                .events
                .iter()
                .rev()
                .nth(self.timeline_selection)?
                .path
                .clone()
        } else {
            self.selected().filter(|r| !r.dir).map(|r| r.path.clone())
        }
    }
    fn refresh(&mut self) {
        let old: Vec<_> = self
            .columns
            .iter()
            .map(|c| {
                self.children
                    .get(&c.parent)
                    .and_then(|v| v.get(c.selected))
                    .map(|r| r.path.clone())
            })
            .collect();
        let compact = self
            .rows
            .get(self.compact_selection)
            .map(|r| r.path.clone());
        let touched_counts = self.data.directories(if self.filter == Filter::All {
            Filter::Touched
        } else {
            self.filter
        });
        let mut entries = BTreeMap::<PathBuf, bool>::new();
        for (path, _) in self.data.files.iter().chain(
            self.data
                .touched
                .keys()
                .filter(|p| !self.data.files.contains_key(*p))
                .map(|p| (p, &false)),
        ) {
            if !self.data.matches(path, self.filter) {
                continue;
            }
            if !self.directories_only {
                entries.insert(path.clone(), false);
            }
            for p in path
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                entries.insert(p.to_path_buf(), true);
            }
        }
        self.children.clear();
        for (path, dir) in entries {
            self.children
                .entry(path.parent().unwrap_or(Path::new("")).to_path_buf())
                .or_default()
                .push(Row {
                    path,
                    dir,
                    depth: 0,
                });
        }
        for rows in self.children.values_mut() {
            rows.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.path.cmp(&b.path)));
        }
        for (i, c) in self.columns.iter_mut().enumerate() {
            let rows = self
                .children
                .get(&c.parent)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            c.selected = old
                .get(i)
                .and_then(|p| p.as_ref())
                .and_then(|p| rows.iter().position(|r| &r.path == p))
                .unwrap_or(c.selected.min(rows.len().saturating_sub(1)));
        }
        self.counts = touched_counts;
        self.rebuild_rows();
        if let Some(path) = compact {
            if let Some(i) = self.rows.iter().position(|r| r.path == path) {
                self.compact_selection = i;
            }
        }
        self.revision = self.data.revision;
        self.refreshed = Instant::now();
    }
    fn rebuild_rows(&mut self) {
        fn walk(parent: &Path, depth: usize, s: &State, out: &mut Vec<Row>) {
            if let Some(rows) = s.children.get(parent) {
                for row in rows {
                    let mut r = row.clone();
                    r.depth = depth;
                    out.push(r);
                    if row.dir && s.expanded.contains(&row.path) {
                        walk(&row.path, depth + 1, s, out);
                    }
                }
            }
        }
        let mut rows = Vec::new();
        walk(Path::new(""), 0, self, &mut rows);
        self.rows = rows;
        self.compact_selection = self
            .compact_selection
            .min(self.rows.len().saturating_sub(1));
    }
    fn down(&mut self, delta: isize) {
        if self.summary {
            self.summary_scroll = self.summary_scroll.saturating_add_signed(delta).min(24);
            return;
        }
        if self.timeline {
            self.timeline_selection = self
                .timeline_selection
                .saturating_add_signed(delta)
                .min(self.data.events.len().saturating_sub(1));
            return;
        }
        if self.compact.get() {
            self.compact_selection = self
                .compact_selection
                .saturating_add_signed(delta)
                .min(self.rows.len().saturating_sub(1));
        } else {
            let c = &mut self.columns[self.column];
            let len = self.children.get(&c.parent).map_or(0, Vec::len);
            c.selected = c
                .selected
                .saturating_add_signed(delta)
                .min(len.saturating_sub(1));
            self.columns.truncate(self.column + 1);
        }
    }
    fn right(&mut self) {
        let Some(row) = self.selected().cloned() else {
            return;
        };
        if !row.dir {
            return;
        }
        if self.compact.get() {
            self.expanded.insert(row.path);
            self.rebuild_rows();
        } else {
            self.columns.truncate(self.column + 1);
            self.columns.push(Column {
                parent: row.path,
                selected: 0,
            });
            self.column += 1;
        }
    }
    fn left(&mut self) {
        if self.compact.get() {
            if let Some(row) = self.selected().cloned() {
                if self.expanded.remove(&row.path) {
                    self.rebuild_rows();
                } else if let Some(p) = row.path.parent() {
                    if let Some(i) = self.rows.iter().position(|r| r.path == p) {
                        self.compact_selection = i;
                    }
                }
            }
        } else {
            self.column = self.column.saturating_sub(1);
        }
    }
}
pub(super) fn poll(app: &mut App, observation: Option<&Observation>) -> bool {
    let s = &mut app.activity;
    let mut changed = false;
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
                Message::Event(event) => {
                    // Keep a selected timeline event anchored while new events arrive.
                    if s.timeline && !s.data.events.is_empty() {
                        s.timeline_selection =
                            (s.timeline_selection + 1).min(activity::HISTORY_LIMIT - 1);
                    }
                    s.data.apply(event);
                }
                Message::Status(status, watching) => {
                    s.data.status = status;
                    s.data.watching = watching;
                }
                Message::AgentStatus(status) => s.data.agent_status = status,
            }
        }
    }
    if s.revision != s.data.revision || s.refreshed.elapsed() >= Duration::from_secs(1) {
        if s.revision != s.data.revision || s.filter == Filter::Recent {
            s.refresh();
        }
        s.refreshed = Instant::now();
        changed = true;
    }
    changed && app.view == View::Activity
}
pub(super) async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    tx: &tokio::sync::mpsc::Sender<AnalyzerCommand>,
) -> bool {
    let s = &mut app.activity;
    match key.code {
        K::Char('f') => {
            s.filter = s.filter.next();
            s.refresh();
        }
        K::Char('d') => {
            s.directories_only = !s.directories_only;
            s.refresh();
        }
        K::Char('t') | K::Tab | K::BackTab => {
            s.timeline = !s.timeline;
            s.summary = false;
        }
        K::Char('s') => {
            s.summary = !s.summary;
            s.summary_scroll = 0;
        }
        K::Char('j') | K::Down => s.down(1),
        K::Char('k') | K::Up => s.down(-1),
        K::PageDown => s.down(10),
        K::PageUp => s.down(-10),
        K::Char('h') | K::Left => s.left(),
        K::Char('l') | K::Right => s.right(),
        K::Esc if s.summary => s.summary = false,
        K::Esc if s.timeline => s.timeline = false,
        K::Esc if s.column > 0 || s.compact.get() => s.left(),
        K::Enter if !s.timeline && s.selected().is_some_and(|r| r.dir) => s.right(),
        K::Enter | K::Char('o') => {
            if let Some(path) = s.selected_path() {
                let root = &app.project.workspace_root;
                if !s.data.files.get(&path).copied().unwrap_or(false) || !root.join(&path).is_file()
                {
                    app.notice = Some(format!("Deleted / unavailable: {}", path.display()));
                    return true;
                }
                if activity::relative(root, &path).is_none()
                    || !activity::watcher::admitted(root, &path)
                {
                    app.notice = Some("Source outside current observation scope".into());
                    return true;
                }
                // Extend the existing Explorer index with an observed new location.
                if let Some(tree) = &app.explorer.tree {
                    if !tree.entries.iter().any(|e| e.path == path) {
                        let mut tree = tree.clone();
                        for p in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
                            if !tree.entries.iter().any(|e| e.path == p) {
                                tree.entries.push(crate::repository::RepoEntry {
                                    path: p.to_path_buf(),
                                    kind: if p == path {
                                        crate::repository::RepoEntryKind::File
                                    } else {
                                        crate::repository::RepoEntryKind::Directory
                                    },
                                    size: None,
                                });
                            }
                        }
                        app.explorer.apply_tree(tree);
                    }
                }
                super::event::open_exact_file(app, tx, path, None).await;
            }
        }
        K::Char('m') => {
            let origin = super::journey::point(app, View::Map);
            app.switch_view(View::Map);
            super::journey::remember(app, origin);
        }
        _ => return false,
    }
    true
}

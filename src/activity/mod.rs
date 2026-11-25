//! Local observations, independent of Rust semantics and terminal layout.
pub mod focus;
#[cfg(test)]
mod focus_tests;
pub mod stream;
#[cfg(test)]
mod tests;
pub mod watcher;
use crate::repository::{RepoEntryKind, RepositoryTree};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub const HISTORY_LIMIT: usize = 1000;
pub const TOUCHED_LIMIT: usize = 5000;
pub const RECENT: Duration = Duration::from_secs(60);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Created,
    Modified,
    Deleted,
    Renamed,
    Read,
    Write,
    Search,
    Command,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Filesystem,
    Agent,
}
#[derive(Debug, Clone)]
pub struct Event {
    pub at: SystemTime,
    pub observed: Instant,
    pub kind: Kind,
    pub source: Source,
    pub path: Option<PathBuf>,
    pub old_path: Option<PathBuf>,
    pub details: Option<String>,
}
impl Event {
    pub fn filesystem(kind: Kind, path: PathBuf) -> Self {
        Self {
            at: SystemTime::now(),
            observed: Instant::now(),
            kind,
            source: Source::Filesystem,
            path: Some(path),
            old_path: None,
            details: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Touched {
    pub last: Instant,
    pub at: SystemTime,
    pub filesystem: Option<Kind>,
    pub agent: Option<Kind>,
    pub events: usize,
    pub created: bool,
    pub deleted: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Touched,
    Recent,
    Modified,
}
impl Filter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All files",
            Self::Touched => "Touched files",
            Self::Recent => "Recent activity (60s)",
            Self::Modified => "Modified only",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Touched => Self::Recent,
            Self::Recent => Self::Modified,
            Self::Modified => Self::All,
            Self::All => Self::Touched,
        }
    }
}
pub struct State {
    pub started: Instant,
    pub watching: bool,
    pub status: String,
    pub incomplete: bool,
    pub files: BTreeMap<PathBuf, bool>, // false retains a deleted location
    pub touched: BTreeMap<PathBuf, Touched>,
    pub events: VecDeque<Event>,
    pub total_events: usize,
    pub agent_events: usize,
    pub agent_status: String,
    pub revision: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            watching: false,
            status: "Starting observation / initial snapshot".into(),
            incomplete: false,
            files: BTreeMap::new(),
            touched: BTreeMap::new(),
            events: VecDeque::new(),
            total_events: 0,
            agent_events: 0,
            agent_status: "Not connected".into(),
            revision: 0,
        }
    }
}
impl State {
    pub fn refresh_scope(&mut self, tree: RepositoryTree) {
        self.incomplete |= tree.truncated || tree.skipped_errors > 0;
        self.files = tree
            .entries
            .into_iter()
            .filter(|e| e.kind == RepoEntryKind::File)
            .map(|e| (e.path, true))
            .collect();
        self.revision += 1;
    }
    pub fn snapshot(&mut self, tree: RepositoryTree) {
        self.refresh_scope(tree);
        self.started = Instant::now();
        self.watching = true;
        self.status = "Watching · initial disk state is baseline".into();
        self.revision += 1;
    }
    pub fn apply(&mut self, event: Event) {
        // A correlated move retains one file identity at its new physical path.
        // Earlier timeline entries retain their observed locations.
        if event.source == Source::Filesystem && event.kind == Kind::Renamed {
            if let (Some(old), Some(new)) = (&event.old_path, &event.path) {
                self.files.remove(old);
                if let Some(prior) = self.touched.remove(old) {
                    self.touched
                        .entry(new.clone())
                        .and_modify(|current| {
                            current.events += prior.events;
                            current.created |= prior.created;
                            current.deleted |= prior.deleted;
                        })
                        .or_insert(prior);
                }
            }
        }
        if event.source == Source::Agent {
            self.agent_events += 1;
        }
        self.total_events += 1;
        if let Some(path) = &event.path {
            if !self.touched.contains_key(path) && self.touched.len() >= TOUCHED_LIMIT {
                self.incomplete = true;
            } else {
                let touched = self.touched.entry(path.clone()).or_insert(Touched {
                    last: event.observed,
                    at: event.at,
                    filesystem: None,
                    agent: None,
                    events: 0,
                    created: false,
                    deleted: false,
                });
                touched.last = event.observed;
                touched.at = event.at;
                touched.events += 1;
                if event.source == Source::Filesystem {
                    touched.created |= event.kind == Kind::Created;
                    touched.deleted |= event.kind == Kind::Deleted;
                }
                match event.source {
                    Source::Filesystem => touched.filesystem = Some(event.kind),
                    Source::Agent => touched.agent = Some(event.kind),
                }
                if self.files.contains_key(path)
                    || self.files.len() < crate::repository::DEFAULT_ENTRY_LIMIT
                {
                    if event.source == Source::Filesystem {
                        self.files.insert(path.clone(), event.kind != Kind::Deleted);
                    }
                } else {
                    self.incomplete = true;
                }
            }
        }
        if self.events.len() == HISTORY_LIMIT {
            self.events.pop_front();
            self.incomplete = true;
        }
        self.events.push_back(event);
        self.revision += 1;
    }
    pub fn matches(&self, path: &Path, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Touched => self.touched.contains_key(path),
            Filter::Recent => self
                .touched
                .get(path)
                .is_some_and(|t| t.last.elapsed() < RECENT),
            Filter::Modified => self
                .touched
                .get(path)
                .is_some_and(|t| t.filesystem.is_some()),
        }
    }
    #[cfg(test)]
    pub fn directories(&self, filter: Filter) -> BTreeMap<PathBuf, usize> {
        let mut counts = BTreeMap::new();
        for path in self
            .files
            .keys()
            .chain(self.touched.keys().filter(|p| !self.files.contains_key(*p)))
        {
            if !self.matches(path, filter) {
                continue;
            }
            for parent in path.ancestors().skip(1) {
                *counts.entry(parent.to_path_buf()).or_insert(0) += 1;
            }
        }
        counts
    }
}
/// Reject traversal, absolute paths and every symlink component, including dangling links.
pub fn relative(root: &Path, path: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let mut joined = root.to_path_buf();
    for c in path.components() {
        joined.push(c);
        if std::fs::symlink_metadata(&joined).is_ok_and(|m| m.file_type().is_symlink()) {
            return None;
        }
    }
    Some(path.to_path_buf())
}
pub fn clock(at: SystemTime) -> String {
    let s = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86400;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

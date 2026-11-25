//! Bounded activity projections. Scores measure observed recency/frequency, not importance.
use super::{Event, Filter, Kind, Source, State};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sources {
    Filesystem,
    Agent,
    #[default]
    Merged,
}
impl Sources {
    pub fn next(self) -> Self {
        match self {
            Self::Merged => Self::Filesystem,
            Self::Filesystem => Self::Agent,
            Self::Agent => Self::Merged,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Merged => "Merged",
            Self::Filesystem => "Filesystem",
            Self::Agent => "Agent reports",
        }
    }
    fn accepts(self, s: Source) -> bool {
        matches!(
            (self, s),
            (Self::Merged, _)
                | (Self::Filesystem, Source::Filesystem)
                | (Self::Agent, Source::Agent)
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Types {
    #[default]
    All,
    Modified,
    CreatedDeleted,
    Renamed,
    Agent,
    Hot,
}
impl Types {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Modified,
            Self::Modified => Self::CreatedDeleted,
            Self::CreatedDeleted => Self::Renamed,
            Self::Renamed => Self::Agent,
            Self::Agent => Self::Hot,
            Self::Hot => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All events",
            Self::Modified => "Modified",
            Self::CreatedDeleted => "New / deleted",
            Self::Renamed => "Renamed",
            Self::Agent => "Agent only",
            Self::Hot => "Hot (15s)",
        }
    }
    fn accepts(self, e: &Event, age: Duration) -> bool {
        match self {
            Self::All => true,
            Self::Modified => e.source == Source::Filesystem && e.kind == Kind::Modified,
            Self::CreatedDeleted => {
                e.source == Source::Filesystem && matches!(e.kind, Kind::Created | Kind::Deleted)
            }
            Self::Renamed => e.source == Source::Filesystem && e.kind == Kind::Renamed,
            Self::Agent => e.source == Source::Agent,
            Self::Hot => age.as_secs() < 15,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Src,
    Tests,
    Docs,
    Config,
}
impl Scope {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Src,
            Self::Src => Self::Tests,
            Self::Tests => Self::Docs,
            Self::Docs => Self::Config,
            Self::Config => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Workspace",
            Self::Src => "src",
            Self::Tests => "tests",
            Self::Docs => "docs",
            Self::Config => "config",
        }
    }
    fn accepts(self, path: Option<&Path>) -> bool {
        if self == Self::All {
            return true;
        }
        let Some(p) = path else {
            return false;
        };
        match self {
            Self::Src => p.starts_with("src"),
            Self::Tests => p.starts_with("tests"),
            Self::Docs => p.starts_with("docs"),
            Self::Config => {
                p.starts_with("config")
                    || p.extension().is_some_and(|x| {
                        matches!(x.to_str(), Some("toml" | "yaml" | "yml" | "json"))
                    })
                    || p.file_name()
                        .is_some_and(|n| n == ".gitignore" || n == ".ignore")
            }
            Self::All => true,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Window {
    #[default]
    Retained,
    Minute,
    Quarter,
}
impl Window {
    pub fn next(self) -> Self {
        match self {
            Self::Retained => Self::Minute,
            Self::Minute => Self::Quarter,
            Self::Quarter => Self::Retained,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Retained => "Session · retained",
            Self::Minute => "Last 60s",
            Self::Quarter => "Last 15s",
        }
    }
    fn accepts(self, age: Duration) -> bool {
        match self {
            Self::Retained => true,
            Self::Minute => age.as_secs() < 60,
            Self::Quarter => age.as_secs() < 15,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub files: Filter,
    pub sources: Sources,
    pub types: Types,
    pub scope: Scope,
    pub window: Window,
}
impl Query {
    pub fn accepts(self, e: &Event, now: Instant) -> bool {
        let age = now.saturating_duration_since(e.observed);
        (match self.files {
            Filter::Recent => age.as_secs() < 60,
            Filter::Modified => e.source == Source::Filesystem,
            _ => true,
        }) && self.sources.accepts(e.source)
            && self.types.accepts(e, age)
            && self.scope.accepts(e.path.as_deref())
            && self.window.accepts(age)
    }
}
#[derive(Clone)]
pub struct File {
    pub events: usize,
    pub filesystem: usize,
    pub agent: usize,
    pub score: u32,
    recent: u32,
    pub last: Instant,
    pub latest: usize,
}
#[derive(Clone)]
pub struct Directory {
    pub files: usize,
    pub events: usize,
    pub score: u32,
    pub latest: usize,
    pub last: Instant,
}
#[derive(Default)]
pub struct Snapshot {
    pub files: BTreeMap<PathBuf, File>,
    pub directories: BTreeMap<PathBuf, Directory>,
    pub events: Vec<usize>,
    pub mix: [usize; 5],
    pub filesystem: usize,
    pub agent: usize,
    pub ranked_files: Vec<PathBuf>,
    pub clusters: Vec<PathBuf>,
    pub flow: Vec<PathBuf>,
    pub buckets: [usize; 12],
    pub stability: &'static str,
}
pub fn heat(age: Duration) -> u32 {
    match age.as_secs() {
        0..=4 => 8,
        5..=14 => 5,
        15..=59 => 2,
        60..=119 => 1,
        _ => 0,
    }
}
pub fn marker(e: &Event) -> &'static str {
    if e.source == Source::Agent {
        "A"
    } else {
        match e.kind {
            Kind::Created => "N",
            Kind::Deleted => "D",
            Kind::Renamed => "R",
            _ => "M",
        }
    }
}
pub fn action(e: &Event) -> &'static str {
    match (e.source, e.kind) {
        (Source::Agent, Kind::Read) => "reported read",
        (Source::Agent, Kind::Write) => "reported write",
        (Source::Agent, Kind::Search) => "reported search",
        (Source::Agent, _) => "reported command",
        (_, Kind::Created) => "created",
        (_, Kind::Deleted) => "deleted",
        (_, Kind::Renamed) => "renamed",
        _ => "modified",
    }
}
impl Snapshot {
    pub fn build(state: &State, query: Query, now: Instant) -> Self {
        let mut result = Self::default();
        for (index, event) in state.events.iter().enumerate() {
            if !query.accepts(event, now) {
                continue;
            }
            result.events.push(index);
            let age = now.saturating_duration_since(event.observed);
            if age.as_secs() < 60 {
                result.buckets[(age.as_secs() / 5) as usize] += 1;
            }
            let mix = if event.source == Source::Agent {
                result.agent += 1;
                4
            } else {
                result.filesystem += 1;
                match event.kind {
                    Kind::Created => 1,
                    Kind::Deleted => 2,
                    Kind::Renamed => 3,
                    _ => 0,
                }
            };
            result.mix[mix] += 1;
            let Some(path) = &event.path else {
                continue;
            };
            if let Some(old) = &event.old_path {
                if let Some(prior) = result.files.remove(old) {
                    let new = result.files.entry(path.clone()).or_insert(File {
                        events: 0,
                        filesystem: 0,
                        agent: 0,
                        score: 0,
                        recent: 0,
                        last: event.observed,
                        latest: index,
                    });
                    new.events += prior.events;
                    new.filesystem += prior.filesystem;
                    new.agent += prior.agent;
                    new.recent += prior.recent;
                }
            }
            let file = result.files.entry(path.clone()).or_insert(File {
                events: 0,
                filesystem: 0,
                agent: 0,
                score: 0,
                recent: 0,
                last: event.observed,
                latest: index,
            });
            file.events += 1;
            file.recent += u32::from(age.as_secs() < 60);
            file.last = event.observed;
            file.latest = index;
            if event.source == Source::Agent {
                file.agent += 1;
            } else {
                file.filesystem += 1;
            }
            if result.flow.last() != Some(path) {
                result.flow.push(path.clone());
            }
        }
        for file in result.files.values_mut() {
            file.score = heat(now.saturating_duration_since(file.last))
                * (1 + file.recent.saturating_sub(1).min(3));
        }
        for (path, file) in &result.files {
            for parent in path
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                let dir = result
                    .directories
                    .entry(parent.to_path_buf())
                    .or_insert(Directory {
                        files: 0,
                        events: 0,
                        score: 0,
                        latest: file.latest,
                        last: file.last,
                    });
                dir.files += 1;
                dir.events += file.events;
                dir.score = dir.score.saturating_add(file.score);
                if file.last > dir.last {
                    dir.last = file.last;
                    dir.latest = file.latest;
                }
            }
        }
        result.ranked_files = result.files.keys().cloned().collect();
        result.ranked_files.sort_by(|a, b| {
            let (a_file, b_file) = (&result.files[a], &result.files[b]);
            b_file
                .score
                .cmp(&a_file.score)
                .then(b_file.last.cmp(&a_file.last))
                .then(a.cmp(b))
        });
        let mut clusters: BTreeMap<PathBuf, (u32, usize)> = BTreeMap::new();
        for (p, f) in &result.files {
            let c = clusters
                .entry(p.parent().unwrap_or(Path::new("")).to_path_buf())
                .or_default();
            c.0 = c.0.saturating_add(f.score);
            c.1 += f.events;
        }
        result.clusters = clusters.keys().cloned().collect();
        result
            .clusters
            .sort_by(|a, b| clusters[b].cmp(&clusters[a]).then(a.cmp(b)));
        let recent: Vec<_> = result
            .events
            .iter()
            .rev()
            .filter_map(|i| {
                let e = &state.events[*i];
                (now.saturating_duration_since(e.observed).as_secs() < 60)
                    .then_some(e.path.as_deref())
                    .flatten()
            })
            .take(20)
            .collect();
        let mut parents = BTreeMap::<&Path, usize>::new();
        for p in &recent {
            *parents
                .entry(p.parent().unwrap_or(Path::new("")))
                .or_default() += 1;
        }
        result.stability = if recent.len() < 3 {
            "Not enough observations"
        } else if parents.values().max().copied().unwrap_or(0) * 4 >= recent.len() * 3 {
            "Concentrated · one directory"
        } else {
            "Distributed · several directories"
        };
        result
    }
    pub fn hottest(&self) -> Option<&PathBuf> {
        self.ranked_files.first()
    }
}

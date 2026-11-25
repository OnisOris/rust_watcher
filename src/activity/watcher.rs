//! OS notifications are hints. Reconcile only affected paths after a 150ms window.
use super::{relative, Event, Kind};
use crate::repository::{self, RepoEntryKind, RepositoryTree};
use notify::{
    event::ModifyKind, event::RenameMode, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
    Arc,
};
use std::time::{Duration, SystemTime};

pub enum Message {
    Snapshot(RepositoryTree),
    Scope(RepositoryTree),
    Event(Event),
    Status(String, bool),
    AgentStatus(String),
}
pub struct Observation {
    pub rx: Receiver<Message>,
    pub lost: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Observation {
    pub fn start(root: PathBuf, stream: Option<PathBuf>) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(256);
        let lost = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (lost_worker, stop_worker) = (lost.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("workspace-activity".into())
            .spawn(move || {
                if let Err(e) = run(&root, stream, &tx, &lost_worker, &stop_worker) {
                    send(
                        &tx,
                        &lost_worker,
                        Message::Status(format!("Watch failed: {e}"), false),
                    );
                }
            })?;
        Ok(Self {
            rx,
            lost,
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}
fn send(tx: &SyncSender<Message>, lost: &AtomicBool, message: Message) {
    if tx.try_send(message).is_err() {
        lost.store(true, Ordering::Relaxed);
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}
fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::symlink_metadata(path).ok()?;
    m.is_file().then(|| Stamp {
        size: m.len(),
        modified: m.modified().ok(),
    })
}
/// Use the same walker (including parent/global ignores) as RepositoryTree.
/// Admission checks walk a single level; they never walk the whole repository.
pub fn admitted(root: &Path, path: &Path) -> bool {
    Scope::default().admitted(root, path)
}
#[derive(Default)]
struct Scope {
    children: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    entries: usize,
    incomplete: bool,
}
impl Scope {
    fn admitted(&mut self, root: &Path, path: &Path) -> bool {
        let Some(path) = relative(root, path) else {
            return false;
        };
        let mut parent = root.to_path_buf();
        for component in path.components() {
            let child = parent.join(component);
            if !self.children.contains_key(&parent) {
                let remaining = repository::DEFAULT_ENTRY_LIMIT.saturating_sub(self.entries);
                let mut entries = BTreeSet::new();
                for entry in repository::walker(&parent)
                    .max_depth(Some(1))
                    .build()
                    .filter_map(Result::ok)
                    .filter(|e| e.depth() == 1 && !e.file_type().is_some_and(|t| t.is_symlink()))
                    .take(remaining + 1)
                {
                    if entries.len() == remaining {
                        self.incomplete = true;
                        break;
                    }
                    entries.insert(entry.path().to_path_buf());
                }
                self.entries += entries.len();
                self.children.insert(parent.clone(), entries);
            }
            let entries = &self.children[&parent];
            if !entries.contains(&child) {
                return false;
            }
            parent = child;
        }
        true
    }
}

struct Disk {
    known: BTreeMap<PathBuf, Stamp>,
    watched: BTreeSet<PathBuf>,
    scope: Scope,
}
impl Disk {
    fn observe_dir(
        &mut self,
        watcher: &mut RecommendedWatcher,
        root: &Path,
        path: &Path,
    ) -> anyhow::Result<()> {
        if self.watched.len() >= repository::DEFAULT_ENTRY_LIMIT {
            anyhow::bail!("directory watch limit");
        }
        if self.watched.insert(path.to_path_buf()) {
            watcher.watch(&root.join(path), RecursiveMode::NonRecursive)?;
        }
        Ok(())
    }
    fn reconcile(
        &mut self,
        root: &Path,
        path: PathBuf,
        tx: &SyncSender<Message>,
        lost: &AtomicBool,
    ) {
        if relative(root, &path).is_none() {
            return;
        }
        let now = stamp(&root.join(&path));
        let old = self.known.get(&path);
        let kind = match (old, &now) {
            (None, Some(_)) if self.scope.admitted(root, &path) => Some(Kind::Created),
            (Some(a), Some(b)) if a != b => Some(Kind::Modified),
            (Some(_), None) => Some(Kind::Deleted),
            _ => None,
        };
        if let Some(kind) = kind {
            if let Some(now) = now {
                if self.known.len() >= repository::DEFAULT_ENTRY_LIMIT
                    && !self.known.contains_key(&path)
                {
                    lost.store(true, Ordering::Relaxed);
                    return;
                }
                self.known.insert(path.clone(), now);
            } else {
                self.known.remove(&path);
            }
            send(tx, lost, Message::Event(Event::filesystem(kind, path)));
        }
    }
}
fn run(
    root: &Path,
    stream: Option<PathBuf>,
    tx: &SyncSender<Message>,
    lost: &AtomicBool,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    let (raw_tx, raw_rx) = mpsc::sync_channel(1024);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = overflow.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if matches!(&event, Ok(e) if matches!(e.kind, EventKind::Access(_))) {
            return;
        }
        if raw_tx.try_send(event).is_err() {
            callback_overflow.store(true, Ordering::Relaxed);
        }
    })?;
    let mut disk = Disk {
        known: BTreeMap::new(),
        watched: BTreeSet::new(),
        scope: Scope::default(),
    };
    disk.observe_dir(&mut watcher, root, Path::new(""))?;
    let tree = repository::scan(root)?;
    for e in &tree.entries {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match e.kind {
            RepoEntryKind::Directory => disk.observe_dir(&mut watcher, root, &e.path)?,
            RepoEntryKind::File => {
                if let Some(s) = stamp(&root.join(&e.path)) {
                    disk.known.insert(e.path.clone(), s);
                }
            }
            _ => {}
        }
    }
    send(tx, lost, Message::Snapshot(tree));
    let mut journal = stream.map(super::stream::Tail::new);
    if journal.is_some() {
        send(
            tx,
            lost,
            Message::AgentStatus("Connected · explicit JSONL adapter".into()),
        );
    }
    while !stop.load(Ordering::Relaxed) {
        std::thread::park_timeout(Duration::from_millis(150));
        disk.scope = Scope::default();
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        let mut renames = Vec::new();
        let mut rescan = overflow.swap(false, Ordering::Relaxed);
        for result in raw_rx.try_iter().take(1024) {
            match result {
                Ok(event) => {
                    rescan |= event.need_rescan();
                    if matches!(event.kind, EventKind::Access(_)) {
                        continue;
                    }
                    if matches!(
                        event.kind,
                        EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                    ) && event.paths.len() == 2
                    {
                        if let (Some(a), Some(b)) = (
                            relative(root, &event.paths[0]),
                            relative(root, &event.paths[1]),
                        ) {
                            renames.push((a, b));
                        }
                    }
                    for path in event.paths {
                        if let Some(path) = relative(root, &path) {
                            if seen.insert(path.clone()) {
                                paths.push(path);
                            }
                        }
                    }
                }
                Err(_) => rescan = true,
            }
        }
        // Explicit gaps never become a fabricated complete history.
        if rescan {
            lost.store(true, Ordering::Relaxed);
        }
        let ignore_change = paths.iter().any(|p| {
            p.file_name()
                .is_some_and(|n| n == ".gitignore" || n == ".ignore" || n == "exclude")
        });
        if rescan || ignore_change {
            let tree = repository::scan(root)?;
            if tree.truncated || tree.skipped_errors > 0 {
                lost.store(true, Ordering::Relaxed);
            }
            let allowed: BTreeSet<_> = tree
                .entries
                .iter()
                .filter(|e| e.kind == RepoEntryKind::File)
                .map(|e| e.path.clone())
                .collect();
            // Ignore policy changes remove scope, not files on disk.
            disk.known
                .retain(|p, _| allowed.contains(p) || !root.join(p).exists());
            if ignore_change && !rescan {
                // Newly admitted locations were not observed before this policy
                // boundary. Discovery is baseline, not evidence of file creation.
                for p in &allowed {
                    if !disk.known.contains_key(p) {
                        if let Some(stamp) = stamp(&root.join(p)) {
                            disk.known.insert(p.clone(), stamp);
                        }
                    }
                }
            }
            for p in &allowed {
                if seen.insert(p.clone()) {
                    paths.push(p.clone());
                }
            }
            for p in disk.known.keys() {
                if seen.insert(p.clone()) {
                    paths.push(p.clone());
                }
            }
            send(tx, lost, Message::Scope(tree.clone()));
            for e in tree.entries {
                if e.kind == RepoEntryKind::Directory {
                    disk.observe_dir(&mut watcher, root, &e.path)?;
                }
            }
            send(
                tx,
                lost,
                Message::Status(
                    "Watching · scope refreshed; prior activity retained".into(),
                    true,
                ),
            );
        }
        let gone: Vec<_> = disk
            .watched
            .iter()
            .filter(|p| !root.join(p).is_dir())
            .cloned()
            .collect();
        for p in gone {
            disk.watched.remove(&p);
            let _ = watcher.unwatch(&root.join(p));
        }
        let mut renamed = BTreeSet::new();
        for path in paths {
            if renamed.contains(&path) {
                continue;
            }
            if let Some((old, new)) = renames.iter().find(|(a, b)| a == &path || b == &path) {
                if disk.known.contains_key(old)
                    && !root.join(old).exists()
                    && disk.scope.admitted(root, new)
                {
                    if let Some(stamp) = stamp(&root.join(new)) {
                        disk.known.remove(old);
                        disk.known.insert(new.clone(), stamp);
                        let mut event = Event::filesystem(Kind::Renamed, new.clone());
                        event.old_path = Some(old.clone());
                        event.details = Some(format!("Renamed from {}", old.display()));
                        send(tx, lost, Message::Event(event));
                        renamed.insert(old.clone());
                        renamed.insert(new.clone());
                        continue;
                    }
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if root.join(&path).is_dir() && disk.scope.admitted(root, &path) {
                for result in repository::walker(&root.join(&path))
                    .build()
                    .take(repository::DEFAULT_ENTRY_LIMIT + 1)
                    .enumerate()
                {
                    let (index, result) = result;
                    if index == repository::DEFAULT_ENTRY_LIMIT {
                        lost.store(true, Ordering::Relaxed);
                        break;
                    }
                    let e = match result {
                        Ok(e) => e,
                        Err(_) => {
                            lost.store(true, Ordering::Relaxed);
                            continue;
                        }
                    };
                    let Some(p) = relative(root, e.path()) else {
                        continue;
                    };
                    if e.file_type().is_some_and(|t| t.is_dir()) {
                        disk.observe_dir(&mut watcher, root, &p)?;
                    } else {
                        disk.reconcile(root, p, tx, lost);
                    }
                }
            } else if !root.join(&path).exists() && !disk.known.contains_key(&path) {
                let deleted: Vec<_> = disk
                    .known
                    .keys()
                    .filter(|p| p.starts_with(&path))
                    .cloned()
                    .collect();
                for p in deleted {
                    disk.reconcile(root, p, tx, lost);
                }
            } else {
                disk.reconcile(root, path, tx, lost);
            }
        }
        if disk.scope.incomplete {
            lost.store(true, Ordering::Relaxed);
        }
        if let Some(journal) = &mut journal {
            journal.poll(
                root,
                |event| send(tx, lost, Message::Event(event)),
                |error| {
                    send(tx, lost, Message::AgentStatus(error));
                    lost.store(true, Ordering::Relaxed);
                },
            );
        }
    }
    Ok(())
}

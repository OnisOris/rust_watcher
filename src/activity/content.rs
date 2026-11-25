//! Bounded, exact physical-line measurements; only the observation worker reads content.
use super::Kind;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
pub const FILE_LIMIT: usize = 512 * 1024;
pub const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const LINE_LIMIT: usize = 65_536;
const WORK_LIMIT: usize = 250_000;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineDelta {
    pub added: u32,
    pub deleted: u32,
}
impl LineDelta {
    pub fn churn(self) -> u64 {
        u64::from(self.added) + u64::from(self.deleted)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    pub delta: Option<LineDelta>,
    pub net: Option<LineDelta>,
    pub reason: Option<&'static str>,
}
/// Myers O((N+M)D) search with a bounded linear frontier and work/time ceilings.
/// Only edit distance is needed: insertions-deletions = new length-old length.
pub fn diff(before: &str, after: &str) -> Option<LineDelta> {
    if before.len() > FILE_LIMIT || after.len() > FILE_LIMIT {
        return None;
    }
    let a: Vec<_> = before.lines().collect();
    let b: Vec<_> = after.lines().collect();
    if a.len() > LINE_LIMIT || b.len() > LINE_LIMIT {
        return None;
    }
    let (n, m) = (a.len() as isize, b.len() as isize);
    if a.is_empty() || b.is_empty() {
        return Some(LineDelta {
            added: b.len() as u32,
            deleted: a.len() as u32,
        });
    }
    let max = a.len() + b.len();
    let offset = max as isize + 1;
    let mut frontier = vec![0isize; 2 * max + 3];
    let started = Instant::now();
    let mut work = 0usize;
    for d in 0..=max as isize {
        for k in (-d..=d).step_by(2) {
            work += 1;
            if work > WORK_LIMIT
                || (work.is_multiple_of(256) && started.elapsed() > Duration::from_millis(10))
            {
                return None;
            }
            let i = (offset + k) as usize;
            let mut x = if k == -d || (k != d && frontier[i - 1] < frontier[i + 1]) {
                frontier[i + 1]
            } else {
                frontier[i - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
                work += 1;
                if work > WORK_LIMIT
                    || (work.is_multiple_of(256) && started.elapsed() > Duration::from_millis(10))
                {
                    return None;
                }
            }
            frontier[i] = x;
            if x >= n && y >= m {
                return Some(LineDelta {
                    added: ((d + m - n) / 2) as u32,
                    deleted: ((d + n - m) / 2) as u32,
                });
            }
        }
    }
    None
}
#[derive(Default)]
struct Revision {
    initial: Option<Arc<str>>,
    previous: Option<Arc<str>>,
    absent: bool,
}
impl Revision {
    fn bytes(&self) -> usize {
        self.initial.as_ref().map_or(0, |s| s.len())
            + self.previous.as_ref().map_or(0, |s| {
                if self.initial.as_ref().is_some_and(|i| Arc::ptr_eq(i, s)) {
                    0
                } else {
                    s.len()
                }
            })
    }
}
#[derive(Default)]
pub struct Snapshots {
    entries: BTreeMap<PathBuf, Revision>,
    pub bytes: usize,
    pub covered: usize,
    pub total: usize,
}
impl Snapshots {
    fn read(root: &Path, path: &Path) -> Result<Arc<str>, &'static str> {
        if super::relative(root, path).is_none() {
            return Err("Unsafe path");
        }
        let mut file = std::fs::File::open(root.join(path)).map_err(|_| "Unreadable content")?;
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= FILE_LIMIT as u64)
        {
            return Err("File size/type limit");
        }
        let before = file.metadata().map_err(|_| "Unreadable metadata")?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(FILE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Unreadable content")?;
        let after = std::fs::symlink_metadata(root.join(path))
            .map_err(|_| "Content changed during read")?;
        if after.file_type().is_symlink()
            || before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
        {
            return Err("Content changed during read");
        }
        if bytes.len() > FILE_LIMIT {
            return Err("File size limit");
        }
        if bytes.contains(&0) {
            return Err("Binary content");
        }
        let text = String::from_utf8(bytes).map_err(|_| "Non UTF-8 content")?;
        if text.lines().count() > LINE_LIMIT {
            return Err("Line count limit");
        }
        Ok(Arc::from(text))
    }
    pub fn baseline(&mut self, root: &Path, path: &Path) {
        if self.entries.contains_key(path)
            || self.entries.len() >= crate::repository::DEFAULT_ENTRY_LIMIT
        {
            return;
        }
        self.total += 1;
        let content = Self::read(root, path)
            .ok()
            .filter(|s| self.bytes + s.len() <= MEMORY_LIMIT);
        self.covered += usize::from(content.is_some());
        self.bytes += content.as_ref().map_or(0, |s| s.len());
        self.entries.insert(
            path.to_path_buf(),
            Revision {
                initial: content.clone(),
                previous: content,
                absent: false,
            },
        );
    }
    pub fn retain(&mut self, mut admitted: impl FnMut(&Path) -> bool) {
        self.entries.retain(|p, r| {
            if admitted(p) {
                true
            } else {
                self.bytes -= r.bytes();
                false
            }
        });
    }
    pub fn measure(
        &mut self,
        root: &Path,
        path: &Path,
        kind: Kind,
        old_path: Option<&Path>,
    ) -> Measurement {
        let old = old_path.unwrap_or(path);
        let fresh = !self.entries.contains_key(old);
        let mut revision = self.entries.remove(old).unwrap_or_else(|| Revision {
            absent: kind == Kind::Created,
            ..Default::default()
        });
        self.bytes -= revision.bytes();
        if old != path {
            if let Some(replaced) = self.entries.remove(path) {
                self.bytes -= replaced.bytes();
            }
        }
        if fresh && kind == Kind::Created {
            revision.initial = Some(Arc::from(""));
        }
        let read = if kind == Kind::Deleted {
            Ok(Arc::<str>::from(""))
        } else {
            Self::read(root, path)
        };
        let mut reason = read.as_ref().err().copied();
        let mut next = read.ok();
        if let (Some(initial), Some(current)) = (&revision.initial, &next) {
            if initial == current {
                next = Some(initial.clone());
            }
        }
        let delta = if revision.absent {
            next.as_ref().map(|s| LineDelta {
                added: s.lines().count() as u32,
                deleted: 0,
            })
        } else {
            revision
                .previous
                .as_ref()
                .zip(next.as_ref())
                .and_then(|(a, b)| diff(a, b))
        };
        let net = revision
            .initial
            .as_ref()
            .zip(next.as_ref())
            .and_then(|(a, b)| diff(a, b));
        if delta.is_none() {
            reason = reason.or(Some("Missing baseline or diff work limit"));
        }
        revision.previous = next;
        revision.absent = kind == Kind::Deleted;
        if self.bytes + revision.bytes() > MEMORY_LIMIT {
            revision.previous = None;
            reason = Some("Snapshot memory limit");
        }
        self.bytes += revision.bytes();
        if self.entries.len() < crate::repository::DEFAULT_ENTRY_LIMIT {
            self.entries.insert(path.to_path_buf(), revision);
        } else {
            self.bytes -= revision.bytes();
        }
        Measurement { delta, net, reason }
    }
}

#[cfg(test)]
mod tests;

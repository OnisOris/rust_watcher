//! Explicit adapter contract, not a parser for native Codex/OpenClaw logs.
use super::{relative, Event, Kind, Source};
use serde::Deserialize;
use std::collections::{HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};
const RECORD_LIMIT: usize = 16 * 1024;
const POLL_LIMIT: u64 = 64 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    session: String,
    source: String,
    event: String,
    path: Option<PathBuf>,
    timestamp: String,
}
pub struct Tail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
    discarding: bool,
    seen: HashSet<Vec<u8>>,
    order: VecDeque<Vec<u8>>,
    identity: Option<u64>,
}
impl Tail {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: Vec::new(),
            discarding: false,
            seen: HashSet::new(),
            order: VecDeque::new(),
            identity: None,
        }
    }
    pub fn poll(
        &mut self,
        root: &Path,
        mut event: impl FnMut(Event),
        mut error: impl FnMut(String),
    ) {
        let result = (|| -> std::io::Result<Vec<u8>> {
            let mut file = std::fs::File::open(&self.path)?;
            let meta = file.metadata()?;
            #[cfg(unix)]
            let identity = {
                use std::os::unix::fs::MetadataExt;
                meta.ino()
            };
            #[cfg(not(unix))]
            let identity = 0;
            if meta.len() < self.offset || self.identity.is_some_and(|id| id != identity) {
                self.offset = 0;
                self.partial.clear();
                self.discarding = false;
                error("Agent stream rotated/truncated · Activity incomplete".into());
            }
            self.identity = Some(identity);
            file.seek(SeekFrom::Start(self.offset))?;
            let mut bytes = Vec::new();
            file.take(POLL_LIMIT).read_to_end(&mut bytes)?;
            self.offset += bytes.len() as u64;
            Ok(bytes)
        })();
        match result {
            Ok(bytes) => self.feed(root, &bytes, &mut event, &mut error),
            Err(e) => error(format!("Agent stream unavailable: {e}")),
        }
    }
    pub fn feed(
        &mut self,
        root: &Path,
        bytes: &[u8],
        mut event: impl FnMut(Event),
        mut error: impl FnMut(String),
    ) {
        for &byte in bytes {
            if byte == b'\n' {
                if self.discarding {
                    self.discarding = false;
                    self.partial.clear();
                    continue;
                }
                let line = std::mem::take(&mut self.partial);
                if line.is_empty() {
                    continue;
                }
                match parse(root, &line) {
                    Some((e, key)) => {
                        if self.seen.contains(&key) {
                            continue;
                        }
                        if self.order.len() == super::HISTORY_LIMIT {
                            if let Some(old) = self.order.pop_front() {
                                self.seen.remove(&old);
                            }
                        }
                        self.seen.insert(key.clone());
                        self.order.push_back(key);
                        event(e);
                    }
                    None => error("Invalid/out-of-scope agent record · Activity incomplete".into()),
                }
            } else if !self.discarding {
                if self.partial.len() == RECORD_LIMIT {
                    self.partial.clear();
                    self.discarding = true;
                    error("Oversized agent record · Activity incomplete".into());
                } else {
                    self.partial.push(byte);
                }
            }
        }
    }
}
fn parse(root: &Path, line: &[u8]) -> Option<(Event, Vec<u8>)> {
    let r: Record = serde_json::from_slice(line).ok()?;
    if r.version != 1
        || r.session.is_empty()
        || r.session.len() > 128
        || r.source.is_empty()
        || r.source.len() > 128
        || !timestamp(&r.timestamp)
    {
        return None;
    }
    let kind = match r.event.as_str() {
        "read" => Kind::Read,
        "write" => Kind::Write,
        "search" => Kind::Search,
        "command" | "test" => Kind::Command,
        _ => return None,
    };
    let path = match r.path {
        Some(p) => {
            let p = relative(root, &p)?;
            if !super::watcher::admitted(root, &p) {
                return None;
            }
            Some(p)
        }
        None if matches!(kind, Kind::Read | Kind::Write) => return None,
        None => None,
    };
    let key = serde_json::to_vec(&(&r.session, &r.source, &r.event, &path, &r.timestamp)).ok()?;
    Some((
        Event {
            at: SystemTime::now(),
            observed: Instant::now(),
            kind,
            source: Source::Agent,
            path,
            old_path: None,
            details: Some(format!(
                "{} · {} · {} · reported {}",
                r.source, r.session, r.event, r.timestamp
            )),
        },
        key,
    ))
}
// v1 accepts a deliberately small RFC3339 subset: UTC, whole seconds.
fn timestamp(s: &str) -> bool {
    if s.len() != 20 {
        return false;
    }
    let b = s.as_bytes();
    if [4, 7, 10, 13, 16, 19]
        .iter()
        .zip(b"--T::Z")
        .any(|(&i, &c)| b[i] != c)
    {
        return false;
    }
    if b.iter()
        .enumerate()
        .any(|(i, c)| ![4, 7, 10, 13, 16, 19].contains(&i) && !c.is_ascii_digit())
    {
        return false;
    }
    let n = |a, b| s[a..b].parse::<u32>().unwrap_or(9999);
    let (y, m, d) = (n(0, 4), n(5, 7), n(8, 10));
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    y > 0 && d > 0 && d <= days && n(11, 13) < 24 && n(14, 16) < 60 && n(17, 19) < 60
}

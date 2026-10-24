use super::{Change, History, Options, MAX_COCHANGE_FILES};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

type Delta = (Option<u64>, Option<u64>);

pub(super) fn parse(
    bytes: &[u8],
    prefix: &Path,
    since: i64,
    options: &Options,
    complete: bool,
    history: &mut History,
) -> Result<(), String> {
    if !complete {
        // Only commit records terminated by the next complete marker are safe.
        // Never accept the final, potentially cut-off commit as measured history.
        let marker = b"\0WT-COMMIT\0";
        let end = bytes
            .windows(marker.len())
            .rposition(|w| w == marker)
            .unwrap_or(0);
        return parse(&bytes[..end], prefix, since, options, true, history);
    }
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err("Unterminated Git output".into());
    }
    let mut tokens = bytes.split(|b| *b == 0);
    let mut current: Option<(String, i64, BTreeMap<PathBuf, Delta>)> = None;
    let mut seen = BTreeSet::new();
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            continue;
        }
        if token == b"WT-COMMIT" {
            if let Some((sha, date, files)) = current.take() {
                if !flush(sha, date, files, &mut seen, options, history) {
                    return Ok(());
                }
            }
            let Some(sha) = tokens.next() else {
                return Err("Missing commit identity".into());
            };
            let Some(date) = tokens.next() else {
                return Err("Missing commit date".into());
            };
            if !(sha.len() == 40 || sha.len() == 64) || !sha.iter().all(u8::is_ascii_hexdigit) {
                return Err("Invalid commit identity".into());
            }
            let date = std::str::from_utf8(date)
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or("Invalid commit timestamp")?;
            if date < since || date > options.as_of {
                return Err("Commit outside fixed analysis window".into());
            }
            current = Some((
                String::from_utf8(sha.to_vec()).unwrap(),
                date,
                BTreeMap::new(),
            ));
        } else {
            // -z protects arbitrary tabs/newlines in paths. Only the first two tabs
            // are numeric separators, never split the path itself into lines.
            let token = token.strip_prefix(b"\n").unwrap_or(token);
            let mut columns = token.splitn(3, |b| *b == b'\t');
            let a = columns.next().ok_or("Missing numstat")?;
            let d = columns.next().ok_or("Missing numstat deletion")?;
            let raw = columns.next().ok_or("Missing numstat path")?;
            let number = |s: &[u8]| -> Result<Option<u64>, String> {
                if s == b"-" {
                    Ok(None)
                } else {
                    std::str::from_utf8(s)
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .map(Some)
                        .ok_or("Invalid numstat number".into())
                }
            };
            let delta = (number(a)?, number(d)?);
            let Some((_, _, files)) = current.as_mut() else {
                return Err("Numstat without commit".into());
            };
            let Ok(path) = std::str::from_utf8(raw) else {
                history.partial("nonUtf8Path");
                continue;
            };
            let path = Path::new(path);
            if path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err("Unsafe Git path".into());
            }
            let Ok(relative) = path.strip_prefix(prefix) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            if files.insert(relative.to_path_buf(), delta).is_some() {
                history.partial("duplicateFileRecord");
            }
            if files.len() > options.max_files {
                history.partial("changedFileBudget");
                return Ok(());
            }
        }
    }
    if let Some((sha, date, files)) = current {
        flush(sha, date, files, &mut seen, options, history);
    }
    Ok(())
}

fn flush(
    sha: String,
    date: i64,
    files: BTreeMap<PathBuf, Delta>,
    seen: &mut BTreeSet<String>,
    options: &Options,
    history: &mut History,
) -> bool {
    if !seen.insert(sha) {
        history.partial("duplicateCommitRecord");
        return true;
    }
    if history.commits_analyzed == options.max_commits {
        history.partial("commitBudget");
        return false;
    }
    if history.files.len()
        + files
            .keys()
            .filter(|p| !history.files.contains_key(*p))
            .count()
        > options.max_files
    {
        history.partial("changedFileBudget");
        return false;
    }
    history.commits_analyzed += 1;
    if files.len() <= MAX_COCHANGE_FILES {
        history.cochanges.push(files.keys().cloned().collect());
    } else {
        history.partial("coChangeCommitFileBudget");
    }
    for (path, (added, deleted)) in files {
        let item = history.files.entry(path).or_insert(Change {
            added: Some(0),
            deleted: Some(0),
            last: date,
            ..Change::default()
        });
        item.commits += 1;
        item.added = item.added.zip(added).and_then(|(a, b)| a.checked_add(b));
        item.deleted = item
            .deleted
            .zip(deleted)
            .and_then(|(a, b)| a.checked_add(b));
        item.last = item.last.max(date);
    }
    true
}

//! Bounded local, committed file history. Independent of Rust semantics and RA.
mod correlation;
mod history;
mod process;
#[cfg(test)]
mod tests;
mod time;

pub use correlation::print;
#[cfg(test)]
pub use correlation::CoChanged;
pub use correlation::{correlate, FileHotspot, HotspotReport};
pub use process::GitRunner;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_DAYS: u16 = 90;
pub const MAX_COMMITS: usize = 1000;
pub const MAX_CHANGED_FILES: usize = 10_000;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_COCHANGE_FILES: usize = 100;
pub const RANKING_LIMIT: usize = 10;

pub struct Options {
    pub days: u16,
    pub as_of: i64,
    pub max_commits: usize,
    pub max_files: usize,
    pub max_output: usize,
    pub timeout: Duration,
    pub binary: OsString,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            days: DEFAULT_DAYS,
            as_of: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            max_commits: MAX_COMMITS,
            max_files: MAX_CHANGED_FILES,
            max_output: MAX_OUTPUT_BYTES,
            timeout: Duration::from_secs(10),
            binary: "git".into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unavailable {
    pub code: String,
    pub message: String,
}

#[derive(Default)]
pub struct History {
    pub since_days: u16,
    pub as_of_utc: String,
    pub head_sha: Option<String>,
    pub reason: Option<Unavailable>,
    pub history_shallow: Option<bool>,
    pub working_tree_dirty: Option<bool>,
    pub commits_analyzed: usize,
    pub partial_reasons: Vec<String>,
    pub files: BTreeMap<PathBuf, Change>,
    cochanges: Vec<Vec<PathBuf>>,
}
#[derive(Debug, Default)]
pub struct Change {
    pub commits: usize,
    pub added: Option<u64>,
    pub deleted: Option<u64>,
    pub last: i64,
}
impl History {
    fn partial(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        if !self.partial_reasons.contains(&reason) {
            self.partial_reasons.push(reason);
        }
    }
}

struct Session<'a> {
    root: &'a Path,
    options: &'a Options,
    runner: &'a GitRunner,
    remaining: usize,
    deadline: Instant,
}
impl Session<'_> {
    fn run(&mut self, args: Vec<OsString>) -> Result<process::Output, String> {
        let output = self.runner.run(
            &self.options.binary,
            self.root,
            &args,
            self.remaining,
            self.deadline,
        )?;
        self.remaining = self.remaining.saturating_sub(output.bytes.len());
        Ok(output)
    }
    fn checked(&mut self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = self.run(args.iter().map(OsString::from).collect())?;
        if let Some(reason) = out.stopped {
            return Err(reason.into());
        }
        if !out.success {
            return Err(if out.stderr.is_empty() {
                format!("Git command failed: {}", args[0])
            } else {
                out.stderr
            });
        }
        Ok(out.bytes)
    }
}

pub fn analyze(root: &Path, options: &Options, runner: &GitRunner) -> History {
    let mut history = History {
        since_days: options.days,
        as_of_utc: time::utc(options.as_of),
        ..History::default()
    };
    if let Err(message) = collect(root, options, runner, &mut history) {
        if matches!(message.as_str(), "outputBudget" | "timeout" | "cancelled") {
            history.partial(message.clone());
        }
        let code = if message == "Git executable not found" {
            "gitNotFound"
        } else if message == "No committed history" {
            "noCommittedHistory"
        } else if message == "Not a Git worktree" {
            "notWorktree"
        } else if message == "cancelled" {
            "cancelled"
        } else {
            "gitError"
        };
        history.reason = Some(Unavailable {
            code: code.into(),
            message,
        });
    }
    history
}

fn collect(
    root: &Path,
    options: &Options,
    runner: &GitRunner,
    history: &mut History,
) -> Result<(), String> {
    let mut session = Session {
        root,
        options,
        runner,
        remaining: options.max_output,
        deadline: Instant::now() + options.timeout,
    };
    let inside = session.run(vec!["rev-parse".into(), "--is-inside-work-tree".into()])?;
    if let Some(reason) = inside.stopped {
        return Err(reason.into());
    }
    if !inside.success || inside.bytes != b"true\n" {
        return Err("Not a Git worktree".into());
    }
    let git_root = session.checked(&["rev-parse", "--show-toplevel"])?;
    let git_root = std::str::from_utf8(git_root.strip_suffix(b"\n").unwrap_or(&git_root))
        .map_err(|_| "Git root is not UTF-8")?;
    let git_root = Path::new(git_root)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let workspace = root.canonicalize().map_err(|e| e.to_string())?;
    let prefix = workspace
        .strip_prefix(&git_root)
        .map_err(|_| "Workspace outside Git root")?;
    let pathspec = if prefix.as_os_str().is_empty() {
        ":(top,literal)".to_owned()
    } else {
        format!(
            ":(top,literal){}/",
            prefix
                .iter()
                .map(|c| c.to_str().ok_or("Workspace path is not UTF-8"))
                .collect::<Result<Vec<_>, _>>()?
                .join("/")
        )
    };
    history.history_shallow = Some(
        match session
            .checked(&["rev-parse", "--is-shallow-repository"])?
            .as_slice()
        {
            b"true\n" => true,
            b"false\n" => false,
            _ => return Err("Invalid shallow repository status".into()),
        },
    );
    if history.history_shallow == Some(true) {
        history.partial("shallowHistory");
    }
    let dirty = session.checked(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=normal",
        "--ignore-submodules=all",
        "--",
        &pathspec,
    ])?;
    history.working_tree_dirty = Some(!dirty.is_empty());
    let head = session.run(vec![
        "rev-parse".into(),
        "--verify".into(),
        "--quiet".into(),
        "HEAD^{commit}".into(),
    ])?;
    if let Some(reason) = head.stopped {
        return Err(reason.into());
    }
    if !head.success {
        let symbolic = session.checked(&["symbolic-ref", "HEAD"])?;
        let name = std::str::from_utf8(&symbolic)
            .map_err(|_| "Invalid HEAD")?
            .trim_end_matches('\n');
        let reference = session.run(vec![
            "show-ref".into(),
            "--verify".into(),
            "--quiet".into(),
            name.into(),
        ])?;
        if reference.stopped.is_some() {
            return Err("Unable to inspect HEAD".into());
        }
        if !reference.success && reference.stderr.is_empty() {
            return Err("No committed history".into());
        }
        return Err("Unable to resolve HEAD commit".into());
    }
    let sha = std::str::from_utf8(&head.bytes)
        .map_err(|_| "Invalid HEAD")?
        .trim()
        .to_owned();
    history.head_sha = Some(sha.clone());
    let since = options
        .as_of
        .saturating_sub(i64::from(options.days) * 86400);
    let args = vec![
        "log".into(),
        "--full-history".into(),
        "--no-merges".into(),
        "--no-renames".into(),
        "--no-ext-diff".into(),
        "--no-textconv".into(),
        "--no-notes".into(),
        "--no-show-signature".into(),
        "--no-color".into(),
        "--numstat".into(),
        "-z".into(),
        "--format=%x00WT-COMMIT%x00%H%x00%ct%x00".into(),
        format!("--since-as-filter=@{since}").into(),
        format!("--until=@{}", options.as_of).into(),
        format!("--max-count={}", options.max_commits.saturating_add(1)).into(),
        sha.into(),
        "--".into(),
        pathspec.into(),
    ];
    let output = session.run(args)?;
    if !output.success && output.stopped.is_none() {
        return Err(format!("Git log: {}", output.stderr));
    }
    history::parse(
        &output.bytes,
        prefix,
        since,
        options,
        output.stopped.is_none(),
        history,
    )?;
    if let Some(reason) = output.stopped {
        history.partial(reason);
    }
    Ok(())
}

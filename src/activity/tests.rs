use super::*;
use std::fs;
use watcher::{Message, Observation};
fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn event(kind: Kind, path: &str) -> Event {
    Event::filesystem(kind, PathBuf::from(path))
}
fn snapshot(root: &Path) -> State {
    let mut state = State::default();
    state.snapshot(crate::repository::scan(root).unwrap());
    state
}
#[test]
fn session_baseline_is_not_activity_and_repeated_paths_are_unique() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "src/a.rs", "pre-existing dirty content");
    let mut s = snapshot(t.path());
    assert!(s.touched.is_empty());
    assert!(s.events.is_empty());
    for _ in 0..3 {
        s.apply(event(Kind::Modified, "src/a.rs"));
    }
    assert_eq!(s.touched.len(), 1);
    assert_eq!(s.total_events, 3);
    assert_eq!(s.directories(Filter::Touched)[Path::new("src")], 1);
    assert_eq!(s.touched[Path::new("src/a.rs")].agent, None);
}
#[test]
fn nested_directories_deduplicate_files_and_separate_provenance() {
    let mut s = State::default();
    for p in ["src/tui/a.rs", "src/tui/b.rs", "src/tui/map/c.rs"] {
        s.apply(event(Kind::Modified, p));
    }
    let mut e = event(Kind::Read, "src/tui/a.rs");
    e.source = Source::Agent;
    s.apply(e);
    let c = s.directories(Filter::Touched);
    assert_eq!(c[Path::new("src")], 3);
    assert_eq!(c[Path::new("src/tui")], 3);
    assert_eq!(c[Path::new("src/tui/map")], 1);
    assert_eq!(s.agent_events, 1);
    assert_eq!(
        s.touched[Path::new("src/tui/a.rs")].filesystem,
        Some(Kind::Modified)
    );
}
#[test]
fn history_and_file_budgets_are_explicit() {
    let mut s = State::default();
    for _ in 0..=HISTORY_LIMIT {
        s.apply(event(Kind::Modified, "one"));
    }
    assert_eq!(s.events.len(), HISTORY_LIMIT);
    assert!(s.incomplete);
    let mut s = State::default();
    for i in 0..=TOUCHED_LIMIT {
        s.apply(event(Kind::Created, &format!("{i}")));
    }
    assert_eq!(s.touched.len(), TOUCHED_LIMIT);
    assert!(s.incomplete);
}
#[test]
fn unicode_deleted_file_and_recent_expiry() {
    let mut s = State::default();
    s.apply(event(Kind::Created, "src/привет.rs"));
    s.apply(event(Kind::Deleted, "src/привет.rs"));
    assert!(!s.files[Path::new("src/привет.rs")]);
    assert_eq!(s.touched.len(), 1);
    s.touched.get_mut(Path::new("src/привет.rs")).unwrap().last =
        Instant::now() - Duration::from_secs(61);
    assert!(!s.matches(Path::new("src/привет.rs"), Filter::Recent));
    assert!(s.matches(Path::new("src/привет.rs"), Filter::Modified));
}
#[test]
fn traversal_absolute_outside_and_ignored_scope() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), ".gitignore", "ignored/\nsecret\n");
    write(t.path(), "ignored/a", "a");
    write(t.path(), "target/a", "a");
    write(t.path(), "src/ok.rs", "a");
    assert!(relative(t.path(), Path::new("../outside")).is_none());
    assert!(relative(t.path(), Path::new("/outside")).is_none());
    assert!(watcher::admitted(t.path(), Path::new("src/ok.rs")));
    assert!(!watcher::admitted(t.path(), Path::new("target/a")));
    assert!(!watcher::admitted(t.path(), Path::new("ignored/a")));
}
#[cfg(unix)]
#[test]
fn never_follows_symlinks_even_when_deleted_or_within_workspace() {
    let t = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "a.rs", "secret");
    std::os::unix::fs::symlink(outside.path(), t.path().join("link")).unwrap();
    assert!(relative(t.path(), Path::new("link/a.rs")).is_none());
    assert!(!watcher::admitted(t.path(), Path::new("link/a.rs")));
}
fn record(path: &str) -> String {
    format!(
        r#"{{"version":1,"session":"test","source":"adapter","event":"read","path":"{path}","timestamp":"2026-11-25T10:30:00Z"}}"#
    )
}
#[test]
fn jsonl_partial_duplicate_invalid_oversize_and_provenance() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "src/a.rs", "a");
    let mut tail = stream::Tail::new(PathBuf::new());
    let line = record("src/a.rs");
    let mut events = Vec::new();
    let mut errors = Vec::new();
    tail.feed(
        t.path(),
        &line.as_bytes()[..20],
        |e| events.push(e),
        |e| errors.push(e),
    );
    assert!(events.is_empty());
    tail.feed(
        t.path(),
        format!("{}\n{line}\n", &line[20..]).as_bytes(),
        |e| events.push(e),
        |e| errors.push(e),
    );
    let formatted =
        serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(&line).unwrap())
            .unwrap()
            .replace('\n', " ")
            + "\n";
    tail.feed(
        t.path(),
        formatted.as_bytes(),
        |e| events.push(e),
        |e| errors.push(e),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, Source::Agent);
    assert_eq!(events[0].kind, Kind::Read);
    let bad = format!(
        "not json\n{}\n{}\n{}\n{}\n",
        record("../secret"),
        record("src/a.rs").replace("\"version\":1", "\"version\":2"),
        record("src/a.rs").replace("2026-11-25", "2026-02-30"),
        "x".repeat(20_000)
    );
    tail.feed(
        t.path(),
        bad.as_bytes(),
        |e| events.push(e),
        |e| errors.push(e),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(errors.len(), 5);
}
fn receive(o: &Observation, state: &mut State, until: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !until(state) && Instant::now() < deadline {
        match o.rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Message::Snapshot(tree)) => state.snapshot(tree),
            Ok(Message::Scope(tree)) => state.refresh_scope(tree),
            Ok(Message::Event(e)) => state.apply(e),
            Ok(Message::Status(message, false)) => panic!("{message}"),
            _ => {}
        }
    }
    assert!(
        until(state),
        "watcher did not produce expected state; {} events",
        state.total_events
    );
}
#[test]
fn real_workspace_external_process_create_modify_rename_delete_and_shutdown() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "Cargo.toml", "[workspace]\n");
    write(t.path(), "src/b.rs", "before");
    write(t.path(), "other/c.rs", "before");
    let o = Observation::start(t.path().to_path_buf(), None).unwrap();
    let mut s = State::default();
    receive(&o, &mut s, |s| s.watching);
    assert!(s.events.is_empty());
    // This producer is an external process, not the read-only UI.
    #[cfg(unix)]
    {
        let result = std::process::Command::new("sh")
            .current_dir(t.path())
            .args([
                "-c",
                "printf new > src/a.rs; printf changed > src/b.rs; printf changed > other/c.rs",
            ])
            .status()
            .unwrap();
        assert!(result.success());
    }
    #[cfg(not(unix))]
    {
        write(t.path(), "src/a.rs", "new");
        write(t.path(), "src/b.rs", "changed");
        write(t.path(), "other/c.rs", "changed");
    }
    receive(&o, &mut s, |s| s.touched.len() == 3);
    for event in &s.events {
        let measured = event.measurement.unwrap().delta.unwrap();
        assert_eq!(
            measured.churn(),
            if event.kind == Kind::Created { 1 } else { 2 }
        );
    }
    fs::rename(t.path().join("src/a.rs"), t.path().join("src/renamed.rs")).unwrap();
    fs::remove_file(t.path().join("other/c.rs")).unwrap();
    receive(&o, &mut s, |s| {
        s.files.get(Path::new("other/c.rs")) == Some(&false)
            && s.files.contains_key(Path::new("src/renamed.rs"))
    });
    assert!(s.events.iter().any(|e| e.kind == Kind::Renamed
        || e.kind == Kind::Created && e.path.as_deref() == Some(Path::new("src/renamed.rs"))));
    assert!(s.events.iter().all(|e| e.source == Source::Filesystem));
    drop(o);
}
#[test]
fn ignore_changes_atomic_replacement_directory_creation_and_burst() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), ".gitignore", "ignored/\n");
    write(t.path(), "src/a.rs", "old");
    write(t.path(), "ignored/x", "x");
    let o = Observation::start(t.path().to_path_buf(), None).unwrap();
    let mut s = State::default();
    receive(&o, &mut s, |s| s.watching);
    assert!(!s.files.contains_key(Path::new("ignored/x")));
    write(t.path(), "src/tmp", "new");
    fs::rename(t.path().join("src/tmp"), t.path().join("src/a.rs")).unwrap();
    for i in 0..40 {
        write(t.path(), &format!("new/sub/{i}.rs"), "new");
    }
    receive(&o, &mut s, |s| {
        s.touched.contains_key(Path::new("src/a.rs"))
            && s.files.contains_key(Path::new("new/sub/39.rs"))
    });
    write(t.path(), ".gitignore", "");
    receive(&o, &mut s, |s| s.files.contains_key(Path::new("ignored/x")));
    assert_eq!(s.touched[Path::new("src/a.rs")].events, 1);
    assert!(
        !s.touched.contains_key(Path::new("ignored/x")),
        "scope discovery is not file creation"
    );
}
#[test]
fn bounded_delivery_exposes_queue_overflow() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "a", "old");
    let o = Observation::start(t.path().to_path_buf(), None).unwrap();
    let mut s = State::default();
    receive(&o, &mut s, |s| s.watching);
    for i in 0..400 {
        write(t.path(), &format!("file{i}"), "new");
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !o.lost.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(o.lost.load(std::sync::atomic::Ordering::Relaxed));
    drop(o);
}

#[test]
fn stream_file_partial_append_rotation_and_non_executable_commands() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "src/модуль.rs", "x");
    let journal = t.path().join("journal.jsonl");
    let line = record("src/модуль.rs");
    fs::write(&journal, &line).unwrap();
    let mut tail = stream::Tail::new(journal.clone());
    let mut events = Vec::new();
    let mut errors = Vec::new();
    tail.poll(t.path(), |e| events.push(e), |e| errors.push(e));
    assert!(events.is_empty());
    fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    tail.poll(t.path(), |e| events.push(e), |e| errors.push(e));
    assert_eq!(events.len(), 1);
    fs::write(&journal,"{\"version\":1,\"session\":\"s\",\"source\":\"x\",\"event\":\"command\",\"timestamp\":\"2026-11-25T10:30:00Z\"}\n").unwrap();
    tail.poll(t.path(), |e| events.push(e), |e| errors.push(e));
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].kind, Kind::Command);
    assert!(!errors.is_empty());
}
#[test]
fn stopping_watcher_disconnects_delivery_and_drops_thread() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "file", "x");
    let mut o = Observation::start(t.path().to_path_buf(), None).unwrap();
    let mut s = State::default();
    receive(&o, &mut s, |s| s.watching);
    let (_tx, dummy) = std::sync::mpsc::sync_channel(1);
    let rx = std::mem::replace(&mut o.rx, dummy);
    drop(o);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(1)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
}
#[test]
fn created_and_deleted_summary_facts_survive_later_modifications() {
    let mut s = State::default();
    s.apply(event(Kind::Created, "a.rs"));
    s.apply(event(Kind::Modified, "a.rs"));
    assert!(s.touched[Path::new("a.rs")].created);
    s.apply(event(Kind::Deleted, "a.rs"));
    s.apply(event(Kind::Created, "a.rs"));
    assert!(s.touched[Path::new("a.rs")].deleted);
    assert_eq!(s.touched.len(), 1);
}
#[test]
fn correlated_rename_moves_file_identity_without_double_counting() {
    let mut s = State::default();
    s.apply(event(Kind::Modified, "src/old.rs"));
    let mut renamed = event(Kind::Renamed, "src/new.rs");
    renamed.old_path = Some("src/old.rs".into());
    s.apply(renamed);
    assert_eq!(s.touched.len(), 1);
    assert_eq!(s.directories(Filter::Touched)[Path::new("src")], 1);
    assert!(!s.files.contains_key(Path::new("src/old.rs")));
    assert_eq!(s.touched[Path::new("src/new.rs")].events, 2);
    assert_eq!(
        s.events.front().unwrap().path.as_deref(),
        Some(Path::new("src/old.rs"))
    );
}

#[test]
fn previously_observed_staging_file_replaces_destination_without_losing_its_baseline() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "src/destination.rs", "old\nkeep\n");
    let o = Observation::start(t.path().to_path_buf(), None).unwrap();
    let mut s = State::default();
    receive(&o, &mut s, |s| s.watching);
    write(t.path(), "src/staged.rs", "new\nkeep\n");
    receive(&o, &mut s, |s| {
        s.touched.contains_key(Path::new("src/staged.rs"))
    });
    fs::rename(
        t.path().join("src/staged.rs"),
        t.path().join("src/destination.rs"),
    )
    .unwrap();
    receive(&o, &mut s, |s| {
        s.touched.contains_key(Path::new("src/destination.rs"))
            && s.files.get(Path::new("src/staged.rs")) == Some(&false)
    });
    let e = s
        .events
        .iter()
        .find(|e| e.path.as_deref() == Some(Path::new("src/destination.rs")))
        .unwrap();
    assert_eq!(e.kind, Kind::Modified);
    let m = e.measurement.unwrap();
    assert_eq!(
        m.delta.unwrap(),
        content::LineDelta {
            added: 1,
            deleted: 1
        }
    );
    assert_eq!(m.net.unwrap().churn(), 2);
    write(t.path(), "src/destination.rs", "old\nkeep\n");
    receive(&o, &mut s, |s| {
        s.events
            .iter()
            .filter(|e| e.path.as_deref() == Some(Path::new("src/destination.rs")))
            .count()
            == 2
    });
    assert_eq!(
        s.events
            .back()
            .unwrap()
            .measurement
            .unwrap()
            .net
            .unwrap()
            .churn(),
        0
    );
}

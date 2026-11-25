use super::focus::*;
use super::*;
fn data() -> State {
    let mut s = State::default();
    for p in ["src/a.rs", "src/a.rs", "src/b.rs", "tests/test.rs"] {
        s.apply(Event::filesystem(Kind::Modified, p.into()));
    }
    let mut e = Event::filesystem(Kind::Read, "docs/readme.md".into());
    e.source = Source::Agent;
    s.apply(e);
    s
}
#[test]
fn heat_decays_without_erasing_facts_and_mixed_sources_remain_distinct() {
    let s = data();
    let now = Instant::now();
    let hot = Snapshot::build(&s, Query::default(), now);
    assert_eq!(hot.hottest().unwrap(), Path::new("src/a.rs"));
    assert_eq!(hot.directories[Path::new("src")].files, 2);
    assert_eq!(hot.mix, [4, 0, 0, 0, 1]);
    let cold = Snapshot::build(&s, Query::default(), now + Duration::from_secs(121));
    assert_eq!(cold.files.len(), 4);
    assert!(cold.files.values().all(|f| f.score == 0));
    assert_eq!(heat(Duration::from_secs(0)), 8);
    assert_eq!(heat(Duration::from_secs(10)), 5);
    assert_eq!(heat(Duration::from_secs(20)), 2);
    assert_eq!(heat(Duration::from_secs(80)), 1);
    assert_eq!(heat(Duration::from_secs(121)), 0);
}
#[test]
fn provenance_type_scope_and_time_filters_share_one_projection() {
    let s = data();
    let now = Instant::now();
    let agent = Snapshot::build(
        &s,
        Query {
            sources: Sources::Agent,
            ..Query::default()
        },
        now,
    );
    assert_eq!(agent.mix, [0, 0, 0, 0, 1]);
    assert_eq!(agent.files.len(), 1);
    let src = Snapshot::build(
        &s,
        Query {
            scope: Scope::Src,
            ..Query::default()
        },
        now,
    );
    assert_eq!(src.files.len(), 2);
    assert_eq!(src.events.len(), 3);
    let empty = Snapshot::build(
        &s,
        Query {
            types: Types::CreatedDeleted,
            ..Query::default()
        },
        now,
    );
    assert!(empty.events.is_empty());
    let expired = Snapshot::build(
        &s,
        Query {
            window: Window::Minute,
            ..Query::default()
        },
        now + Duration::from_secs(61),
    );
    assert!(expired.files.is_empty());
    assert_eq!(expired.mix, [0; 5]);
}
#[test]
fn rename_transfers_unique_identity_and_command_has_no_invented_path() {
    let mut s = State::default();
    s.apply(Event::filesystem(Kind::Created, "src/a.rs".into()));
    let mut renamed = Event::filesystem(Kind::Renamed, "tests/a.rs".into());
    renamed.old_path = Some("src/a.rs".into());
    s.apply(renamed);
    let mut command = Event::filesystem(Kind::Command, "".into());
    command.source = Source::Agent;
    command.path = None;
    s.apply(command);
    let snapshot = Snapshot::build(&s, Query::default(), Instant::now());
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[Path::new("tests/a.rs")].events, 2);
    assert!(!snapshot.directories.contains_key(Path::new("src")));
    assert_eq!(snapshot.mix, [0, 1, 0, 1, 1]);
    assert_eq!(snapshot.events.len(), 3);
}
#[test]
fn concentration_uses_observation_distribution_not_agent_productivity() {
    let mut s = State::default();
    for p in ["src/a", "src/b", "src/c"] {
        s.apply(Event::filesystem(Kind::Modified, p.into()));
    }
    assert!(Snapshot::build(&s, Query::default(), Instant::now())
        .stability
        .starts_with("Concentrated"));
    for p in ["tests/a", "docs/a", "config/a"] {
        s.apply(Event::filesystem(Kind::Modified, p.into()));
    }
    assert!(Snapshot::build(&s, Query::default(), Instant::now())
        .stability
        .starts_with("Distributed"));
}
#[test]
fn an_old_write_storm_cannot_permanently_outshine_a_fresh_signal() {
    let mut s = State::default();
    let old = Instant::now() - Duration::from_secs(90);
    for _ in 0..50 {
        let mut e = Event::filesystem(Kind::Modified, "old.rs".into());
        e.observed = old;
        s.apply(e);
    }
    s.apply(Event::filesystem(Kind::Modified, "now.rs".into()));
    assert_eq!(
        Snapshot::build(&s, Query::default(), Instant::now())
            .hottest()
            .unwrap(),
        Path::new("now.rs")
    );
}

use super::{
    focus::*,
    heat::{Mode, *},
    *,
};
fn measured_event(path: &str, added: u32, deleted: u32, at: Instant) -> Event {
    let mut e = Event::filesystem(Kind::Modified, path.into());
    e.observed = at;
    e.measurement = Some(content::Measurement {
        delta: Some(content::LineDelta { added, deleted }),
        net: None,
        reason: None,
    });
    e
}
#[test]
fn absolute_scale_is_monotone_and_small_edits_stay_cool() {
    let mut prior = 0;
    for churn in 0..10_000 {
        let l = level(churn as f64);
        assert!(l >= prior);
        prior = l;
    }
    assert_eq!(
        (
            level(0.),
            level(2.),
            level(8.),
            level(32.),
            level(96.),
            level(256.)
        ),
        (0, 1, 2, 3, 4, 5)
    );
    assert!(level(100.) > level(10.));
    assert!(level(180.) > level(52.));
    assert!(level(52.) > level(18.));
    assert_eq!(weighted(180, Duration::from_secs(45), Mode::Live), 90.);
    assert_eq!(weighted(180, Duration::from_secs(120), Mode::Live), 0.);
    assert_eq!(weighted(180, Duration::from_secs(500), Mode::Session), 180.);
}
#[test]
fn measured_projection_filters_aggregation_and_unknowns() {
    let now = Instant::now();
    let mut s = State::default();
    s.apply(measured_event("src/tui/a", 120, 60, now));
    s.apply(measured_event("src/tui/b", 52, 0, now));
    s.apply(measured_event("src/tui/c", 18, 0, now));
    s.apply(Event::filesystem(Kind::Modified, "src/unknown".into()));
    let mut a = Event::filesystem(Kind::Read, "src/read".into());
    a.source = Source::Agent;
    s.apply(a);
    let snap = Snapshot::build(&s, Query::default(), now);
    assert_eq!(snap.directories[Path::new("src")].changed_files, 4);
    assert_eq!(snap.directories[Path::new("src/tui")].totals.churn(), 250);
    assert_eq!(snap.files[Path::new("src/read")].score, 0);
    assert_eq!(snap.files[Path::new("src/unknown")].totals.unknown, 1);
    assert_eq!(snap.files[Path::new("src/unknown")].score, 0);
    assert_eq!(snap.totals.churn(), 250);
    assert_eq!(snap.totals.unknown, 1);
    let agent = Snapshot::build(
        &s,
        Query {
            sources: Sources::Agent,
            ..Default::default()
        },
        now,
    );
    assert_eq!(agent.totals.churn(), 0);
    assert_eq!(agent.totals.unknown, 0);
    let modified = Snapshot::build(
        &s,
        Query {
            types: Types::Modified,
            ..Default::default()
        },
        now,
    );
    assert_eq!(modified.totals.churn(), 250);
}
#[test]
fn session_totals_outlive_timeline_and_rename_preserves_unique_volume() {
    let now = Instant::now();
    let mut s = State::default();
    s.apply(measured_event("src/a", 30, 30, now));
    let mut rename = Event::filesystem(Kind::Renamed, "tests/a".into());
    rename.old_path = Some("src/a".into());
    rename.measurement = Some(content::Measurement {
        delta: Some(Default::default()),
        net: Some(Default::default()),
        reason: None,
    });
    s.apply(rename);
    for _ in 0..HISTORY_LIMIT {
        s.apply(measured_event("tests/b", 1, 0, now));
    }
    let snap = Snapshot::build(
        &s,
        Query {
            heat_mode: Mode::Session,
            ..Default::default()
        },
        now + Duration::from_secs(500),
    );
    assert!(!snap.files.contains_key(Path::new("src/a")));
    assert_eq!(snap.files[Path::new("tests/a")].totals.churn(), 60);
    assert_eq!(snap.totals.churn(), 1060);
    assert_eq!(snap.directories[Path::new("tests")].changed_files, 2);
    assert_eq!(snap.mix, [1001, 0, 0, 1, 0]);
    let scoped = Snapshot::build(
        &s,
        Query {
            heat_mode: Mode::Session,
            scope: Scope::Src,
            ..Default::default()
        },
        now,
    );
    assert_eq!(scoped.totals.churn(), 0);
}

use super::*;
fn delta(a: &str, b: &str) -> LineDelta {
    diff(a, b).unwrap()
}
#[test]
fn exact_physical_lines_and_normalization() {
    for (a, b, added, deleted) in [
        ("a\n", "a\nb\n", 1, 0),
        ("a\nb\n", "a\n", 0, 1),
        ("a\n", "b\n", 1, 1),
        ("a\nb\nc\nd\n", "A\nb\nC\nd\n", 2, 2),
        ("same\n", "same\n", 0, 0),
        ("a\r\nb\r\n", "a\nb\n", 0, 0),
        ("ёж\n", "猫\n", 1, 1),
        ("", "", 0, 0),
        ("", "new\n", 1, 0),
        ("gone\n", "", 0, 1),
        ("x\n", "x", 0, 0),
    ] {
        assert_eq!(delta(a, b), LineDelta { added, deleted }, "{a:?} -> {b:?}");
    }
}
#[test]
fn bounded_myers_agrees_with_small_dynamic_programming_oracle() {
    let strings: Vec<_> = (0u32..64)
        .map(|bits| {
            (0..6)
                .map(|i| if bits & (1 << i) == 0 { "a\n" } else { "b\n" })
                .collect::<String>()
        })
        .collect();
    for a in &strings {
        for b in &strings {
            let aa: Vec<_> = a.lines().collect();
            let bb: Vec<_> = b.lines().collect();
            let mut table = [[0u32; 7]; 7];
            for (i, row) in table.iter_mut().enumerate() {
                row[0] = i as u32;
            }
            for (i, cell) in table[0].iter_mut().enumerate() {
                *cell = i as u32;
            }
            for i in 1..=6 {
                for j in 1..=6 {
                    table[i][j] = if aa[i - 1] == bb[j - 1] {
                        table[i - 1][j - 1]
                    } else {
                        1 + table[i - 1][j].min(table[i][j - 1])
                    };
                }
            }
            assert_eq!(delta(a, b).churn(), u64::from(table[6][6]));
        }
    }
}
#[test]
fn work_and_size_caps_are_unknown() {
    assert!(diff(&"x".repeat(FILE_LIMIT + 1), "").is_none());
    assert!(diff(&"a\n".repeat(LINE_LIMIT + 1), "").is_none());
    assert!(diff(&"a\n".repeat(2000), &"b\n".repeat(2000)).is_none());
}
fn setup() -> (tempfile::TempDir, Snapshots, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = PathBuf::from("file.rs");
    std::fs::write(t.path().join(&p), "a\nb\n").unwrap();
    let mut s = Snapshots::default();
    s.baseline(t.path(), &p);
    (t, s, p)
}
#[test]
fn consecutive_revisions_revert_and_net_are_distinct() {
    let (t, mut s, p) = setup();
    for text in ["a\nB\n", "a\nb\n"] {
        std::fs::write(t.path().join(&p), text).unwrap();
        let m = s.measure(t.path(), &p, Kind::Modified, None);
        assert_eq!(m.delta.unwrap().churn(), 2);
        assert_eq!(
            m.net.unwrap().churn(),
            if text.contains('B') { 2 } else { 0 }
        );
    }
    assert_eq!(
        s.measure(t.path(), &p, Kind::Modified, None)
            .delta
            .unwrap()
            .churn(),
        0
    );
    std::fs::File::open(t.path().join(&p))
        .unwrap()
        .set_modified(std::time::SystemTime::now())
        .unwrap();
    assert_eq!(
        s.measure(t.path(), &p, Kind::Modified, None)
            .delta
            .unwrap()
            .churn(),
        0
    );
}
#[test]
fn create_delete_recreate_rename_and_atomic_save() {
    let (t, mut s, p) = setup();
    let new = PathBuf::from("renamed.rs");
    std::fs::rename(t.path().join(&p), t.path().join(&new)).unwrap();
    assert_eq!(
        s.measure(t.path(), &new, Kind::Renamed, Some(&p))
            .delta
            .unwrap()
            .churn(),
        0
    );
    std::fs::write(t.path().join("temp"), "a\nB\n").unwrap();
    std::fs::rename(t.path().join("temp"), t.path().join(&new)).unwrap();
    assert_eq!(
        s.measure(t.path(), &new, Kind::Modified, None)
            .delta
            .unwrap()
            .churn(),
        2
    );
    std::fs::remove_file(t.path().join(&new)).unwrap();
    assert_eq!(
        s.measure(t.path(), &new, Kind::Deleted, None)
            .delta
            .unwrap(),
        LineDelta {
            added: 0,
            deleted: 2
        }
    );
    std::fs::write(t.path().join(&new), "a\nb\n").unwrap();
    let m = s.measure(t.path(), &new, Kind::Created, None);
    assert_eq!(m.delta.unwrap().added, 2);
    assert_eq!(m.net.unwrap().churn(), 0);
    let fresh = PathBuf::from("新.rs");
    std::fs::write(t.path().join(&fresh), "x\n".repeat(200)).unwrap();
    assert_eq!(
        s.measure(t.path(), &fresh, Kind::Created, None)
            .delta
            .unwrap()
            .added,
        200
    );
}
#[test]
fn unknown_baseline_unreadable_binary_and_oversize_do_not_become_zero() {
    let (t, mut s, p) = setup();
    let missing = PathBuf::from("missing");
    assert!(s
        .measure(t.path(), &missing, Kind::Modified, None)
        .delta
        .is_none());
    let unknown = PathBuf::from("unknown");
    std::fs::write(t.path().join(&unknown), "a\n").unwrap();
    assert!(s
        .measure(t.path(), &unknown, Kind::Modified, None)
        .delta
        .is_none());
    std::fs::write(t.path().join(&p), [0, 1, 2]).unwrap();
    assert!(s
        .measure(t.path(), &p, Kind::Modified, None)
        .delta
        .is_none());
    std::fs::write(t.path().join(&p), "x".repeat(FILE_LIMIT + 1)).unwrap();
    assert!(s
        .measure(t.path(), &p, Kind::Modified, None)
        .delta
        .is_none());
}
#[test]
fn snapshot_payload_and_admission_are_bounded() {
    let t = tempfile::tempdir().unwrap();
    let mut s = Snapshots::default();
    for i in 0..66 {
        let p = PathBuf::from(format!("{i}.rs"));
        std::fs::write(t.path().join(&p), "x".repeat(FILE_LIMIT)).unwrap();
        s.baseline(t.path(), &p);
        assert!(s.bytes <= MEMORY_LIMIT);
    }
    assert_eq!(s.covered, 64);
    assert_eq!(s.total, 66);
    s.retain(|_| false);
    assert_eq!(s.bytes, 0);
}
#[cfg(unix)]
#[test]
fn external_symlink_cannot_be_snapshotted() {
    let (t, mut s, p) = setup();
    let external = tempfile::tempdir().unwrap();
    std::fs::write(external.path().join("secret"), "outside").unwrap();
    std::fs::remove_file(t.path().join(&p)).unwrap();
    std::os::unix::fs::symlink(external.path().join("secret"), t.path().join(&p)).unwrap();
    assert!(s
        .measure(t.path(), &p, Kind::Modified, None)
        .delta
        .is_none());
}

#[test]
fn deleted_unknown_baseline_does_not_invent_initial_empty_content_on_recreation() {
    let t = tempfile::tempdir().unwrap();
    let p = PathBuf::from("unknown.rs");
    let mut s = Snapshots::default();
    std::fs::write(t.path().join(&p), [0, 1]).unwrap();
    s.baseline(t.path(), &p);
    std::fs::remove_file(t.path().join(&p)).unwrap();
    assert!(s.measure(t.path(), &p, Kind::Deleted, None).delta.is_none());
    std::fs::write(t.path().join(&p), "text\n").unwrap();
    let m = s.measure(t.path(), &p, Kind::Created, None);
    assert_eq!(m.delta.unwrap().added, 1);
    assert!(m.net.is_none());
}

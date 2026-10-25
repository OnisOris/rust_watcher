use super::{diagram::*, *};
#[test]
fn geometry_is_bounded_directed_disjoint_stable_and_selection_is_visible() {
    for (w, h) in [(40, 9), (50, 19), (64, 24), (85, 34)] {
        for total in [0, 1, 2, 5, 12, 47, 2000] {
            for selected in [0, total / 2, total] {
                for mode in [Mode::Hierarchy, Mode::Imports, Mode::UsedBy] {
                    let d = layout(w, h, total, selected, mode, true).unwrap();
                    assert_eq!(d, layout(w, h, total, selected, mode, true).unwrap());
                    assert_eq!(d.nodes.iter().filter(|n| n.ordinal == 0).count(), 1);
                    assert!(d.nodes.iter().any(|n| n.ordinal == selected));
                    assert!(d.nodes.len() <= 13);
                    for n in &d.nodes {
                        assert!(n.rect.right() <= w && n.rect.bottom() <= h);
                        for other in &d.nodes {
                            if n.ordinal != other.ordinal {
                                assert!(n.rect.intersection(other.rect).is_empty());
                            }
                        }
                    }
                    for e in &d.edges {
                        if e.from != e.to {
                            assert_eq!(if mode == Mode::UsedBy { e.to } else { e.from }, 0);
                        }
                        for pair in e.points.windows(2) {
                            let (a, b) = (pair[0], pair[1]);
                            assert!(a.0 == b.0 || a.1 == b.1);
                            for x in a.0.min(b.0)..=a.0.max(b.0) {
                                for y in a.1.min(b.1)..=a.1.max(b.1) {
                                    assert!(x < w && y < h);
                                    assert!(d
                                        .nodes
                                        .iter()
                                        .all(|n| !n.rect.contains((x, y).into())));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(layout(27, 9, 1, 1, Mode::Imports, false).is_none());
    assert!(layout(40, 8, 1, 1, Mode::Imports, false).is_none());
}

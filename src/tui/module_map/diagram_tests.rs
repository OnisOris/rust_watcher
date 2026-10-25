use super::{diagram::*, *};
#[test]
fn geometry_is_bounded_directed_disjoint_stable_and_selection_is_visible() {
    for (w, h) in [(40, 9), (50, 19), (64, 24), (85, 34)] {
        for total in [0, 1, 2, 5, 12, 40, 47, 2000] {
            for selected in [0, total.min(13), total / 2, total] {
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
                        let from = d.nodes.iter().find(|n| n.ordinal == e.from).unwrap().rect;
                        let to = d.nodes.iter().find(|n| n.ordinal == e.to).unwrap().rect;
                        let first = e.points[0];
                        let last = *e.points.last().unwrap();
                        if e.from == e.to {
                            assert_eq!(first, (from.right(), from.y + 1));
                        } else if mode == Mode::UsedBy {
                            assert_eq!(first, (from.x + from.width / 2, from.y - 1));
                            assert_eq!(last, (to.x + to.width / 2, to.bottom()));
                            // The final routed segment can be horizontal; its explicit
                            // port connects upward into the center's bottom border.
                            assert!(to.y < last.1);
                        } else {
                            assert_eq!(first, (from.x + from.width / 2, from.bottom()));
                            assert_eq!(last, (to.x + to.width / 2, to.y - 1));
                            assert!(e.points[e.points.len() - 2].1 < last.1);
                        }
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

#[test]
fn dense_indexed_neighborhood_preserves_records_direction_and_thirteenth_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='dense'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    let mut src = String::new();
    for i in 0..40 {
        src.push_str(&format!(
            "mod m{i:02} {{pub struct Item;use crate::center;}}\n"
        ));
    }
    src.push_str("mod center {use crate::center;use unknown::Unproven;\n");
    for i in 0..40 {
        src.push_str(&format!(
            "use crate::m{i:02};#[cfg(test)] use crate::m{i:02}::Item as Item{i:02};\n"
        ));
    }
    src.push('}');
    std::fs::write(root.join("src/lib.rs"), src).unwrap();
    let p = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    let g = crate::module_graph::analyze(root, &tree, &p.architecture, || false);
    let center = g
        .nodes
        .iter()
        .find(|n| n.name == "center")
        .unwrap()
        .id
        .clone();
    let mut app = App::new(p);
    app.module_map.apply(g);
    app.switch_view(super::super::app::View::Map);
    for mode in [Mode::Imports, Mode::UsedBy] {
        app.module_map.mode = mode;
        app.module_map.focus = center.clone();
        app.module_map.selected = center.clone();
        assert_eq!(app.module_map.choices().len(), 41);
        let e = &app.module_map.index.nodes[&center];
        assert_eq!(e.outgoing.len(), 41);
        assert_eq!(e.incoming.len(), 41);
        assert!(e
            .outgoing
            .iter()
            .filter(|l| l.id != center)
            .all(|l| l.records == 2 && l.conditional == 1));
        for _ in 0..13 {
            app.module_map.move_by(1);
        }
        assert_eq!(
            app.module_map
                .graph
                .as_ref()
                .unwrap()
                .node(&app.module_map.selected)
                .unwrap()
                .name,
            "m12"
        );
        for (w, h) in [(80, 20), (100, 30), (120, 35), (160, 45)] {
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            t.draw(|f| super::super::ui::render(f, &app)).unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("m12"));
            assert!(text.contains("↺×1"));
            let cache = app.module_map.diagram_cache.borrow();
            let d = cache.as_ref().unwrap().1.as_ref().unwrap();
            assert_eq!(d.total, 40);
            assert!(d.nodes.iter().any(|n| n.ordinal == 13));
            assert!(d.nodes.len() <= 13);
        }
    }
}

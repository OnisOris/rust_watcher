use super::*;
fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, crate::project::Project, RepositoryTree) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (p, s) in std::iter::once((
        "Cargo.toml",
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    ))
    .chain(files.iter().copied())
    {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }
    let project = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    (temp, project, tree)
}
#[test]
fn default_path_contexts_path_attributes_and_inline_directories() {
    let (temp,p,t)=fixture(&[("src/lib.rs","mod a; mod directory; #[path=\"custom/named.rs\"] mod custom; #[path=\"somewhere\"] mod inline { #[path=\"nested.rs\"] mod nested; }"),("src/a.rs","mod inside { mod leaf; #[path=\"other.rs\"] mod other; } #[path=\"alternate.rs\"] mod alt;"),("src/a/inside/leaf.rs",""),("src/a/inside/other.rs",""),("src/alternate.rs",""),("src/directory/mod.rs","mod leaf;"),("src/directory/leaf.rs",""),("src/custom/named.rs","mod leaf;"),("src/custom/leaf.rs",""),("src/somewhere/nested.rs","")]);
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert_eq!(g.unresolved_count, 0, "{:?}", g.unresolved);
    assert_eq!(g.nodes.len(), 12);
    for (q, path) in [
        ("crate::a::inside::other", "src/a/inside/other.rs"),
        ("crate::a::alt", "src/alternate.rs"),
        ("crate::custom::leaf", "src/custom/leaf.rs"),
        ("crate::inline::nested", "src/somewhere/nested.rs"),
    ] {
        assert_eq!(
            g.nodes
                .iter()
                .find(|n| n.qualified_path == q)
                .unwrap()
                .source_file
                .as_deref(),
            Some(Path::new(path))
        );
    }
}
#[test]
fn rooted_imports_aliases_reexports_and_ambiguous_names_never_guess() {
    let (temp,p,t)=fixture(&[("src/lib.rs","mod foo { pub struct Bar; } mod bar {pub struct Item; pub use crate::foo::Bar as Reexport;} mod consumer {mod helpers {} use crate::foo; use crate::foo::Bar; use crate::foo::{self as alias, Bar as Renamed}; pub use crate::bar::Item; use self::helpers as h; use super::bar as b; use crate::foo::*; use external::Thing; use crate::bar::Reexport; use crate::foo::Bar::Assoc; use alias::Bar; #[custom] use crate::foo::Bar as Changed; } #[cfg(a)] mod same {} #[cfg(b)] mod same {} mod other {use crate::same;}")]);
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    let consumer = g
        .nodes
        .iter()
        .find(|n| n.qualified_path == "crate::consumer")
        .unwrap();
    let edges: Vec<_> = g
        .verified_import_edges
        .iter()
        .filter(|e| e.from == consumer.id)
        .collect();
    assert_eq!(edges.len(), 7, "{edges:?}");
    assert!(edges.iter().any(|e| e.import_path == "self::helpers"));
    assert!(edges
        .iter()
        .any(|e| e.public && e.import_path == "crate::bar::Item"));
    assert!(!g
        .verified_import_edges
        .iter()
        .any(|e| e.import_path.contains("same")
            || e.import_path.contains("Assoc")
            || e.import_path.contains("Reexport")
            || e.import_path.starts_with("alias")));
    assert!(g
        .unresolved
        .iter()
        .any(|u| u.reason.contains("multiple") || u.reason.contains("duplicate")));
}
#[test]
fn ambiguity_ignored_files_parse_limits_and_cancellation_are_explicit() {
    let (temp, p, mut t) = fixture(&[
        ("src/lib.rs", "mod both; mod ignored; mod huge;"),
        ("src/both.rs", ""),
        ("src/both/mod.rs", ""),
        ("src/ignored.rs", ""),
        ("src/huge.rs", ""),
    ]);
    t.entries.retain(|e| e.path != Path::new("src/ignored.rs"));
    std::fs::write(
        temp.path().join("src/huge.rs"),
        " ".repeat(crate::maintainability::MAX_AST_FILE_BYTES as usize + 1),
    )
    .unwrap();
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert!(g.partial);
    assert_eq!(g.unresolved_count, 3);
    assert!(g
        .nodes
        .iter()
        .filter(|n| n.parent_id.is_some())
        .all(|n| n.status == ModuleStatus::Unresolved));
    t.truncated = true;
    let cancelled = analyze(temp.path(), &t, &p.architecture, || true);
    assert!(cancelled.partial);
    assert_eq!(cancelled.read_bytes, 0);
    let (temp, p, t) = fixture(&[(
        "src/lib.rs",
        "mod r#type {mod helper {}} mod other {use crate::r#type::helper;}",
    )]);
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert!(g.nodes.iter().any(|n| n.name == "type"));
}
#[test]
fn ids_survive_checkout_changes_and_duplicate_package_names_are_errors() {
    let files = [("src/lib.rs", "pub mod same {}")];
    let (a, p, t) = fixture(&files);
    let (b, q, u) = fixture(&files);
    assert_eq!(
        serde_json::to_value(analyze(a.path(), &t, &p.architecture, || false)).unwrap(),
        serde_json::to_value(analyze(b.path(), &u, &q.architecture, || false)).unwrap()
    );
    let mut model = p.architecture.clone();
    let mut duplicate = model.nodes[0].clone();
    duplicate.id = "crate:other/Cargo.toml".into();
    model.nodes.push(duplicate);
    let e = select(&model, Some("demo"), None).unwrap_err();
    assert_eq!(e.candidates.len(), 2);
    assert_eq!(select(&model, None, None).unwrap().nodes.len(), 2);
}

#[test]
fn nesting_byte_decoding_and_local_scopes_are_bounded_and_unresolved() {
    let (temp, p, _) = fixture(&[
        (
            "src/lib.rs",
            "mod deep; mod invalid; mod binary; fn f() {mod local {} use crate::deep;}",
        ),
        ("src/deep.rs", ""),
        ("src/invalid.rs", ""),
        ("src/binary.rs", ""),
    ]);
    std::fs::write(
        temp.path().join("src/deep.rs"),
        format!("{}{}", "mod n {".repeat(70), "}".repeat(70)),
    )
    .unwrap();
    std::fs::write(temp.path().join("src/invalid.rs"), [255, 254]).unwrap();
    std::fs::write(temp.path().join("src/binary.rs"), [0, 1]).unwrap();
    let t = crate::repository::scan(temp.path()).unwrap();
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert!(g.partial);
    assert!(g.nodes.len() <= MAX_DEPTH + 4);
    assert!(g.unresolved.iter().any(|u| u.reason.contains("depth")));
    assert!(g.unresolved.iter().any(|u| u.reason.contains("UTF-8")));
    assert!(g.unresolved.iter().any(|u| u.kind == "scope"));
    assert!(!g.nodes.iter().any(|n| n.name == "local"));
    std::fs::write(
        temp.path().join("src/deep.rs"),
        format!("{}{}", "mod n {".repeat(30), "}".repeat(30)),
    )
    .unwrap();
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert!(g.nodes.iter().any(|n| n.depth == 31));
}

#[test]
fn external_depth_budget_stops_without_recursive_stack_growth_in_parser() {
    let (temp, p, _) = fixture(&[("src/lib.rs", "#[path=\"f0.rs\"] mod n;")]);
    for n in 0..70 {
        std::fs::write(
            temp.path().join(format!("src/f{n}.rs")),
            format!("#[path=\"f{}.rs\"] mod n;", n + 1),
        )
        .unwrap();
    }
    let t = crate::repository::scan(temp.path()).unwrap();
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert_eq!(g.nodes.len(), MAX_DEPTH + 1);
    assert!(g.partial);
    assert!(g.unresolved.iter().any(|u| u.reason.contains("depth")));
}

#[test]
fn expanded_use_paths_labels_and_model_payloads_have_explicit_budgets() {
    let long = "x".repeat(MAX_PATH_BYTES + 1);
    let leaves = (0..MAX_USE_LEAVES + 5)
        .map(|n| format!("Leaf{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "mod {} {{}} use crate::{long}::{{{leaves}}}; use crate::short::{{{leaves}}};",
        "m".repeat(MAX_NAME_BYTES + 1)
    );
    let (temp, p, t) = fixture(&[("src/lib.rs", &source)]);
    let g = analyze(temp.path(), &t, &p.architecture, || false);
    assert!(g.partial);
    assert!(g.verified_import_edges.is_empty());
    assert_eq!(g.nodes.len(), 1);
    assert!(g.unresolved_count <= MAX_USE_LEAVES + 3);
    assert!(g.partial_reasons.iter().any(|r| r.contains("budget")));
    let mut g = ModuleGraph {
        model_bytes: MAX_MODEL_BYTES,
        ..Default::default()
    };
    let n = analyze(temp.path(), &t, &p.architecture, || false)
        .nodes
        .remove(0);
    assert!(!g.reserve_node(&n));
}
#[test]
#[ignore = "manual release module discovery and render timings, real workspaces"]
fn performance() {
    for path in [
        env!("CARGO_MANIFEST_DIR"),
        "/home/poke/code/uav-hit-estimator-tauri",
    ] {
        let root = Path::new(path);
        if !root.is_dir() {
            continue;
        }
        let p = crate::project::Project::discover(root).unwrap();
        let start = std::time::Instant::now();
        let t = crate::repository::scan(root).unwrap();
        let scan = start.elapsed();
        let start = std::time::Instant::now();
        let g = analyze(root, &t, &p.architecture, || false);
        let elapsed = start.elapsed();
        eprintln!("{} scan={scan:?} modules={elapsed:?} files={} nodes={} mod={} use={} unresolved={} conditional={} partial={} read={}",path,g.parsed_files,g.nodes.len(),g.containment_edges.len(),g.verified_import_edges.len(),g.unresolved_count,g.conditional_relations,g.partial,g.read_bytes);
    }
}
#[test]
fn discovery_and_imports_are_contextual_conservative_and_stable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let write = |p: &str, s: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    write("Cargo.toml","[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n[[bin]]\nname='demo-bin'\npath='src/lib.rs'\n");
    write("src/lib.rs","mod foo; mod bar; mod helpers {} mod inside { mod nested; #[path=\"custom.rs\"] mod custom; } #[cfg(test)] mod tests; #[cfg(feature=\"x\")] mod feature {} #[cfg_attr(test,path=\"alt.rs\")] mod uncertain; mod missing; mod broken; #[path=\"lib.rs\"] mod cycle; make_modules!(); use crate::foo; use crate::foo::Bar; use crate::foo::{self as alias, Bar as Renamed}; pub use crate::bar::Item; use self::helpers; use crate::foo::*; use external::Thing; use crate::bar::Reexport;");
    write("src/foo.rs","pub struct Bar; mod child; mod sibling {} use super::bar; use self::sibling; use crate::bar::Item::Associated;");
    write("src/foo/child.rs", "pub fn work() {}\n");
    write(
        "src/bar/mod.rs",
        "pub struct Item; pub use crate::foo::Bar as Reexport;",
    );
    write("src/inside/nested.rs", "");
    write("src/inside/custom.rs", "");
    write("src/tests.rs", "");
    write("src/broken.rs", "fn broken(");
    write("src/unused.rs", "mod imaginary;");
    let project = crate::project::Project::discover(root).unwrap();
    let tree = crate::repository::scan(root).unwrap();
    let g = analyze(root, &tree, &project.architecture, || false);
    assert_eq!(g.targets.len(), 2);
    let target = &g.targets[0].id;
    let node = |q: &str| {
        g.nodes
            .iter()
            .find(|n| &n.target_id == target && n.qualified_path == q)
            .unwrap()
    };
    assert_eq!(
        node("crate::foo::child").source_file.as_deref(),
        Some(Path::new("src/foo/child.rs"))
    );
    assert_eq!(
        node("crate::inside::custom").source_file.as_deref(),
        Some(Path::new("src/inside/custom.rs"))
    );
    assert_eq!(
        node("crate::tests").status,
        ModuleStatus::DeclaredConditional
    );
    for q in [
        "crate::uncertain",
        "crate::missing",
        "crate::broken",
        "crate::cycle",
    ] {
        assert_eq!(node(q).status, ModuleStatus::Unresolved);
    }
    assert!(!g.nodes.iter().any(|n| n.name == "imaginary"));
    assert!(g.partial);
    assert!(g.unresolved.iter().any(|u| u.kind == "macro"));
    assert!(g
        .verified_import_edges
        .iter()
        .any(|e| e.import_path == "super::bar"));
    assert!(g
        .verified_import_edges
        .iter()
        .any(|e| e.import_path == "crate::bar::Item" && e.public));
    assert!(!g
        .verified_import_edges
        .iter()
        .any(|e| e.import_path.contains("Associated")
            || e.import_path.contains("Reexport")
            || e.import_path.contains("external")));
    assert_eq!(
        serde_json::to_value(&g).unwrap(),
        serde_json::to_value(analyze(root, &tree, &project.architecture, || false)).unwrap()
    );
    let root_node = g.nodes.iter().find(|n| n.parent_id.is_none()).unwrap();
    assert_eq!(g.subtree_files(&root_node.id).len(), 8);
    assert_eq!(g.parsed_files, 7);
    assert_eq!(
        g.read_bytes,
        g.subtree_files(&root_node.id)
            .iter()
            .map(|p| std::fs::metadata(root.join(p)).unwrap().len())
            .sum::<u64>()
    );
    assert!(
        g.nodes
            .iter()
            .filter(|n| n.source_file.as_deref() == Some(Path::new("src/foo.rs")))
            .count()
            >= 2
    );
}

#[test]
fn full_unresolved_counts_survive_retention_budget_and_line_moves_do_not_change_ids() {
    let mut g = ModuleGraph::default();
    for n in 0..MAX_DETAILS + 3 {
        g.issue(
            "module",
            "import",
            Path::new("src/lib.rs"),
            n + 1,
            "unresolved",
        );
    }
    assert_eq!(g.unresolved.len(), MAX_DETAILS);
    assert_eq!(g.unresolved_count, MAX_DETAILS + 3);
    assert_eq!(g.unresolved_by_module["module"].imports, MAX_DETAILS + 3);
    let (temp, p, t) = fixture(&[("src/lib.rs", "mod inline {pub fn f(){}}")]);
    let before = analyze(temp.path(), &t, &p.architecture, || false);
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "// shifted declaration\n\nmod inline {pub fn f(){}}",
    )
    .unwrap();
    let after = analyze(temp.path(), &t, &p.architecture, || false);
    assert_eq!(
        before.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
        after.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
    );
    std::fs::write(temp.path().join("src/lib.rs"), "mod 𐐀 {mod 内{}}").unwrap();
    let unicode = analyze(temp.path(), &t, &p.architecture, || false);
    let inner = unicode.nodes.iter().find(|n| n.name == "内").unwrap();
    assert_eq!(
        inner.declaration_column,
        "mod 𐐀 {mod ".encode_utf16().count() + 1
    );
    std::fs::write(temp.path().join("src/lib.rs"), "\u{feff}mod 𐐀 {mod 内{}}").unwrap();
    let unicode = analyze(temp.path(), &t, &p.architecture, || false);
    let inner = unicode.nodes.iter().find(|n| n.name == "内").unwrap();
    assert_eq!(
        inner.declaration_column,
        "\u{feff}mod 𐐀 {mod ".encode_utf16().count() + 1
    );
}

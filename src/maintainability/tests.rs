use super::*;
use crate::project::Package;
use crate::repository::scan;

fn project(root: &Path) -> Project {
    Project {
        workspace_root: root.into(),
        packages: vec![],
        rust_files: vec![],
    }
}

fn parsed(source: &str) -> Vec<FunctionMetrics> {
    syntax::functions(
        source,
        Path::new("src/lib.rs"),
        None,
        FUNCTION_LIMIT,
        &|| false,
    )
    .unwrap()
}

#[test]
fn function_kinds_positions_and_boundaries_are_syntactic() {
    let source = "#[allow(dead_code)]\nfn empty() {}\nasync fn work() {\n execute().await;\n}\nimpl<T> Engine<T> {\n fn run(\n &self,\n ) {\n // { if while }\n let _ = \"{ loop }\";\n }\n fn new() {}\n}\ntrait Api {\n fn abstract_method(&self);\n fn default_method(&self) {}\n}\nmod inner { fn same() {} }\nfn outer() { fn same() { if true {} } }\n";
    let functions = parsed(source);
    assert_eq!(functions.len(), 8);
    assert_eq!(
        (
            functions[0].start_line,
            functions[0].end_line,
            functions[0].physical_lines
        ),
        (2, 2, 1)
    );
    assert_eq!(
        (functions[1].name.as_str(), functions[1].physical_lines),
        ("work", 3)
    );
    assert_eq!(functions[2].qualified_name, "Engine::run");
    assert_eq!(functions[2].kind, FunctionKind::Method);
    assert_eq!(
        (
            functions[2].start_line,
            functions[2].end_line,
            functions[2].physical_lines
        ),
        (7, 12, 6)
    );
    assert_eq!(functions[3].kind, FunctionKind::AssociatedFunction);
    assert_eq!(functions[4].kind, FunctionKind::TraitDefault);
    assert_eq!(functions[5].qualified_name, "inner::same");
    assert_eq!(functions[7].qualified_name, "outer::same");
    assert_eq!(functions[6].control_flow_nodes, 0);
    assert_eq!(functions[7].control_flow_nodes, 1);
}

#[test]
fn control_depth_excludes_closures_items_plain_blocks_and_match_arms() {
    let f = parsed(
        r#"
fn example() {
 for item in items {
  if item.ready() { match item.kind { A => {}, B => {} } }
  else if let Some(x) = item { while let Some(y) = x { loop { break; } } }
 }
 let _ = || { if true { loop {} } };
 unsafe { let _ = value?; }
 fn nested() { loop { loop {} } }
}
"#,
    );
    assert_eq!(f[0].max_control_depth, 4);
    assert_eq!(f[0].control_flow_nodes, 6);
    assert_eq!(f[1].max_control_depth, 2);
    let chain = parsed("fn chain() { if a {} else if b {} else if c {} }");
    assert_eq!(
        (chain[0].max_control_depth, chain[0].control_flow_nodes),
        (1, 3)
    );
    let arms = parsed("fn arms() { match a { A=>{}, B=>{}, C=>{} } }");
    assert_eq!(
        (arms[0].max_control_depth, arms[0].control_flow_nodes),
        (1, 1)
    );
}

#[test]
fn macros_do_not_expand_and_inline_test_markers_are_visible() {
    let f = parsed("macro_rules! make { () => { fn generated() {} } }\nmake!();\n#[cfg(test)] mod tests { fn helper() {} #[test] fn test_it() {} }\nfn ordinary() { macro_call!(if loop { fn generated() {} }); }");
    assert_eq!(f.len(), 3);
    assert!(f[0].test_classified && f[1].test_classified);
    assert!(!f[2].test_classified);
    assert_eq!(f[2].control_flow_nodes, 0);
}

#[test]
fn reports_failures_nested_packages_percentiles_and_stable_rankings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("nested")).unwrap();
    std::fs::write(
        root.join("a.rs"),
        "fn same() {}\nfn longer() {\n if true {}\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("nested/b.rs"), "fn same() {}\n").unwrap();
    std::fs::write(root.join("bad.rs"), "fn broken( {").unwrap();
    let mut p = project(root);
    for (name, path) in [("outer", "Cargo.toml"), ("inner", "nested/Cargo.toml")] {
        p.packages.push(Package {
            name: name.into(),
            manifest_path: root.join(path),
            targets: vec![],
            dependencies: vec![],
        });
    }
    let tree = scan(root).unwrap();
    let r = analyze(root, &tree, &p);
    assert_eq!(
        (
            r.analyzed_files,
            r.parsed_files,
            r.parse_failures,
            r.functions
        ),
        (3, 2, 1, 3)
    );
    assert!(r.partial);
    assert_eq!(r.failures[0].file, Path::new("bad.rs"));
    assert_eq!((r.median_function_lines, r.p95_function_lines), (1, 3));
    assert_eq!(r.longest_functions[0].name, "longer");
    assert_eq!(r.longest_functions[1].path, Path::new("a.rs"));
    assert_eq!(
        r.packages.iter().map(|p| p.function_count).sum::<usize>(),
        3
    );
    assert_eq!(r.packages[0].name, "inner");
    assert_eq!(r.packages[0].function_count, 1);
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        serde_json::to_value(analyze(root, &tree, &p)).unwrap()
    );
    let empty = analyze(root, &RepositoryTree::default(), &p);
    assert_eq!(
        (
            empty.functions,
            empty.median_function_lines,
            empty.p95_function_lines
        ),
        (0, 0, 0)
    );
    let cancelled = analyze_until(root, &tree, &p, || true);
    assert!(cancelled.partial);
    let mut partial = tree;
    partial.truncated = true;
    assert!(analyze(root, &partial, &p).partial);
}

#[test]
fn identifier_anchor_uses_one_based_utf16_coordinates() {
    let f = parsed("fn outer() { let _ = \"🦀\"; fn r#type() {} }");
    assert_eq!(f[1].name, "type");
    assert_eq!(
        f[1].identifier_column,
        "fn outer() { let _ = \"🦀\"; fn ".encode_utf16().count() + 1
    );
    assert_eq!(parsed("\u{feff}fn bom() {}")[0].identifier_column, 5);
    assert_eq!(
        parsed("#!/usr/bin/env rust-script\nfn script() {}")[0].start_line,
        2
    );
}

#[test]
fn ast_resource_limits_and_failure_samples_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::File::create(root.join("large.rs"))
        .unwrap()
        .set_len(MAX_AST_FILE_BYTES + 1)
        .unwrap();
    for i in 0..30 {
        std::fs::write(root.join(format!("bad{i:02}.rs")), "fn (").unwrap();
    }
    std::fs::write(
        root.join("deep.rs"),
        format!("fn deep() {{ {} 1 {} }}", "(".repeat(140), ")".repeat(140)),
    )
    .unwrap();
    let r = analyze(root, &scan(root).unwrap(), &project(root));
    assert_eq!(r.skipped_files, 2);
    assert_eq!(r.parse_failures, 30);
    assert_eq!(r.failures.len(), FAILURE_LIMIT);
    assert!(r.partial);
}

#[test]
#[ignore = "local scan/metrics/AST timing on real projects and synthetic 10k Rust files"]
fn performance() {
    fn measure(root: &Path) {
        let p = project(root);
        let start = std::time::Instant::now();
        let tree = scan(root).unwrap();
        let scan_time = start.elapsed();
        let start = std::time::Instant::now();
        let m = crate::metrics::analyze(root, &tree, &p);
        let metrics_time = start.elapsed();
        let start = std::time::Instant::now();
        let r = analyze(root, &tree, &p);
        eprintln!("{}: scan {:?}, basic {:?}, AST {:?}; files {}, parsed {}, failures {}, skipped {}, functions {}, partial {}",root.display(),scan_time,metrics_time,start.elapsed(),m.total_files,r.parsed_files,r.parse_failures,r.skipped_files,r.functions,r.partial);
    }
    measure(Path::new(env!("CARGO_MANIFEST_DIR")));
    let uav = Path::new("/home/poke/code/uav-hit-estimator-tauri");
    if uav.is_dir() {
        measure(uav);
    }
    let temp = tempfile::tempdir().unwrap();
    for i in 0..10_000 {
        std::fs::write(
            temp.path().join(format!("source{i:05}.rs")),
            format!("fn f{i}() {{\n{}if true {{}}\n}}\n", "// body\n".repeat(96)),
        )
        .unwrap();
    }
    measure(temp.path());
}

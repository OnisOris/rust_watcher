use super::*;
use cargo_metadata::{Metadata, MetadataCommand};
use std::path::Path;

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
    names: Vec<String>,
}
impl Fixture {
    fn new(nested: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = if nested {
            temp.path().join("monorepo/rust")
        } else {
            temp.path().to_path_buf()
        };
        std::fs::create_dir_all(&root).unwrap();
        Self {
            temp,
            root,
            names: vec![],
        }
    }
    fn add(&mut self, name: &str, deps: &str) {
        let dir = self.root.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n{deps}"),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
        self.names.push(name.into());
    }
    fn metadata(&self) -> Metadata {
        std::fs::write(
            self.root.join("Cargo.toml"),
            format!(
                "[workspace]\nresolver='2'\nmembers=[{}]\n",
                self.names
                    .iter()
                    .map(|n| format!("'{n}'"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        )
        .unwrap();
        MetadataCommand::new()
            .manifest_path(self.root.join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap()
    }
}
#[test]
fn singleton_and_verified_branching_kinds_aliases_external_and_components() {
    let mut one = Fixture::new(false);
    one.add("only", "");
    let graph = Architecture::from_metadata(&one.metadata());
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.edges.is_empty());
    assert!(!graph.partial);
    let mut f = Fixture::new(true);
    f.add("alpha","[dependencies]\nrenamed={package='beta',path='../beta'}\nanyhow='1'\n[dev-dependencies]\ngamma={path='../gamma'}\n[build-dependencies]\ndelta={path='../delta'}\n");
    f.add("beta", "[dependencies]\ngamma={path='../gamma'}\n");
    f.add("gamma", "");
    f.add("delta", "");
    f.add("omega", "");
    let metadata = f.metadata();
    let graph = Architecture::from_metadata(&metadata);
    assert!(!graph.partial, "{:?}", graph.unresolved_dependencies);
    assert_eq!(graph.nodes.len(), 5);
    assert_eq!(graph.edges.len(), 4);
    let alpha = graph.nodes.iter().find(|n| n.name == "alpha").unwrap();
    assert_eq!(alpha.external_dependencies, 1);
    let deps: Vec<_> = graph.outgoing(&alpha.id).collect();
    assert_eq!(deps.len(), 3);
    assert!(deps.iter().any(|e| e.alias == "renamed"
        && e.kind == DependencyKind::Normal
        && e.to == "crate:beta/Cargo.toml"));
    assert!(deps
        .iter()
        .any(|e| e.kind == DependencyKind::Dev && e.to == "crate:gamma/Cargo.toml"));
    assert!(deps
        .iter()
        .any(|e| e.kind == DependencyKind::Build && e.to == "crate:delta/Cargo.toml"));
    assert_eq!(graph.neighborhood("crate:omega/Cargo.toml").len(), 1);
    assert!(graph
        .nodes
        .iter()
        .all(|n| !n.manifest_path.is_absolute() && !n.manifest_path.starts_with("..")));
    assert!(!serde_json::to_string(&graph)
        .unwrap()
        .contains(f.temp.path().to_str().unwrap()));
    assert_eq!(
        serde_json::to_value(&graph).unwrap(),
        serde_json::to_value(Architecture::from_metadata(&metadata)).unwrap()
    );
}
#[test]
fn unresolved_never_becomes_a_name_only_edge_and_duplicate_identity_is_rejected() {
    let mut f = Fixture::new(false);
    f.add("app", "[dependencies]\ncore={path='../core'}\n");
    f.add("core", "");
    let metadata = f.metadata();
    let mut broken = metadata.clone();
    let app = broken
        .packages
        .iter_mut()
        .find(|p| p.name == "app")
        .unwrap();
    app.dependencies[0].path = None;
    let graph = Architecture::from_metadata(&broken);
    assert!(graph.partial);
    assert!(graph.edges.is_empty());
    assert_eq!(graph.unresolved_dependencies.len(), 1);
    // Same spelling from a registry is external, not the workspace member.
    let dep = &mut broken
        .packages
        .iter_mut()
        .find(|p| p.name == "app")
        .unwrap()
        .dependencies[0];
    dep.source = Some(
        serde_json::from_value(serde_json::json!(
            "registry+https://github.com/rust-lang/crates.io-index"
        ))
        .unwrap(),
    );
    let external = Architecture::from_metadata(&broken);
    assert!(external.edges.is_empty());
    assert!(!external.partial);
    assert_eq!(
        external
            .nodes
            .iter()
            .find(|n| n.name == "app")
            .unwrap()
            .external_dependencies,
        1
    );
    let mut duplicate = metadata.clone();
    let mut core = duplicate
        .packages
        .iter()
        .find(|p| p.name == "core")
        .unwrap()
        .clone();
    core.id.repr.push_str("-other-identity");
    duplicate.workspace_members.push(core.id.clone());
    duplicate.packages.push(core);
    let graph = Architecture::from_metadata(&duplicate);
    assert!(graph.partial);
    assert!(graph.edges.is_empty());
    assert!(graph
        .unresolved_dependencies
        .iter()
        .any(|u| u.reason.contains("ambiguous")));
    let mut missing = metadata;
    missing
        .packages
        .iter_mut()
        .find(|p| p.name == "app")
        .unwrap()
        .dependencies[0]
        .path = Some(f.root.join("absent").to_str().unwrap().into());
    assert!(Architecture::from_metadata(&missing).edges.is_empty());
}
#[test]
fn multiple_targets_and_synthetic_fifty_crates_have_stable_relative_identities() {
    let mut f = Fixture::new(false);
    for n in 0..55 {
        f.add(
            &format!("node{n:02}"),
            &if n == 0 {
                String::new()
            } else {
                format!(
                    "[dependencies]\nnode{:02}={{path='../node{:02}'}}\n",
                    n - 1,
                    n - 1
                )
            },
        );
    }
    let dir = f.root.join("node00");
    std::fs::create_dir(dir.join("bin")).unwrap();
    std::fs::write(dir.join("bin/a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("bin/b.rs"), "fn main() {}\n").unwrap();
    use std::io::Write;
    write!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join("Cargo.toml"))
            .unwrap(),
        "\n[[bin]]\nname='cli-a'\npath='bin/a.rs'\n[[bin]]\nname='cli-b'\npath='bin/b.rs'\n"
    )
    .unwrap();
    let graph = Architecture::from_metadata(&f.metadata());
    assert_eq!(graph.nodes.len(), 55);
    assert_eq!(graph.edges.len(), 54);
    assert!(!graph.partial);
    assert_eq!(
        graph.node("crate:node00/Cargo.toml").unwrap().targets.len(),
        3
    );
    assert_eq!(graph.neighborhood("crate:node30/Cargo.toml").len(), 3);
    let mut cyclic = graph.clone();
    let mut back = cyclic.edges[0].clone();
    std::mem::swap(&mut back.from, &mut back.to);
    cyclic.edges.push(back);
    let mut invalid = cyclic.edges[0].clone();
    invalid.to = "missing".into();
    cyclic.edges.push(invalid);
    assert!(!cyclic
        .neighborhood("crate:node00/Cargo.toml")
        .contains("missing"));
    assert!(cyclic.neighborhood("bad-id").is_empty());
    assert!(graph
        .nodes
        .iter()
        .all(|n| n.manifest_path.ends_with(Path::new("Cargo.toml"))));
}

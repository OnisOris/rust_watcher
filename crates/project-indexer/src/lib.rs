use anyhow::{anyhow, Context, Result};
use cargo_metadata::{Metadata, MetadataCommand, Package};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectIndex {
    pub root: PathBuf,
    pub name: String,
    pub metadata: Metadata,
    pub packages: Vec<IndexedPackage>,
    pub files: Vec<IndexedFile>,
}

#[derive(Debug, Clone)]
pub struct IndexedPackage {
    pub name: String,
    pub manifest_path: PathBuf,
    pub package_root: PathBuf,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub absolute_path: PathBuf,
    pub relative_path: String,
    pub package_name: String,
    pub module_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLanguageManifest {
    pub root: PathBuf,
    pub has_cargo: bool,
    pub has_package_json: bool,
    pub has_pyproject: bool,
    pub has_qml: bool,
    pub rust_files: usize,
    pub python_files: usize,
    pub typescript_files: usize,
    pub javascript_files: usize,
    pub qml_files: usize,
    pub total_supported_files: usize,
}

pub fn scan_project_languages(root: impl AsRef<Path>) -> Result<ProjectLanguageManifest> {
    let root = root.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to canonicalize project root {}",
            root.as_ref().display()
        )
    })?;
    let mut manifest = ProjectLanguageManifest {
        has_cargo: root.join("Cargo.toml").is_file(),
        has_package_json: root.join("package.json").is_file(),
        has_pyproject: root.join("pyproject.toml").is_file(),
        root: root.clone(),
        has_qml: false,
        rust_files: 0,
        python_files: 0,
        typescript_files: 0,
        javascript_files: 0,
        qml_files: 0,
        total_supported_files: 0,
    };

    scan_supported_files(&root, &mut manifest)?;
    manifest.has_qml = manifest.qml_files > 0;
    manifest.total_supported_files = manifest.rust_files
        + manifest.python_files
        + manifest.typescript_files
        + manifest.javascript_files
        + manifest.qml_files;
    Ok(manifest)
}

pub fn index_project(root: impl AsRef<Path>) -> Result<ProjectIndex> {
    let root = root.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to canonicalize project root {}",
            root.as_ref().display()
        )
    })?;
    let manifest = root.join("Cargo.toml");
    if !manifest.exists() {
        return Err(anyhow!("No Cargo.toml found in project root."));
    }

    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .exec()
        .with_context(|| format!("failed to run cargo metadata for {}", manifest.display()))?;

    let workspace_root = metadata
        .workspace_root
        .as_std_path()
        .canonicalize()
        .unwrap_or_else(|_| metadata.workspace_root.as_std_path().to_path_buf());
    let is_workspace_root = workspace_root == root;
    let workspace_members: HashSet<_> = metadata.workspace_members.iter().cloned().collect();
    let packages: Vec<IndexedPackage> = metadata
        .packages
        .iter()
        .filter(|pkg| {
            if !workspace_members.contains(&pkg.id) {
                return false;
            }
            if is_workspace_root {
                return true;
            }
            let manifest_path = PathBuf::from(pkg.manifest_path.as_std_path());
            manifest_path
                .parent()
                .map(|package_root| package_root.starts_with(&root))
                .unwrap_or(false)
        })
        .map(indexed_package)
        .collect::<Result<_>>()?;

    let mut files = Vec::new();
    for package in &packages {
        let mut package_files = Vec::new();
        collect_rs_files(&package.package_root, &mut package_files)?;
        package_files.sort();
        for absolute_path in package_files {
            let relative_path = relative_to(&root, &absolute_path);
            let module_path = infer_module_path(&package.package_root, &absolute_path);
            files.push(IndexedFile {
                absolute_path,
                relative_path,
                package_name: package.name.clone(),
                module_path,
            });
        }
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();

    Ok(ProjectIndex {
        root,
        name,
        metadata,
        packages,
        files,
    })
}

pub fn start_watcher<F>(root: PathBuf, mut on_event: F) -> Result<RecommendedWatcher>
where
    F: FnMut(Event) + Send + 'static,
{
    let mut watcher = RecommendedWatcher::new(
        move |event: std::result::Result<Event, notify::Error>| match event {
            Ok(event) => {
                let interesting = event.paths.iter().any(|path| {
                    if is_ignored_path(path) {
                        return false;
                    }
                    let extension = path.extension().and_then(|e| e.to_str());
                    path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml")
                        || matches!(
                            extension,
                            Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "qml")
                        )
                });
                if interesting {
                    on_event(event);
                }
            }
            Err(error) => tracing::warn!(?error, "file watcher error"),
        },
        Config::default(),
    )?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn indexed_package(pkg: &Package) -> Result<IndexedPackage> {
    let manifest_path = PathBuf::from(pkg.manifest_path.as_std_path());
    let package_root = manifest_path
        .parent()
        .ok_or_else(|| {
            anyhow!(
                "package manifest has no parent: {}",
                manifest_path.display()
            )
        })?
        .to_path_buf();
    let dependencies = pkg
        .dependencies
        .iter()
        .map(|dep| dep.name.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    Ok(IndexedPackage {
        name: pkg.name.to_string(),
        manifest_path,
        package_root,
        dependencies,
    })
}

fn collect_rs_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if is_ignored_dir_name(file_name) {
            continue;
        }
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn scan_supported_files(root: &Path, manifest: &mut ProjectLanguageManifest) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !is_ignored_dir_name(file_name) {
                scan_supported_files(&path, manifest)?;
            }
            continue;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => manifest.rust_files += 1,
            Some("py") => manifest.python_files += 1,
            Some("ts" | "tsx") => manifest.typescript_files += 1,
            Some("js" | "jsx") => manifest.javascript_files += 1,
            Some("qml") => manifest.qml_files += 1,
            _ => {}
        }
    }
    Ok(())
}

pub fn is_ignored_path(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(is_ignored_dir_name)
    })
}

fn is_ignored_dir_name(name: &str) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | ".git"
            | "dist"
            | "build"
            | ".next"
            | ".cache"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "coverage"
            | ".vite"
    )
}

pub fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn infer_module_path(package_root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(package_root).unwrap_or(file);
    let mut components: Vec<String> = rel
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(ToOwned::to_owned)
        .collect();
    if components.first().map(String::as_str) == Some("src") {
        components.remove(0);
    }
    if let Some(last) = components.last_mut() {
        *last = last.trim_end_matches(".rs").to_string();
    }
    if components == ["main"] || components == ["lib"] {
        "crate root".to_string()
    } else {
        components
            .into_iter()
            .filter(|part| part != "mod" && part != "main" && part != "lib")
            .collect::<Vec<_>>()
            .join("::")
    }
}

pub fn files_by_package(files: &[IndexedFile]) -> HashMap<String, Vec<IndexedFile>> {
    let mut grouped: HashMap<String, Vec<IndexedFile>> = HashMap::new();
    for file in files {
        grouped
            .entry(file.package_name.clone())
            .or_default()
            .push(file.clone());
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_root(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rust-watcher-project-indexer-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(root: &Path, relative_path: &str) {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, "").unwrap();
    }

    #[test]
    fn generated_and_cache_paths_are_ignored() {
        assert!(is_ignored_path(Path::new(
            "target/debug/build/demo/out/private.rs"
        )));
        assert!(is_ignored_path(Path::new("node_modules/pkg/index.ts")));
        assert!(is_ignored_path(Path::new(".venv/lib/site.py")));
        assert!(!is_ignored_path(Path::new("src/main.rs")));
    }

    #[test]
    fn scan_project_languages_empty_project() {
        let root = test_root("empty");

        let manifest = scan_project_languages(&root).unwrap();

        assert_eq!(manifest.root, root.canonicalize().unwrap());
        assert!(!manifest.has_cargo);
        assert!(!manifest.has_package_json);
        assert!(!manifest.has_pyproject);
        assert!(!manifest.has_qml);
        assert_eq!(manifest.total_supported_files, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_project_languages_rust_project() {
        let root = test_root("rust");
        write_file(&root, "Cargo.toml");
        write_file(&root, "src/main.rs");
        write_file(&root, "src/lib.rs");

        let manifest = scan_project_languages(&root).unwrap();

        assert!(manifest.has_cargo);
        assert_eq!(manifest.rust_files, 2);
        assert_eq!(manifest.total_supported_files, 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_project_languages_mixed_project() {
        let root = test_root("mixed");
        for file in [
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "src/main.rs",
            "scripts/tool.py",
            "frontend/App.tsx",
            "frontend/api.ts",
            "frontend/index.jsx",
            "frontend/utils.js",
            "qml/Main.qml",
        ] {
            write_file(&root, file);
        }

        let manifest = scan_project_languages(&root).unwrap();

        assert!(manifest.has_cargo);
        assert!(manifest.has_package_json);
        assert!(manifest.has_pyproject);
        assert!(manifest.has_qml);
        assert_eq!(manifest.rust_files, 1);
        assert_eq!(manifest.python_files, 1);
        assert_eq!(manifest.typescript_files, 2);
        assert_eq!(manifest.javascript_files, 2);
        assert_eq!(manifest.qml_files, 1);
        assert_eq!(manifest.total_supported_files, 7);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_project_languages_skips_ignored_directories() {
        let root = test_root("ignored");
        for file in [
            "target/generated.rs",
            "node_modules/pkg/index.ts",
            ".git/hooks/test.py",
            ".venv/lib/test.py",
            "dist/bundle.js",
            "build/generated.qml",
            ".next/page.tsx",
            ".cache/file.rs",
            "__pycache__/cached.py",
            "coverage/report.js",
            ".vite/temp.ts",
        ] {
            write_file(&root, file);
        }

        let manifest = scan_project_languages(&root).unwrap();

        assert_eq!(manifest.total_supported_files, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_project_languages_detects_root_manifests_only() {
        let root = test_root("root-manifests");
        write_file(&root, "nested/Cargo.toml");
        write_file(&root, "nested/package.json");
        write_file(&root, "nested/pyproject.toml");

        let manifest = scan_project_languages(&root).unwrap();

        assert!(!manifest.has_cargo);
        assert!(!manifest.has_package_json);
        assert!(!manifest.has_pyproject);

        let _ = fs::remove_dir_all(root);
    }
}

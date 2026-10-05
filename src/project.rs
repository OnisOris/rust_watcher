use anyhow::{Context, Result};
use cargo_metadata::{Metadata, MetadataCommand};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ProjectNotFound(pub PathBuf);

impl std::fmt::Display for ProjectNotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "project path does not exist: {}",
            self.0.display()
        )
    }
}

impl std::error::Error for ProjectNotFound {}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub workspace_root: PathBuf,
    pub packages: Vec<Package>,
    pub rust_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Package {
    pub name: String,
    pub manifest_path: PathBuf,
    pub targets: Vec<Target>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub name: String,
    pub crate_root: PathBuf,
    pub kinds: Vec<String>,
}

impl Project {
    pub fn discover(path: &Path) -> Result<Self> {
        let manifest = find_manifest(path)?;
        let metadata = MetadataCommand::new()
            .manifest_path(&manifest)
            .no_deps()
            .exec()
            .with_context(|| format!("cargo metadata failed for {}", manifest.display()))?;
        Self::from_metadata(metadata)
    }

    fn from_metadata(metadata: Metadata) -> Result<Self> {
        let workspace_root = metadata.workspace_root.into_std_path_buf();
        let mut packages: Vec<_> = metadata
            .packages
            .into_iter()
            .filter(|package| metadata.workspace_members.contains(&package.id))
            .map(|package| Package {
                name: package.name.to_string(),
                manifest_path: package.manifest_path.into_std_path_buf(),
                targets: package
                    .targets
                    .into_iter()
                    .map(|target| Target {
                        name: target.name,
                        crate_root: target.src_path.into_std_path_buf(),
                        kinds: target
                            .kind
                            .into_iter()
                            .map(|kind| kind.to_string())
                            .collect(),
                    })
                    .collect(),
                dependencies: package
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.name)
                    .collect(),
            })
            .collect();
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        for package in &mut packages {
            package.targets.sort_by(|a, b| a.name.cmp(&b.name));
            package.dependencies.sort();
        }
        let mut rust_files = Vec::new();
        for package in &packages {
            if let Some(root) = package.manifest_path.parent() {
                collect_rust_files(root, root, &mut rust_files)?;
            }
        }
        rust_files.sort();
        rust_files.dedup();
        Ok(Self {
            workspace_root,
            packages,
            rust_files,
        })
    }

    pub fn binary_entrypoints(&self) -> Vec<PathBuf> {
        let mut roots: Vec<_> = self
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .filter(|target| target.kinds.iter().any(|kind| kind == "bin"))
            .map(|target| {
                target
                    .crate_root
                    .strip_prefix(&self.workspace_root)
                    .unwrap_or(&target.crate_root)
                    .to_path_buf()
            })
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }
}

pub fn find_manifest(path: &Path) -> Result<PathBuf> {
    let start = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProjectNotFound(path.to_path_buf()).into())
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to access project path: {}", path.display()))
        }
    };
    let mut directory = if start.is_file() {
        start.parent().unwrap_or(&start).to_path_buf()
    } else {
        start
    };
    loop {
        let candidate = directory.join("Cargo.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !directory.pop() {
            anyhow::bail!("Cargo.toml not found from {}", path.display());
        }
    }
}

fn collect_rust_files(
    package_root: &Path,
    directory: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name != "target" && name != ".git" {
                if path != package_root && path.join("Cargo.toml").is_file() {
                    continue;
                }
                collect_rust_files(package_root, &path, output)?;
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_manifest_in_parent() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let source = temp.path().join("src");
        std::fs::create_dir(&source).unwrap();
        assert_eq!(
            find_manifest(&source).unwrap(),
            temp.path().join("Cargo.toml")
        );
    }

    #[test]
    fn classifies_missing_project_path() {
        let error =
            find_manifest(Path::new("/definitely/missing/rust-watcher-project")).unwrap_err();
        assert!(error.downcast_ref::<ProjectNotFound>().is_some());
    }
}

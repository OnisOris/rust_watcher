//! Passive physical-line metrics over exactly the repository scanner's file set.
use crate::project::Project;
use crate::repository::{RepoEntryKind, RepositoryTree};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod stream;
use stream::{ReadFailure, MAX_METRIC_FILE_BYTES, MAX_TOTAL_READ_BYTES};
pub const LARGEST_FILE_LIMIT: usize = 10;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryMetrics {
    pub total_files: usize,
    pub total_directories: usize,
    pub symlinks: usize,
    pub text_files: usize,
    pub code_files: usize,
    pub test_files: usize,
    pub test_nonblank_lines: u64,
    pub physical_lines: u64,
    pub blank_lines: u64,
    pub nonblank_lines: u64,
    pub code_physical_lines: u64,
    pub code_nonblank_lines: u64,
    pub languages: Vec<LanguageMetrics>,
    pub categories: Vec<CategoryMetrics>,
    pub packages: Vec<PackageMetrics>,
    pub largest_files: Vec<FileMetrics>,
    pub unmeasured_large_files: Vec<UnmeasuredFile>,
    pub read_bytes: u64,
    pub skipped: SkippedMetrics,
    pub partial: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedMetrics {
    pub binary: usize,
    pub invalid_utf8: usize,
    pub oversized: usize,
    pub unreadable: usize,
    pub scan_errors: usize,
    pub total_read_budget: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmeasuredFile {
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageMetrics {
    pub language: String,
    pub files: usize,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryMetrics {
    pub category: Category,
    pub files: usize,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMetrics {
    pub name: String,
    pub root: PathBuf,
    pub files: usize,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
    pub test_files: usize,
    pub source_files: usize,
    pub source_physical_lines: u64,
    pub source_nonblank_lines: u64,
    pub test_nonblank_lines: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetrics {
    pub path: PathBuf,
    pub language: String,
    pub category: Category,
    pub physical_lines: u64,
    pub blank_lines: u64,
    pub nonblank_lines: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Source,
    Tests,
    Configuration,
    Documentation,
    Other,
}

pub(crate) fn classify(path: &Path) -> (&'static str, Category) {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let (language, mut category) = match extension {
        "rs" => ("Rust", Category::Source),
        "ts" | "tsx" => ("TypeScript", Category::Source),
        "js" | "jsx" => ("JavaScript", Category::Source),
        "py" => ("Python", Category::Source),
        "c" | "h" => ("C", Category::Source),
        "cpp" | "cc" | "cxx" | "hpp" => ("C++", Category::Source),
        "go" => ("Go", Category::Source),
        "java" => ("Java", Category::Source),
        "sh" => ("Shell", Category::Source),
        "toml" => ("TOML", Category::Configuration),
        "json" => ("JSON", Category::Configuration),
        "yaml" | "yml" => ("YAML", Category::Configuration),
        "md" => ("Markdown", Category::Documentation),
        _ => ("Other", Category::Other),
    };
    if category == Category::Source
        && (path.components().any(|c| c.as_os_str() == "tests")
            || path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                s.ends_with("_test.rs")
                    || s.ends_with("_test.go")
                    || s.ends_with(".test.ts")
                    || s.ends_with(".test.tsx")
                    || s.ends_with(".test.js")
                    || s.ends_with(".test.jsx")
                    || (extension == "py" && s.starts_with("test_"))
            }))
    {
        category = Category::Tests;
    }
    (language, category)
}

pub fn analyze(root: &Path, tree: &RepositoryTree, project: &Project) -> RepositoryMetrics {
    analyze_until(root, tree, project, || false)
}

pub fn analyze_until(
    root: &Path,
    tree: &RepositoryTree,
    project: &Project,
    cancelled: impl Fn() -> bool,
) -> RepositoryMetrics {
    analyze_budget(root, tree, project, cancelled, MAX_TOTAL_READ_BYTES)
}

fn analyze_budget(
    root: &Path,
    tree: &RepositoryTree,
    project: &Project,
    cancelled: impl Fn() -> bool,
    total_budget: u64,
) -> RepositoryMetrics {
    let mut result = RepositoryMetrics {
        partial: tree.truncated || tree.skipped_errors > 0,
        skipped: SkippedMetrics {
            scan_errors: tree.skipped_errors,
            ..Default::default()
        },
        packages: project
            .packages
            .iter()
            .filter_map(|p| {
                let root = p
                    .manifest_path
                    .parent()?
                    .strip_prefix(root)
                    .ok()?
                    .to_path_buf();
                Some(PackageMetrics {
                    name: p.name.clone(),
                    root,
                    ..Default::default()
                })
            })
            .collect(),
        ..Default::default()
    };
    let mut languages = BTreeMap::<String, LanguageMetrics>::new();
    let mut read_budget = total_budget;
    result.categories = [
        Category::Source,
        Category::Tests,
        Category::Configuration,
        Category::Documentation,
        Category::Other,
    ]
    .into_iter()
    .map(|category| CategoryMetrics {
        category,
        files: 0,
        physical_lines: 0,
        nonblank_lines: 0,
    })
    .collect();
    for entry in &tree.entries {
        if cancelled() {
            result.partial = true;
            break;
        }
        match entry.kind {
            RepoEntryKind::Directory => {
                result.total_directories += 1;
                continue;
            }
            RepoEntryKind::Symlink => {
                result.symlinks += 1;
                continue;
            }
            RepoEntryKind::File => result.total_files += 1,
        }
        let (language, category) = classify(&entry.path);
        let category_metrics = result
            .categories
            .iter_mut()
            .find(|c| c.category == category)
            .expect("all categories initialized");
        category_metrics.files += 1;
        let code = matches!(category, Category::Source | Category::Tests);
        result.code_files += usize::from(code);
        result.test_files += usize::from(category == Category::Tests);
        let package = result
            .packages
            .iter_mut()
            .filter(|p| entry.path.starts_with(&p.root))
            .max_by_key(|p| p.root.components().count());
        let mut package = package;
        if let Some(p) = package.as_mut() {
            p.files += 1;
            p.test_files += usize::from(category == Category::Tests);
            p.source_files += usize::from(code);
        }
        let lang = languages
            .entry(language.into())
            .or_insert_with(|| LanguageMetrics {
                language: language.into(),
                ..Default::default()
            });
        lang.files += 1;
        let counted = if entry.size.is_some_and(|size| size > MAX_METRIC_FILE_BYTES) {
            Err(ReadFailure::PerFileBudget)
        } else {
            stream::count(&root.join(&entry.path), &mut read_budget, &cancelled)
        };
        let lines = match counted {
            Ok(lines) => lines,
            Err(reason) => {
                result.partial |= reason != ReadFailure::Binary;
                match reason {
                    ReadFailure::Binary => result.skipped.binary += 1,
                    ReadFailure::InvalidUtf8 => result.skipped.invalid_utf8 += 1,
                    ReadFailure::Unreadable => result.skipped.unreadable += 1,
                    ReadFailure::PerFileBudget | ReadFailure::TotalReadBudget => {
                        if reason == ReadFailure::PerFileBudget {
                            result.skipped.oversized += 1;
                        } else {
                            result.skipped.total_read_budget += 1;
                        }
                        result.unmeasured_large_files.push(UnmeasuredFile {
                            path: entry.path.clone(),
                            size_bytes: std::fs::metadata(root.join(&entry.path))
                                .ok()
                                .map(|m| m.len())
                                .or(entry.size),
                            reason: if reason == ReadFailure::PerFileBudget {
                                "perFileBudget"
                            } else {
                                "totalReadBudget"
                            },
                        });
                        result.unmeasured_large_files.sort_by(|a, b| {
                            b.size_bytes.cmp(&a.size_bytes).then(a.path.cmp(&b.path))
                        });
                        result.unmeasured_large_files.truncate(LARGEST_FILE_LIMIT);
                    }
                    ReadFailure::Cancelled => break,
                }
                continue;
            }
        };
        let physical_lines = lines.physical;
        let blank_lines = lines.blank;
        let nonblank_lines = physical_lines - blank_lines;
        result.text_files += 1;
        result.physical_lines += physical_lines;
        result.blank_lines += blank_lines;
        result.nonblank_lines += nonblank_lines;
        category_metrics.physical_lines += physical_lines;
        category_metrics.nonblank_lines += nonblank_lines;
        lang.physical_lines += physical_lines;
        lang.nonblank_lines += nonblank_lines;
        if let Some(p) = package {
            p.physical_lines += physical_lines;
            p.nonblank_lines += nonblank_lines;
            if code {
                p.source_physical_lines += physical_lines;
                p.source_nonblank_lines += nonblank_lines;
            }
            if category == Category::Tests {
                p.test_nonblank_lines += nonblank_lines;
            }
        }
        if code {
            result.code_physical_lines += physical_lines;
            result.code_nonblank_lines += nonblank_lines;
            if category == Category::Tests {
                result.test_nonblank_lines += nonblank_lines;
            }
            result.largest_files.push(FileMetrics {
                path: entry.path.clone(),
                language: language.into(),
                category,
                physical_lines,
                blank_lines,
                nonblank_lines,
            });
            result.largest_files.sort_by(|a, b| {
                b.nonblank_lines
                    .cmp(&a.nonblank_lines)
                    .then(a.path.cmp(&b.path))
            });
            result.largest_files.truncate(LARGEST_FILE_LIMIT);
        }
    }
    result.languages = languages.into_values().collect();
    result.read_bytes = total_budget - read_budget;
    result.packages.sort_by(|a, b| {
        b.physical_lines
            .cmp(&a.physical_lines)
            .then(a.name.cmp(&b.name))
            .then(a.root.cmp(&b.root))
    });
    result
}

#[cfg(test)]
mod tests;

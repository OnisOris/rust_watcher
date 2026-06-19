use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::QmlFile;

pub(super) fn resolve_qml_import(
    from_file: &str,
    module: &str,
    files_by_path: &HashMap<String, &QmlFile>,
) -> Vec<String> {
    let module = module.trim_matches(['"', '\'']);
    if !(module.starts_with("./") || module.starts_with("../")) {
        return Vec::new();
    }
    let base = Path::new(from_file)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(module);
    let normalized = normalize_relative_path(&base);
    files_by_path
        .keys()
        .filter(|file| {
            file.starts_with(&format!("{normalized}/")) || *file == &format!("{normalized}.qml")
        })
        .cloned()
        .collect()
}

pub(super) fn resolve_qml_component(
    component: &str,
    files_by_component: &HashMap<String, String>,
) -> Option<String> {
    files_by_component.get(component).cloned()
}

fn normalize_relative_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(part) => {
                if let Some(part) = part.to_str() {
                    parts.push(part.to_string());
                }
            }
            _ => {}
        }
    }
    PathBuf::from_iter(parts).display().to_string()
}

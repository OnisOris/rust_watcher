use graph_core::{LanguageId, NodeType};
use project_indexer::IndexedFile;
use std::path::Path;

pub(crate) fn crate_id(name: &str) -> String {
    format!("crate:{name}")
}

pub(crate) fn external_id(name: &str) -> String {
    format!("external:{name}")
}

pub(crate) fn file_id(path: &str) -> String {
    format!("file:{path}")
}

pub(crate) fn language_for_ts_path(path: &str) -> LanguageId {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("js" | "jsx") => LanguageId::JavaScript,
        _ => LanguageId::TypeScript,
    }
}

pub(crate) fn language_for_file(path: &str) -> Option<String> {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("rs") => Some(LanguageId::Rust.to_string()),
        Some("py") => Some(LanguageId::Python.to_string()),
        Some("ts" | "tsx") => Some(LanguageId::TypeScript.to_string()),
        Some("js" | "jsx") => Some(LanguageId::JavaScript.to_string()),
        _ => None,
    }
}

pub(crate) fn infer_node_language(
    node_type: NodeType,
    file: Option<&str>,
    module: Option<&str>,
    crate_name: Option<&str>,
) -> Option<String> {
    if let Some(language) = file.and_then(language_for_file) {
        return Some(language);
    }
    if module.is_some_and(|module| module.contains("typescript"))
        || crate_name == Some("frontend")
        || matches!(
            node_type,
            NodeType::Component | NodeType::Hook | NodeType::Interface | NodeType::TypeAlias
        )
    {
        return Some(LanguageId::TypeScript.to_string());
    }
    if matches!(node_type, NodeType::ExternalCrate)
        || crate_name.is_some_and(|crate_name| crate_name != "frontend")
    {
        return Some(LanguageId::Rust.to_string());
    }
    None
}

pub(crate) fn symbol_id(node_type: NodeType, file: &IndexedFile, name: &str, line: u32) -> String {
    let prefix = match node_type {
        NodeType::Struct => "struct",
        NodeType::Class => "class",
        NodeType::Enum => "enum",
        NodeType::Trait => "trait",
        NodeType::Impl => "impl",
        NodeType::Function => "fn",
        NodeType::Method => "method",
        NodeType::Component => "component",
        NodeType::Hook => "hook",
        NodeType::Interface => "interface",
        NodeType::TypeAlias => "type",
        NodeType::Endpoint => "endpoint",
        NodeType::Macro => "macro",
        NodeType::Module => "module",
        NodeType::File => "file",
        NodeType::ExternalCrate => "external",
    };
    format!(
        "{prefix}:{}::{}::{}@{}",
        file.package_name, file.module_path, name, line
    )
}

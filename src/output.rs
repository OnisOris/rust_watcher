use crate::model::{CallNode, Diagnostic, Explanation, Location, ProjectSummary, Severity, Symbol};
use anyhow::Result;
use serde::Serialize;

pub fn json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

pub fn summary(value: &ProjectSummary) {
    println!("rust_watcher\n\nWorkspace\n  root:        {}\n  crates:      {}\n  files:       {}\n  symbols:     {}\n\nDiagnostics\n  errors:      {}\n  warnings:    {}", value.workspace_root.display(), value.crates, value.files, value.symbols, value.errors, value.warnings);
    if !value.entrypoints.is_empty() {
        println!("\nEntrypoints");
        for entrypoint in &value.entrypoints {
            println!(
                "  {}:{}",
                entrypoint.file.display(),
                entrypoint.range.start.line + 1
            );
        }
    }
}

pub fn symbols(values: &[Symbol]) {
    if values.is_empty() {
        println!("No symbols found.");
        return;
    }
    for (index, symbol) in values.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!(
            "{}\n  kind: {}\n  file: {}:{}",
            symbol.name,
            symbol.kind,
            symbol.file.display(),
            symbol.range.start.line + 1
        );
        if let Some(container) = &symbol.container {
            println!("  container: {container}");
        }
    }
}

pub fn locations(values: &[Location]) {
    if values.is_empty() {
        println!("No locations found.");
        return;
    }
    for location in values {
        println!(
            "{}:{}:{}",
            location.file.display(),
            location.range.start.line + 1,
            location.range.start.character + 1
        );
    }
}

pub fn diagnostics(values: &[Diagnostic]) {
    if values.is_empty() {
        println!("No diagnostics reported after rust-analyzer finished indexing.");
        return;
    }
    for diagnostic in values {
        println!(
            "{}:{}:{}: {}: {}",
            diagnostic.file.display(),
            diagnostic.range.start.line + 1,
            diagnostic.range.start.character + 1,
            severity(diagnostic.severity),
            diagnostic.message
        );
    }
}

pub fn calls(root: &str, values: &[CallNode]) {
    println!("{root}");
    for (index, node) in values.iter().enumerate() {
        call_node(node, "", index + 1 == values.len());
    }
}

fn call_node(node: &CallNode, prefix: &str, last: bool) {
    println!(
        "{prefix}{} {}  ({}:{})",
        if last { "└──" } else { "├──" },
        node.name,
        node.location.file.display(),
        node.location.range.start.line + 1
    );
    let next = format!("{prefix}{}", if last { "    " } else { "│   " });
    for (index, child) in node.children.iter().enumerate() {
        call_node(child, &next, index + 1 == node.children.len());
    }
}

pub fn explanation(value: &Explanation) {
    println!(
        "{}\n  kind: {}\n  file: {}:{}",
        value.symbol.name,
        value.symbol.kind,
        value.symbol.file.display(),
        value.symbol.range.start.line + 1
    );
    if let Some(signature) = &value.signature {
        println!("  signature: {signature}");
    }
    println!("  references: {}", value.references);
    if let Some(definition) = &value.definition {
        println!(
            "  definition: {}:{}",
            definition.file.display(),
            definition.range.start.line + 1
        );
    }
    if let Some(hover) = &value.hover {
        println!("\nHover\n{hover}");
    }
    println!("\nSource\n{}", value.source);
    println!("\nCallers");
    if value.callers.is_empty() {
        println!("  (none)");
    } else {
        for (index, node) in value.callers.iter().enumerate() {
            call_node(node, "", index + 1 == value.callers.len());
        }
    }
    println!("\nCallees");
    if value.callees.is_empty() {
        println!("  (none)");
    } else {
        for (index, node) in value.callees.iter().enumerate() {
            call_node(node, "", index + 1 == value.callees.len());
        }
    }
    if !value.diagnostics.is_empty() {
        println!("\nDiagnostics");
        diagnostics(&value.diagnostics);
    }
}

fn severity(value: Severity) -> &'static str {
    match value {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Information => "info",
        Severity::Hint => "hint",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_locations_are_compact_and_stable() {
        let value = Location {
            file: "src/main.rs".into(),
            range: crate::model::Range {
                start: crate::model::Position {
                    line: 1,
                    character: 2,
                },
                end: crate::model::Position {
                    line: 1,
                    character: 5,
                },
            },
        };
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"file":"src/main.rs","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":5}}}"#
        );
    }
}

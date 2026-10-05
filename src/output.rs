use crate::lsp::AnalyzerNotFound;
use crate::model::{
    CallNode, CallTree, Diagnostic, Explanation, Location, ProjectSummary, Severity, Symbol,
    SymbolSearchResult,
};
use crate::project::ProjectNotFound;
use crate::rust::ResolveError;
use anyhow::Result;
use serde::Serialize;

pub fn json<T: Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&SuccessEnvelope {
            schema_version: 1,
            ok: true,
            data: value
        })?
    );
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SuccessEnvelope<'a, T> {
    schema_version: u8,
    ok: bool,
    data: &'a T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    schema_version: u8,
    ok: bool,
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<Symbol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated: Option<bool>,
}

pub fn json_error(error: &anyhow::Error) {
    let body = error_body(error);
    let envelope = ErrorEnvelope {
        schema_version: 1,
        ok: false,
        error: body,
    };
    println!(
        "{}",
        serde_json::to_string(&envelope).expect("error envelope should serialize")
    );
}

fn error_body(error: &anyhow::Error) -> ErrorBody {
    let (code, candidates, truncated) = if let Some(error) = error.downcast_ref::<ResolveError>() {
        match error {
            ResolveError::NotFound { .. } => ("symbol_not_found", Vec::new(), None),
            ResolveError::Ambiguous {
                candidates,
                truncated,
                ..
            } => ("symbol_ambiguous", candidates.clone(), Some(*truncated)),
        }
    } else if error.downcast_ref::<ProjectNotFound>().is_some() {
        ("project_not_found", Vec::new(), None)
    } else if error.downcast_ref::<AnalyzerNotFound>().is_some() {
        ("analyzer_not_found", Vec::new(), None)
    } else if error
        .to_string()
        .contains("timed out waiting for rust-analyzer diagnostics")
    {
        ("diagnostics_timeout", Vec::new(), None)
    } else if error
        .to_string()
        .contains("timed out waiting for rust-analyzer readiness")
    {
        ("analyzer_timeout", Vec::new(), None)
    } else {
        ("operation_failed", Vec::new(), None)
    };
    ErrorBody {
        code,
        message: format!("{error:#}"),
        candidates,
        truncated,
    }
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

pub fn symbols(result: &SymbolSearchResult) {
    if result.items.is_empty() {
        println!("No symbols found.");
        return;
    }
    for (index, symbol) in result.items.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!(
            "{}\n  kind: {}\n  file: {}:{}",
            symbol.name,
            symbol.kind,
            symbol.file.display(),
            symbol.selection_range.start.line + 1
        );
        if let Some(container) = &symbol.container {
            println!("  container: {container}");
        }
    }
    if result.truncated {
        println!(
            "\nShowing first {} matching symbols; more were omitted.",
            result.items.len()
        );
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

pub fn calls(root: &str, tree: &CallTree) {
    println!("{root}");
    for (index, node) in tree.nodes.iter().enumerate() {
        call_node(node, "", index + 1 == tree.nodes.len());
    }
    if tree.truncated {
        println!("… output truncated at the call hierarchy node limit");
    }
}

fn call_node(node: &CallNode, prefix: &str, last: bool) {
    println!(
        "{prefix}{} {}  ({}:{}){}",
        if last { "└──" } else { "├──" },
        node.name,
        node.location.file.display(),
        node.location.range.start.line + 1,
        if node.cycle { "  [cycle]" } else { "" }
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
        value.symbol.selection_range.start.line + 1
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
    if value.callers.nodes.is_empty() {
        println!("  (none)");
    } else {
        for (index, node) in value.callers.nodes.iter().enumerate() {
            call_node(node, "", index + 1 == value.callers.nodes.len());
        }
    }
    if value.callers.truncated {
        println!("  … truncated");
    }
    println!("\nCallees");
    if value.callees.nodes.is_empty() {
        println!("  (none)");
    } else {
        for (index, node) in value.callees.nodes.iter().enumerate() {
            call_node(node, "", index + 1 == value.callees.nodes.len());
        }
    }
    if value.callees.truncated {
        println!("  … truncated");
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

    #[test]
    fn classifies_missing_analyzer_separately_from_missing_projects() {
        let analyzer = anyhow::Error::new(AnalyzerNotFound("missing-ra".into()));
        let project = anyhow::Error::new(ProjectNotFound("missing-project".into()));
        assert_eq!(error_body(&analyzer).code, "analyzer_not_found");
        assert_eq!(error_body(&project).code, "project_not_found");
    }
}

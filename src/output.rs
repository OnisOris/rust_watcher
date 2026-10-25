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
    if let Some(selection) = error.downcast_ref::<crate::module_graph::SelectionError>() {
        println!(
            "{}",
            serde_json::json!({"schemaVersion":1,"ok":false,"error":{"code":"module_selection","message":selection.message,"candidates":selection.candidates}})
        );
        return;
    }
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
    println!("wt\n\nWorkspace\n  root:        {}\n  crates:      {}\n  files:       {}\n  symbols:     {}\n\nDiagnostics\n  errors:      {}\n  warnings:    {}", value.workspace_root.display(), value.crates, value.files, value.symbols, value.errors, value.warnings);
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

pub fn metrics(m: &crate::metrics::RepositoryMetrics) {
    println!("Repository Metrics{}\n\nFiles                {}\nSource files         {}\nPhysical LOC         {}\nNonblank LOC         {}\nSource physical LOC  {}\nSource nonblank LOC  {}\nTest files           {}\nTest nonblank LOC    {}", if m.partial { " (partial)" } else { "" }, m.total_files, m.code_files, m.physical_lines, m.nonblank_lines, m.code_physical_lines, m.code_nonblank_lines, m.test_files, m.test_nonblank_lines);
    println!("\nLanguages (nonblank lines)");
    for l in &m.languages {
        println!(
            "  {:16} {:8}  {} files",
            l.language, l.nonblank_lines, l.files
        );
    }
    println!("\nCargo packages (physical lines)");
    for p in &m.packages {
        println!("  {:16} {:8}  {} files", p.name, p.physical_lines, p.files);
    }
    println!("\nLargest source files (nonblank lines)");
    for f in &m.largest_files {
        println!("  {}  {}", f.path.display(), f.nonblank_lines);
    }
    if !m.unmeasured_large_files.is_empty() {
        println!("\nUnmeasured files (LOC unknown)");
    }
    for f in &m.unmeasured_large_files {
        println!(
            "  {}  {} bytes  {}",
            f.path.display(),
            f.size_bytes
                .map_or_else(|| "unknown".into(), |n| n.to_string()),
            f.reason
        );
    }
    println!(
        "\nSkipped: binary {}, invalid UTF-8 {}, oversized {}, unreadable {}, scan errors {}, total-read budget {}",
        m.skipped.binary,
        m.skipped.invalid_utf8,
        m.skipped.oversized,
        m.skipped.unreadable,
        m.skipped.scan_errors,
        m.skipped.total_read_budget
    );
}

pub fn maintainability(r: &crate::maintainability::MaintainabilityReport) {
    println!("Rust Maintainability{}\n\nFunctions {}\nMedian length {} lines\nP95 length {} lines\nMax control depth {}\nParsed files {} / {}\nParse failures {} · skipped {}", if r.partial { " (partial)" } else { "" }, r.functions, r.median_function_lines, r.p95_function_lines, r.max_control_depth, r.parsed_files, r.analyzed_files, r.parse_failures, r.skipped_files);
    for (title, functions) in [
        ("LONGEST FUNCTIONS", &r.longest_functions),
        ("DEEPEST CONTROL FLOW", &r.deepest_functions),
    ] {
        println!("\n{title}");
        for f in functions {
            println!(
                "  {}{}  {}:{}  {} lines · depth {} · {} control nodes",
                f.qualified_name,
                if f.test_classified { " [test]" } else { "" },
                f.path.display(),
                f.start_line,
                f.physical_lines,
                f.max_control_depth,
                f.control_flow_nodes
            );
        }
    }
    println!("\nCargo packages");
    for p in &r.packages {
        println!(
            "  {}: {} functions · median {} · p95 {} · depth {}",
            p.name,
            p.function_count,
            p.median_function_lines,
            p.p95_function_lines,
            p.max_control_depth
        );
    }
    for f in &r.failures {
        println!("  {}: {}", f.file.display(), f.reason);
    }
    println!(
        "\nSize and nesting are review signals, not defect or quality scores. Lines are 1-based."
    );
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

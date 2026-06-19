use graph_core::{NodeType, TextPosition, TextRange};

use super::{QmlImport, QmlRelationshipFact, QmlSymbol};

#[derive(Debug, Clone)]
struct ObjectFrame {
    id: String,
    depth: i32,
}

pub(crate) fn parse_qml_source(
    relative_path: &str,
    source: &str,
) -> (Vec<QmlSymbol>, Vec<QmlImport>, Vec<QmlRelationshipFact>) {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut facts = Vec::new();
    let mut stack: Vec<ObjectFrame> = Vec::new();

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line_no = line_idx as u32 + 1;
        let trimmed = strip_comment(raw_line).trim();
        while stack.last().is_some_and(|frame| {
            frame.depth > 0 && brace_depth_before_line(source, line_idx) < frame.depth
        }) {
            stack.pop();
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some(module) = parse_import(trimmed) {
            imports.push(QmlImport { module });
        }

        let parent_id = stack.last().map(|frame| frame.id.clone());
        if let Some((name, character)) = parse_object_declaration(trimmed, raw_line) {
            let id = qml_symbol_id(NodeType::Object, relative_path, &name, line_no);
            symbols.push(qml_symbol(
                id.clone(),
                name.clone(),
                NodeType::Object,
                relative_path,
                raw_line,
                line_no,
                character,
                parent_id.clone(),
            ));
            if let Some(parent) = stack.last() {
                facts.push(QmlRelationshipFact::ComponentUse {
                    source_id: parent.id.clone(),
                    component: name.clone(),
                });
            }
            stack.push(ObjectFrame {
                id,
                depth: brace_depth_before_line(source, line_idx) + brace_delta(raw_line),
            });
            continue;
        }

        let owner_id = stack.last().map(|frame| frame.id.clone());
        if let Some((name, character)) = parse_property(trimmed, raw_line) {
            let id = qml_symbol_id(NodeType::Property, relative_path, &name, line_no);
            symbols.push(qml_symbol(
                id.clone(),
                name.clone(),
                NodeType::Property,
                relative_path,
                raw_line,
                line_no,
                character,
                owner_id.clone(),
            ));
            collect_binding_facts(&mut facts, &id, trimmed);
            continue;
        }
        if let Some((name, character)) = parse_signal(trimmed, raw_line) {
            let id = qml_symbol_id(NodeType::Signal, relative_path, &name, line_no);
            symbols.push(qml_symbol(
                id,
                name,
                NodeType::Signal,
                relative_path,
                raw_line,
                line_no,
                character,
                owner_id,
            ));
            continue;
        }
        if let Some((name, character)) = parse_handler(trimmed, raw_line) {
            let id = qml_symbol_id(NodeType::Handler, relative_path, &name, line_no);
            symbols.push(qml_symbol(
                id.clone(),
                name.clone(),
                NodeType::Handler,
                relative_path,
                raw_line,
                line_no,
                character,
                owner_id.clone(),
            ));
            collect_call_facts(&mut facts, &id, trimmed);
            collect_api_facts(&mut facts, &id, trimmed);
            continue;
        }
        if let Some((name, character)) = parse_function(trimmed, raw_line) {
            let id = qml_symbol_id(NodeType::Function, relative_path, &name, line_no);
            symbols.push(qml_symbol(
                id.clone(),
                name.clone(),
                NodeType::Function,
                relative_path,
                raw_line,
                line_no,
                character,
                owner_id,
            ));
            collect_api_facts(&mut facts, &id, trimmed);
            continue;
        }

        if let Some(owner) = stack.last() {
            collect_binding_facts(&mut facts, &owner.id, trimmed);
            collect_call_facts(&mut facts, &owner.id, trimmed);
            collect_api_facts(&mut facts, &owner.id, trimmed);
        }
    }

    for idx in 0..symbols.len() {
        if symbols[idx].parent_id.is_none() && symbols[idx].node_type != NodeType::Object {
            symbols[idx].parent_id = symbols
                .iter()
                .find(|symbol| symbol.node_type == NodeType::Object)
                .map(|symbol| symbol.id.clone());
        }
    }

    (symbols, imports, facts)
}

#[allow(clippy::too_many_arguments)]
fn qml_symbol(
    id: String,
    label: String,
    node_type: NodeType,
    _relative_path: &str,
    raw_line: &str,
    line: u32,
    character: u32,
    parent_id: Option<String>,
) -> QmlSymbol {
    QmlSymbol {
        id,
        label,
        node_type,
        line,
        character,
        range: line_range(line, raw_line.chars().count() as u32),
        selection_range: line_range(line, raw_line.chars().count() as u32),
        signature: raw_line.trim().to_string(),
        parent_id,
    }
}

pub(crate) fn qml_symbol_id(
    node_type: NodeType,
    relative_path: &str,
    name: &str,
    line: u32,
) -> String {
    let prefix = match node_type {
        NodeType::Object => "qml-object",
        NodeType::Property => "qml-property",
        NodeType::Signal => "qml-signal",
        NodeType::Handler => "qml-handler",
        NodeType::Function => "qml-fn",
        _ => "qml-symbol",
    };
    format!("{prefix}:{relative_path}::{name}@{line}")
}

fn parse_import(line: &str) -> Option<String> {
    line.strip_prefix("import ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches(['"', '\'']).to_string())
}

fn parse_object_declaration(trimmed: &str, raw_line: &str) -> Option<(String, u32)> {
    if !trimmed.contains('{') {
        return None;
    }
    let name = trimmed.split('{').next()?.trim();
    if name.contains(' ') || name.contains(':') || name.starts_with("function") {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    if !first.is_ascii_uppercase() || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return None;
    }
    Some((name.to_string(), raw_line.find(name).unwrap_or(0) as u32))
}

fn parse_property(trimmed: &str, raw_line: &str) -> Option<(String, u32)> {
    let rest = trimmed.strip_prefix("property ")?;
    let mut parts = rest.split_whitespace();
    let _ty = parts.next()?;
    let name = parts.next()?.trim_end_matches(':');
    valid_identifier(name).then(|| (name.to_string(), raw_line.find(name).unwrap_or(0) as u32))
}

fn parse_signal(trimmed: &str, raw_line: &str) -> Option<(String, u32)> {
    let rest = trimmed.strip_prefix("signal ")?;
    let name = rest
        .split(|ch: char| ch == '(' || ch.is_whitespace())
        .next()
        .unwrap_or_default();
    valid_identifier(name).then(|| (name.to_string(), raw_line.find(name).unwrap_or(0) as u32))
}

fn parse_handler(trimmed: &str, raw_line: &str) -> Option<(String, u32)> {
    let (name, _) = trimmed.split_once(':')?;
    let name = name.trim();
    (name.starts_with("on")
        && name
            .chars()
            .nth(2)
            .map(|ch| ch.is_ascii_uppercase())
            .unwrap_or(false))
    .then(|| (name.to_string(), raw_line.find(name).unwrap_or(0) as u32))
}

fn parse_function(trimmed: &str, raw_line: &str) -> Option<(String, u32)> {
    let rest = trimmed.strip_prefix("function ")?;
    let name = rest.split('(').next()?.trim();
    valid_identifier(name).then(|| (name.to_string(), raw_line.find(name).unwrap_or(0) as u32))
}

fn collect_call_facts(facts: &mut Vec<QmlRelationshipFact>, source_id: &str, line: &str) {
    for name in call_names(line) {
        facts.push(QmlRelationshipFact::Call {
            source_id: source_id.to_string(),
            target_name: name,
        });
    }
}

fn collect_binding_facts(facts: &mut Vec<QmlRelationshipFact>, source_id: &str, line: &str) {
    if !line.contains(':') {
        return;
    }
    for reference in id_references(line) {
        facts.push(QmlRelationshipFact::Use {
            source_id: source_id.to_string(),
            target_name: reference,
        });
    }
}

fn collect_api_facts(facts: &mut Vec<QmlRelationshipFact>, source_id: &str, line: &str) {
    for (method, path) in api_calls(line) {
        facts.push(QmlRelationshipFact::ApiCall {
            source_id: source_id.to_string(),
            method,
            path,
        });
    }
}

fn call_names(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (idx, _) in line.char_indices().filter(|(_, ch)| *ch == '(') {
        let name = line[..idx]
            .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
            .next()
            .unwrap_or_default()
            .rsplit('.')
            .next()
            .unwrap_or_default();
        if valid_identifier(name) && !matches!(name, "if" | "for" | "while" | "fetch") {
            names.push(name.to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn id_references(line: &str) -> Vec<String> {
    line.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
        .filter_map(|part| part.split_once('.').map(|(name, _)| name))
        .filter(|name| valid_identifier(name))
        .map(ToOwned::to_owned)
        .collect()
}

fn api_calls(line: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    for (path, start) in extract_api_paths(line) {
        let method = xhr_method_before_path(line, start).unwrap_or_else(|| "GET".to_string());
        calls.push((method.to_ascii_uppercase(), path));
    }
    calls
}

fn extract_api_paths(line: &str) -> Vec<(String, usize)> {
    let mut paths = Vec::new();
    let mut rest = line;
    let mut offset = 0usize;
    while let Some(start) = rest.find("/api/") {
        let absolute_start = offset + start;
        let after = &rest[start..];
        let path = after
            .split(['"', '\'', '`', ')', ',', ';', '}', ' '])
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        if !path.is_empty() {
            paths.push((path.to_string(), absolute_start));
        }
        let advance = start + path.len().max(1);
        offset += advance;
        rest = &rest[advance..];
    }
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    paths.dedup_by(|left, right| left.0 == right.0);
    paths
}

fn xhr_method_before_path(line: &str, path_start: usize) -> Option<String> {
    let before = &line[..path_start];
    let xhr_start = before.rfind("xhr.open")?;
    let call = &before[xhr_start..];
    let quote_idx = call.find(['"', '\''])?;
    let quote = call.as_bytes()[quote_idx] as char;
    let rest = &call[quote_idx + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn strip_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

fn brace_delta(line: &str) -> i32 {
    line.chars().filter(|ch| *ch == '{').count() as i32
        - line.chars().filter(|ch| *ch == '}').count() as i32
}

fn brace_depth_before_line(source: &str, line_idx: usize) -> i32 {
    source.lines().take(line_idx).map(brace_delta).sum()
}

fn line_range(one_based_line: u32, end_character: u32) -> TextRange {
    let line = one_based_line.saturating_sub(1);
    TextRange {
        start: TextPosition { line, character: 0 },
        end: TextPosition {
            line,
            character: end_character,
        },
    }
}

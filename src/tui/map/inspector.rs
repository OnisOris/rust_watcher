use super::*;
use std::path::Path;
pub(in crate::tui) fn owner<'a>(model: &'a Architecture, path: &Path) -> Option<&'a str> {
    model
        .nodes
        .iter()
        .filter(|n| {
            n.manifest_path
                .parent()
                .is_some_and(|r| path.starts_with(r))
        })
        .max_by_key(|n| n.manifest_path.components().count())
        .map(|n| n.id.as_str())
}
pub(super) fn details(app: &App) -> Vec<Line<'static>> {
    let model = &app.project.architecture;
    let Some(node) = model.node(&app.map.selected) else {
        return vec![components::section_header("No crate selected")];
    };
    let root = node.manifest_path.parent().unwrap_or(Path::new(""));
    let metrics = app
        .metrics
        .as_ref()
        .and_then(|m| m.packages.iter().find(|p| p.root == root));
    let rust_files = app.explorer.tree.as_ref().map(|t| {
        t.entries
            .iter()
            .filter(|e| {
                e.kind == crate::repository::RepoEntryKind::File
                    && e.path.extension().is_some_and(|ext| ext == "rs")
                    && owner(model, &e.path) == Some(node.id.as_str())
            })
            .count()
    });
    // A skipped/failed parse is unknown, not a zero-complexity crate.
    let ast = app.maintainability.as_ref().and_then(|report| {
        let files: Vec<_> = report
            .files
            .iter()
            .filter(|f| owner(model, &f.path) == Some(node.id.as_str()))
            .collect();
        let complete = app
            .explorer
            .tree
            .as_ref()
            .is_some_and(|t| !t.truncated && t.skipped_errors == 0)
            && rust_files == Some(files.len())
            && files.iter().all(|f| f.structure.is_some());
        complete
            .then(|| report.packages.iter().find(|p| p.root == root))
            .flatten()
    });
    let hotspots = app.hotspots.as_ref().filter(|r| r.available).map(|r| {
        r.candidate_paths
            .iter()
            .filter(|p| owner(model, p) == Some(node.id.as_str()))
            .count()
    });
    let mut l = vec![
        components::section_header(&node.name),
        components::path_label(&node.manifest_path, usize::MAX),
        Line::styled(node.id.clone(), Style::default().fg(theme::MUTED)),
        Line::raw(""),
        components::section_header("CURRENT STRUCTURE"),
        components::metric_value(
            "Source nonblank LOC",
            metrics.map_or_else(
                || "unknown".into(),
                |p| {
                    if app.metrics.as_ref().is_some_and(|m| m.partial) {
                        format!("≥{} (partial)", p.source_nonblank_lines)
                    } else {
                        p.source_nonblank_lines.to_string()
                    }
                },
            ),
        ),
        components::metric_value("Rust files", value(rust_files)),
        components::metric_value("Functions", value(ast.map(|p| p.function_count))),
        components::metric_value(
            "Median / P95",
            ast.map_or_else(
                || "unknown".into(),
                |p| {
                    format!(
                        "{} / {} lines",
                        p.median_function_lines, p.p95_function_lines
                    )
                },
            ),
        ),
        components::metric_value("Max control depth", value(ast.map(|p| p.max_control_depth))),
        components::metric_value("Hotspot files", value(hotspots)),
        Line::raw(""),
        components::section_header("CARGO RELATIONSHIPS"),
        components::metric_value(
            "Dependencies",
            model
                .outgoing(&node.id)
                .map(|e| &e.to)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        ),
        components::metric_value(
            "Dependents",
            model
                .incoming(&node.id)
                .map(|e| &e.from)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        ),
        components::metric_value("External declarations", node.external_dependencies),
    ];
    let unresolved = model
        .unresolved_dependencies
        .iter()
        .filter(|u| u.from == node.id)
        .count();
    let hint = if model.nodes.len() == 1 {
        format!("1 Cargo crate · {} Rust files", value(rust_files))
    } else {
        "Rust modules of this crate".into()
    };
    l.insert(3, Line::styled(hint, Style::default().fg(theme::MUTED)));
    l.insert(
        4,
        Line::styled(
            "m → Open module map to explore internal architecture",
            Style::default().fg(theme::ACTIVE),
        ),
    );
    if unresolved > 0 {
        l.insert(
            2,
            Line::styled(
                format!("[!] {unresolved} unresolved dependencies · reasons below"),
                Style::default().fg(theme::WARNING),
            ),
        );
    }
    l.push(components::section_header("DEPENDS ON · outgoing →"));
    for edge in model.outgoing(&node.id) {
        l.push(Line::styled(
            format!(
                "{} {} · alias {}{}{}",
                edge_mark(edge.kind),
                model
                    .node(&edge.to)
                    .map_or(edge.to.as_str(), |n| n.name.as_str()),
                edge.alias,
                edge.target
                    .as_ref()
                    .map_or_else(String::new, |s| format!(" · {s}")),
                if edge.optional { " · optional" } else { "" }
            ),
            Style::default().fg(edge_color(edge.kind)),
        ));
    }
    l.push(components::section_header("USED BY · incoming →"));
    for edge in model.incoming(&node.id) {
        l.push(Line::styled(
            format!(
                "{} {}",
                model
                    .node(&edge.from)
                    .map_or(edge.from.as_str(), |n| n.name.as_str()),
                edge_mark(edge.kind)
            ),
            Style::default().fg(edge_color(edge.kind)),
        ));
    }
    l.push(components::section_header("CARGO TARGETS · o to choose"));
    for target in &node.targets {
        l.push(Line::raw(format!(
            "{} · {}",
            target.name,
            target.kinds.join(",")
        )));
        l.push(components::path_label(&target.path, usize::MAX));
    }
    for u in model
        .unresolved_dependencies
        .iter()
        .filter(|u| u.from == node.id)
    {
        l.push(Line::styled(
            format!("Unresolved {}: {}", u.alias, u.reason),
            Style::default().fg(theme::WARNING),
        ));
    }
    if app.metrics.as_ref().is_some_and(|m| m.partial)
        || app.maintainability.as_ref().is_some_and(|m| m.partial)
    {
        l.push(Line::styled(
            "Current structure partial · inspect Overview Analysis",
            Style::default().fg(theme::WARNING),
        ));
    }
    l.push(Line::styled(
        "Declared dependencies include cfg/optional/dev/build; not an activated feature graph.",
        Style::default().fg(theme::MUTED),
    ));
    l
}
fn value(n: Option<impl std::fmt::Display>) -> String {
    n.map_or_else(|| "unknown".into(), |n| n.to_string())
}

//! File-scoped metrics are deliberately not attributed to each inline declaration.
use super::*;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};
fn metric(label: &str, v: impl std::fmt::Display) -> Line<'static> {
    let v = v.to_string();
    let color = if v == "unknown" {
        theme::MUTED
    } else {
        theme::NUMBER
    };
    Line::from(vec![
        Span::styled(format!("{label:<26} "), Style::default().fg(theme::MUTED)),
        Span::styled(v, Style::default().fg(color)),
    ])
}
fn value(v: Option<impl std::fmt::Display>) -> String {
    v.map_or_else(|| "unknown".into(), |v| v.to_string())
}
fn loc(app: &App, files: &BTreeSet<Arc<PathBuf>>) -> Option<u64> {
    app.metrics
        .as_ref()
        .filter(|_| !files.is_empty())
        .and_then(|r| {
            files.iter().try_fold(0, |sum, p| {
                r.source_files
                    .iter()
                    .find(|f| f.path == **p)
                    .map(|f| sum + f.nonblank_lines)
            })
        })
}
pub(super) fn details(app: &App) -> Vec<Line<'static>> {
    let s = &app.module_map;
    let Some(g) = &s.graph else { return vec![] };
    let Some(n) = g.node(&s.selected) else {
        return vec![Line::raw("No module selected")];
    };
    let Some(e) = s.index.nodes.get(&n.id) else {
        return vec![];
    };
    let files = &e.files;
    let ast = app
        .maintainability
        .as_ref()
        .filter(|_| !files.is_empty())
        .and_then(|r| {
            files
                .iter()
                .map(|p| {
                    r.files
                        .iter()
                        .find(|f| f.path == **p)
                        .and_then(|f| f.structure.as_ref())
                })
                .collect::<Option<Vec<_>>>()
        });
    let own = g
        .resolution_by_module
        .get(&n.id)
        .cloned()
        .unwrap_or_default();
    let mut l = vec![
        components::section_header(&format!("MODULE · {}", n.qualified_path)),
        components::path_label(
            n.source_file.as_deref().unwrap_or(&n.declaration_file),
            usize::MAX,
        ),
        Line::styled(
            format!(
                "{:?} · {}:{} · o source",
                n.status,
                n.declaration_file.display(),
                n.declaration_line
            ),
            Style::default().fg(theme::MUTED),
        ),
        components::section_header("STRUCTURE · physical subtree"),
        metric("Child modules", e.children.len()),
        metric("Physical subtree files", files.len()),
        metric("Subtree file LOC", value(loc(app, files))),
        metric(
            "Subtree file functions",
            value(
                ast.as_ref()
                    .map(|v| v.iter().map(|s| s.functions).sum::<usize>()),
            ),
        ),
        metric(
            "Max file control depth",
            value(
                ast.as_ref()
                    .map(|v| v.iter().map(|s| s.max_control_depth).max().unwrap_or(0)),
            ),
        ),
        Line::styled(
            "Own module LOC/functions: unknown",
            Style::default().fg(theme::MUTED),
        ),
        Line::styled(
            "Physical files deduped; not exclusive inline contents.",
            Style::default().fg(theme::MUTED),
        ),
        components::section_header("RELATIONSHIPS · records, not calls"),
        metric("Direct verified imports", own.verified_imports),
        metric("Subtree verified imports", e.verified),
        metric("Direct imported by modules", e.incoming.len()),
        metric("Direct unclassified", own.unclassified_imports()),
        metric("Subtree unclassified", e.unclassified),
        components::section_header("REVIEW PRIORITIES"),
    ];
    if s.mode != super::Mode::Hierarchy {
        if let Some(focus) = s.index.nodes.get(&s.focus) {
            let links = if s.mode == super::Mode::Imports {
                &focus.outgoing
            } else {
                &focus.incoming
            };
            if let Some(link) = links.iter().find(|l| l.id == n.id) {
                l.insert(
                    1,
                    Line::styled(
                        format!(
                            "Selected relation: {} records · {} conditional",
                            link.records, link.conditional
                        ),
                        Style::default().fg(theme::NUMBER),
                    ),
                );
            }
        }
    }
    let mut children: Vec<_> = e
        .children
        .iter()
        .filter_map(|id| {
            if g.node(id).is_none_or(|n| n.inline) {
                return None;
            }
            let c = &s.index.nodes[id];
            loc(app, &c.files).map(|v| (id, v))
        })
        .collect();
    children.sort_by(|(a, x), (b, y)| y.cmp(x).then(a.cmp(b)));
    if children.is_empty() {
        l.push(Line::raw(
            "Largest children: no measured file-backed children",
        ));
    }
    for (id, size) in children.into_iter().take(3) {
        if let Some(n) = g.node(id) {
            l.push(Line::raw(format!("{} · {size} file LOC", n.qualified_path)));
        }
    }
    if let Some(h) = app.hotspots.as_ref().filter(|h| h.available) {
        l.push(metric(
            "Hotspot files",
            h.candidate_paths
                .iter()
                .filter(|p| files.contains(*p))
                .count(),
        ));
        let mut count = 0;
        for f in h
            .hotspots
            .iter()
            .filter(|f| files.contains(&f.path))
            .take(3)
        {
            l.push(Line::raw(format!(
                "Hotspot: {} · {} changes",
                f.path.display(),
                f.change_commits
            )));
            count += 1;
        }
        if count == 0 {
            l.push(Line::raw("No ranked hotspot files in subtree"));
        }
    } else {
        l.push(Line::raw("Hotspots: unknown (loading/unavailable)"));
    }
    if let Some(r) = &app.maintainability {
        for f in r
            .longest_functions
            .iter()
            .filter(|f| files.contains(&f.path))
            .take(3)
        {
            l.push(Line::raw(format!(
                "{} · {} lines · {}:{}",
                f.qualified_name,
                f.physical_lines,
                f.path.display(),
                f.start_line
            )));
        }
        l.push(metric(
            "Longest in subtree files",
            value(ast.as_ref().map(|v| {
                v.iter()
                    .map(|s| s.longest_function_lines)
                    .max()
                    .unwrap_or(0)
            })),
        ));
        for f in r
            .deepest_functions
            .iter()
            .filter(|f| files.contains(&f.path))
            .take(1)
        {
            l.push(Line::raw(format!(
                "Deepest known: {} · depth {}",
                f.qualified_name, f.max_control_depth
            )));
        }
        if ast.is_none() {
            l.push(Line::raw(
                "AST totals unknown: skipped/failed files or no data",
            ));
        }
    } else {
        l.push(Line::raw("Functions: unknown (AST analyzing)"));
    }
    l.extend([
        components::section_header("ANALYSIS STATUS"),
        metric("Coverage (graph)", format!("{:?}", g.coverage_status)),
        metric("Resolution (graph)", format!("{:?}", g.resolution_status)),
        metric("Subtree coverage losses", e.losses),
        metric("Direct conditional", own.conditional_relations),
        Line::raw("Syntax only; unclassified imports are not defects."),
        components::section_header("FILES · f expand/collapse"),
        Line::raw(format!(
            "{} physical files{}",
            files.len(),
            if s.show_files {
                ""
            } else {
                " · press f to expand"
            }
        )),
    ]);
    if s.show_files {
        for p in files {
            l.push(components::path_label(p, usize::MAX));
        }
    }
    l.push(components::section_header(
        "ANALYSIS LIMITATIONS · a expand/collapse",
    ));
    if s.show_limits {
        for c in &n.conditions {
            l.push(Line::raw(format!("[cfg] {c} · not evaluated")));
        }
        for reason in &g.partial_reasons {
            l.push(Line::styled(
                reason.clone(),
                Style::default().fg(theme::WARNING),
            ));
        }
        let reasons: Vec<_> = g
            .unresolved
            .iter()
            .filter(|u| u.module_id == n.id)
            .collect();
        let count = g
            .unresolved_by_module
            .get(&n.id)
            .map_or(0, |c| c.imports + c.other);
        l.push(Line::raw(format!(
            "{} of {count} direct reasons retained",
            reasons.len()
        )));
        for u in reasons {
            l.push(Line::raw(format!(
                "{:?} · {}:{} · {}",
                u.issue_kind,
                u.source_file.display(),
                u.line,
                u.reason
            )));
        }
        l.push(Line::raw(
            "No macro expansion, alias/re-export resolver or cfg evaluation.",
        ));
    } else {
        l.push(Line::raw("a: classification details / coverage limits"));
    }
    l
}

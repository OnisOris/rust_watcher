use super::*;
pub(in crate::tui) fn details(app: &App) -> Vec<Line<'static>> {
    if let Some(f) = super::super::hotspots::selected(app) {
        return super::super::hotspots::details(app, f);
    }
    let Some(target) = selected(app) else {
        return vec![
            heading("Selection unavailable"),
            Line::raw("The selected item is absent from the latest results."),
        ];
    };
    let mut lines = match target {
        Target::Metric(m) => {
            let mut l = vec![
                heading(m.label()),
                Line::raw(metric_value(app, m)),
                Line::raw(m.explanation().what),
                heading("WHY"),
                Line::raw(m.explanation().why),
                heading("LIMITATIONS"),
                Line::raw(m.explanation().limitations),
                heading("NEXT ACTION"),
                Line::raw(m.explanation().next),
                Line::raw("? full interpretation · Tab then j/k scroll"),
            ];
            if let Some(r) = &app.maintainability {
                l.push(Line::raw(if r.partial {
                    "Rust structure PARTIAL · measured bodies only"
                } else {
                    "Rust structure ready · bounded syntax measurement"
                }));
            }
            l
        }
        Target::Guide(g) => vec![
            heading(g.label()),
            Line::raw(g.description()),
            Line::raw(""),
            heading("Availability"),
            Line::raw(g.availability(app)),
            heading("Next action"),
            Line::raw("Enter starts · Esc returns along the exploration route"),
            Line::raw("? explains this view · Tab focuses scrollable details"),
            heading("What you can trust"),
            Line::raw("Cargo identifies targets. RA supplies symbols and static calls."),
            Line::raw("Module relationships are conservative syntax, not runtime behavior."),
            Line::raw("Review areas are measured signals, not defect predictions."),
        ],
        Target::Workspace => summary(app),
        Target::Group(Group::Analysis) => return analysis::analysis_details(app),
        Target::Group(Group::Hotspots) => {
            let mut l = super::super::hotspots::status(app);
            l.push(Line::raw("Review priorities, not defect predictions."));
            l.push(Line::raw(
                "≥3 commits AND source ≥400 lines / function ≥80 / depth ≥4.",
            ));
            l.push(Line::raw("Enter expands / collapses this group."));
            l
        }
        Target::Group(Group::Languages) => {
            let mut l = vec![heading("Languages · nonblank lines")];
            if let Some(m) = &app.metrics {
                for lang in &m.languages {
                    l.push(components::metric_value(
                        &lang.language,
                        lang.nonblank_lines,
                    ));
                }
            }
            l
        }
        Target::Group(g) => vec![
            heading(g.label()),
            Line::raw("Enter to expand / collapse. j/k selects a result."),
            Line::raw("Sizes and structural signals are not a quality score."),
        ],
        Target::File(p, n) => vec![
            heading("Source file"),
            components::path_label(&p, usize::MAX),
            components::metric_value("Nonblank source lines", n),
            Line::raw("Includes comments; not syntactic SLOC."),
            Line::raw("Enter → Explorer → Rust symbols → Inspector → Calls"),
        ],
        Target::Unmeasured(f) => vec![
            heading("LOC unknown"),
            components::path_label(&f.path, usize::MAX),
            Line::raw(f.reason),
            Line::raw("Enter → Explorer"),
        ],
        Target::Longest(f) | Target::Deepest(f) => vec![
            heading(&f.qualified_name),
            components::path_label(&f.path, usize::MAX),
            components::metric_value("Declaration line", f.start_line),
            components::metric_value("Physical lines", f.physical_lines),
            components::metric_value("Max control depth", f.max_control_depth),
            components::metric_value("Control-flow nodes", f.control_flow_nodes),
            Line::raw(if f.test_classified {
                "[test] · file/attribute classification"
            } else {
                "Rust structure · not cyclomatic complexity"
            }),
            heading("Why investigate"),
            Line::raw("Included in a bounded length/depth ranking, not an importance or defect ranking."),
            Line::raw("Physical lines include comments/blanks. Depth counts nested control constructs, not cyclomatic complexity."),
            Line::raw("? definitions · Enter exact anchor → RA Inspector · c Calls · Esc back"),
        ],
        Target::Entrypoint(p) => vec![
            heading("Cargo entrypoint"),
            components::path_label(&p, usize::MAX),
            Line::raw("Enter → exact Cargo target in Explorer"),
        ],
        Target::Package(n) => package(app, &n),
        Target::Hotspot(_) | Target::Historical(_) => unreachable!(),
    };
    if let Some(notice) = &app.notice {
        lines.insert(
            0,
            Line::styled(notice.clone(), Style::default().fg(theme::WARNING)),
        );
    }
    if let AnalyzerState::Error(e) = &app.analyzer_state {
        lines.push(heading("RA unavailable · metrics remain available"));
        lines.push(Line::styled(e.clone(), Style::default().fg(theme::ERROR)));
    }
    lines
}
fn summary(app: &App) -> Vec<Line<'static>> {
    let mut l = vec![
        heading(project_name(&app.project)),
        heading("PROJECT SNAPSHOT"),
    ];
    if let Some(m) = &app.metrics {
        l.push(components::metric_value("Scanned files", m.total_files));
        if m.partial {
            l.push(Line::raw("PARTIAL scan · counts cover measured files"));
        }
    } else {
        l.push(Line::raw(app.metrics_error.clone().unwrap_or_else(|| {
            "File counts unknown · scanning / unavailable".into()
        })));
    }
    if let Some(r) = &app.maintainability {
        l.push(components::metric_value("Rust functions", r.functions));
        if r.partial {
            l.push(Line::raw("PARTIAL Rust structure · measured bodies only"));
        }
    } else {
        l.push(Line::raw(app.maintainability_error.clone().unwrap_or_else(
            || "Rust functions unknown · analyzing / unavailable".into(),
        )));
    }
    if let Some(n) = app.hotspots.as_ref().and_then(|h| h.candidate_files) {
        l.push(components::metric_value("Review candidates", n));
    } else {
        l.push(Line::raw("Review candidates unknown"));
    }
    l.extend([
        heading("WHAT CAN I DO?"),
        Line::raw("Choose execution, architecture, symbols or review."),
        Line::raw("Additional data → Enter expands technical lists."),
        heading("ANALYSIS STATUS"),
        Line::raw(format!("RA: {:?}", app.analyzer_state)),
    ]);
    l.extend(super::super::hotspots::status(app));
    l.push(Line::raw(
        "Additional data → Analysis: full metrics and limitations",
    ));
    l
}
fn package(app: &App, name: &str) -> Vec<Line<'static>> {
    let mut l = vec![heading(name)];
    if let Some(p) = app.project.packages.iter().find(|p| p.name == name) {
        l.push(components::path_label(
            p.manifest_path
                .strip_prefix(&app.project.workspace_root)
                .unwrap_or(&p.manifest_path),
            usize::MAX,
        ));
        l.push(heading("Cargo targets"));
        for t in &p.targets {
            l.push(Line::raw(format!("{} · {}", t.name, t.kinds.join(", "))));
        }
    }
    if let Some(m) = &app.metrics {
        if let Some(p) = m.packages.iter().find(|p| p.name == name) {
            l.push(components::metric_value(
                "Source nonblank LOC",
                p.source_nonblank_lines,
            ));
        }
    }
    if let Some(m) = &app.maintainability {
        if let Some(p) = m.packages.iter().find(|p| p.name == name) {
            l.push(components::metric_value("Functions", p.function_count));
        }
    }
    l.push(Line::raw("5 Map → dependencies and dependents"));
    l
}

fn metric_value(app: &App, metric: super::super::explanations::Metric) -> String {
    use super::super::explanations::Metric as M;
    let n = match metric {
        M::SourceLoc => app.metrics.as_ref().map(|r| {
            format!(
                "{} nonblank source lines{}",
                r.code_nonblank_lines,
                if r.partial { " · PARTIAL" } else { "" }
            )
        }),
        M::PhysicalLoc => app.metrics.as_ref().map(|r| {
            format!(
                "{} repository physical lines{}",
                r.physical_lines,
                if r.partial { " · PARTIAL" } else { "" }
            )
        }),
        M::Functions => app
            .maintainability
            .as_ref()
            .map(|r| format!("{} measured bodies", r.functions)),
        M::Length => app.maintainability.as_ref().map(|r| {
            if r.functions == 0 {
                "No measured bodies · median/P95 not applicable".into()
            } else {
                format!(
                    "Median {} · P95 {} physical lines",
                    r.median_function_lines, r.p95_function_lines
                )
            }
        }),
        M::Depth => app
            .maintainability
            .as_ref()
            .map(|r| format!("Depth {} · nested control structures", r.max_control_depth)),
        M::Candidates => app.hotspots.as_ref().and_then(|r| {
            r.candidate_files.map(|n| {
                format!(
                    "{n} Git + structural candidates{}",
                    if r.history_partial || r.current_metrics_partial {
                        " · PARTIAL"
                    } else {
                        ""
                    }
                )
            })
        }),
        _ => {
            return "Context-specific measurement · select a file, module or callable for its value"
                .into()
        }
    };
    n.unwrap_or_else(|| "Unknown · measurement loading or unavailable; not zero".into())
}

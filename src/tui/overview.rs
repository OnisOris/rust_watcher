//! Overview presentation and its typed, shared selection model.
use super::app::{project_name, AnalyzerState, App, Pane};
use super::ui::{heading, pane_block};
use crate::maintainability::FunctionMetrics;
use crate::metrics::UnmeasuredFile;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::path::PathBuf;

#[derive(Clone)]
pub(super) enum Target {
    Workspace,
    Hotspot(PathBuf),
    Entrypoint(PathBuf),
    Package(String),
    File(PathBuf, u64),
    Unmeasured(UnmeasuredFile),
    Longest(FunctionMetrics),
    Deepest(FunctionMetrics),
}

pub(super) fn targets(app: &App) -> Vec<Target> {
    let mut items = vec![Target::Workspace];
    if let Some(report) = &app.hotspots {
        items.extend(
            report
                .rows()
                .iter()
                .map(|f| Target::Hotspot(f.path.clone())),
        );
    }
    items.extend(
        app.project
            .binary_entrypoints()
            .into_iter()
            .map(Target::Entrypoint),
    );
    items.extend(
        app.project
            .packages
            .iter()
            .map(|p| Target::Package(p.name.clone())),
    );
    if let Some(m) = &app.metrics {
        items.extend(
            m.largest_files
                .iter()
                .map(|f| Target::File(f.path.clone(), f.nonblank_lines)),
        );
        items.extend(
            m.unmeasured_large_files
                .iter()
                .cloned()
                .map(Target::Unmeasured),
        );
    }
    if let Some(r) = &app.maintainability {
        items.extend(r.longest_functions.iter().cloned().map(Target::Longest));
        items.extend(r.deepest_functions.iter().cloned().map(Target::Deepest));
    }
    items
}

pub(super) fn render_project(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut previous = "";
    let mut items = Vec::new();
    for target in targets(app) {
        let (section, text) = match target {
            Target::Workspace => ("", format!("▼ Project\n  {}", project_name(&app.project))),
            Target::Hotspot(path) => ("GIT HOTSPOTS", super::hotspots::label(app, &path)),
            Target::Entrypoint(p) => ("", format!("Entrypoint\n  {}", p.display())),
            Target::Package(name) => ("", format!("Crate\n  {name}")),
            Target::File(path, lines) => (
                "LARGEST SOURCE FILES",
                format!("{lines} nonblank · {}", path.display()),
            ),
            Target::Unmeasured(f) => (
                "UNMEASURED · LOC unknown",
                format!(
                    "{}\n  {} bytes · {}",
                    f.path.display(),
                    f.size_bytes
                        .map_or_else(|| "unknown".into(), |n| n.to_string()),
                    f.reason
                ),
            ),
            Target::Longest(f) => ("LONGEST FUNCTIONS", function_label(&f, false)),
            Target::Deepest(f) => ("DEEPEST CONTROL FLOW", function_label(&f, true)),
        };
        let mut lines = Vec::new();
        if !section.is_empty() && previous != section {
            lines.push(heading(section));
        }
        lines.extend(text.lines().map(|s| Line::raw(s.to_owned())));
        items.push(ListItem::new(lines));
        previous = section;
    }
    let mut state = ListState::default().with_selected(Some(app.project_selection));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
            .block(pane_block(
                " PROJECT · review candidates ",
                app.pane == Pane::Project,
            )),
        area,
        &mut state,
    );
}

fn function_label(f: &FunctionMetrics, deepest: bool) -> String {
    format!(
        "{} {}{} · {}\n  {}:{}",
        if deepest {
            f.max_control_depth
        } else {
            f.physical_lines
        },
        if deepest { "depth" } else { "lines" },
        if f.test_classified { " [test]" } else { "" },
        f.qualified_name,
        f.path.display(),
        f.start_line
    )
}

pub(super) fn render_details(frame: &mut Frame<'_>, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(details(app))
            .scroll((app.metrics_scroll.min(u16::MAX as usize) as u16, 0))
            .wrap(Wrap { trim: false })
            .block(pane_block(
                if super::hotspots::selected(app).is_some() {
                    " HOTSPOT DETAILS · j/k scroll "
                } else {
                    " REPOSITORY METRICS · j/k scroll "
                },
                app.pane == Pane::Details,
            )),
        area,
    );
}

pub(super) fn details(app: &App) -> Vec<Line<'static>> {
    if let Some(file) = super::hotspots::selected(app) {
        return super::hotspots::details(app, file);
    }
    let mut lines = vec![heading("Workspace")];
    lines.extend(super::hotspots::status(app));
    if let Some(notice) = &app.notice {
        lines.push(Line::styled(
            notice.clone(),
            Style::default().fg(Color::Yellow),
        ));
    }
    if let Some(m) = &app.metrics {
        lines.push(Line::raw(if m.partial {
            "Metrics ready · PARTIAL"
        } else {
            "Metrics ready"
        }));
        lines.push(Line::raw(format!(
            "Files {} · source {} · dirs {}",
            m.total_files, m.code_files, m.total_directories
        )));
        lines.push(Line::raw(format!(
            "Physical LOC {} · nonblank {}",
            m.physical_lines, m.nonblank_lines
        )));
        lines.push(Line::raw(format!(
            "Source LOC {} · nonblank {}",
            m.code_physical_lines, m.code_nonblank_lines
        )));
        lines.push(Line::raw(format!(
            "Test files {} · nonblank {} ({:.1}%)",
            m.test_files,
            m.test_nonblank_lines,
            100.0 * m.test_nonblank_lines as f64 / m.code_nonblank_lines.max(1) as f64
        )));
    } else if let Some(e) = &app.metrics_error {
        lines.push(Line::raw(format!("Metrics error: {e}")));
    } else {
        lines.push(Line::raw("Repository Metrics: loading..."));
    }
    lines.push(heading("MAINTAINABILITY · Rust"));
    if let Some(r) = &app.maintainability {
        lines.push(Line::raw(if r.partial {
            "Maintainability: partial"
        } else {
            "Maintainability: ready"
        }));
        lines.push(Line::raw(format!(
            "Functions {} · median {} · P95 {} lines",
            r.functions, r.median_function_lines, r.p95_function_lines
        )));
        lines.push(Line::raw(format!(
            "Max control depth {} · parsed {}/{}",
            r.max_control_depth, r.parsed_files, r.analyzed_files
        )));
        lines.push(Line::raw(format!(
            "Parse failures {} · skipped {}",
            r.parse_failures, r.skipped_files
        )));
        lines.push(Line::raw("Size and nesting are review signals."));
        for f in r.failures.iter().take(3) {
            lines.push(Line::raw(format!("{}: {}", f.file.display(), f.reason)));
        }
        lines.push(heading("Function packages · count / median / P95"));
        let mut packages: Vec<_> = r.packages.iter().collect();
        packages.sort_by(|a, b| {
            b.function_count
                .cmp(&a.function_count)
                .then(a.name.cmp(&b.name))
                .then(a.root.cmp(&b.root))
        });
        for p in packages.iter().take(5) {
            lines.push(Line::raw(format!(
                "{}  {} / {} / {}",
                p.name, p.function_count, p.median_function_lines, p.p95_function_lines
            )));
        }
    } else if let Some(e) = &app.maintainability_error {
        lines.push(Line::raw(format!("Maintainability unavailable: {e}")));
    } else {
        lines.push(Line::raw("Maintainability: analyzing..."));
    }
    if let Some(m) = &app.metrics {
        lines.push(heading("Languages · nonblank lines"));
        let mut languages: Vec<_> = m.languages.iter().collect();
        languages.sort_by(|a, b| {
            b.nonblank_lines
                .cmp(&a.nonblank_lines)
                .then(a.language.cmp(&b.language))
        });
        for l in languages {
            lines.push(Line::raw(format!(
                "{}  {} · {} files",
                l.language, l.nonblank_lines, l.files
            )));
        }
        lines.push(heading("Largest Packages · source nonblank lines"));
        let mut packages: Vec<_> = m.packages.iter().collect();
        packages.sort_by(|a, b| {
            b.source_nonblank_lines
                .cmp(&a.source_nonblank_lines)
                .then(a.name.cmp(&b.name))
                .then(a.root.cmp(&b.root))
        });
        for p in packages.iter().take(5) {
            lines.push(Line::raw(format!(
                "{}  {} · {} source files",
                p.name, p.source_nonblank_lines, p.source_files
            )));
        }
        lines.push(Line::raw(format!(
            "Skipped: binary {} · UTF8 {} · large {} · read {} · scan {} · total budget {}",
            m.skipped.binary,
            m.skipped.invalid_utf8,
            m.skipped.oversized,
            m.skipped.unreadable,
            m.skipped.scan_errors,
            m.skipped.total_read_budget
        )));
        lines.push(Line::raw("File-test LOC excludes inline test separation."));
    }
    lines.push(heading("Entrypoints"));
    lines.extend(
        app.project
            .binary_entrypoints()
            .iter()
            .map(|p| Line::raw(p.display().to_string())),
    );
    lines.push(heading("Analyzer"));
    lines.push(Line::raw(match &app.analyzer_state {
        AnalyzerState::Starting => "starting...".into(),
        AnalyzerState::Indexing => "indexing...".into(),
        AnalyzerState::Ready => "ready".into(),
        AnalyzerState::Error(e) => format!("error: {e}"),
    }));
    lines
}

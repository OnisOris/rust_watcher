use super::*;
pub(super) fn analysis_details(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![heading("Workspace")];
    lines.extend(super::super::hotspots::status(app));
    if let Some(r) = &app.hotspots {
        lines.extend(
            r.partial_reasons
                .iter()
                .map(|r| Line::raw(format!("Limitation: {r}"))),
        );
    }
    if let Some(notice) = &app.notice {
        lines.push(Line::styled(
            notice.clone(),
            Style::default().fg(theme::WARNING),
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

use super::*;
use crate::{maintainability::FunctionMetrics, model::Symbol};
use std::collections::BTreeSet;
#[derive(Clone)]
pub(in crate::tui) enum Action {
    Function(FunctionMetrics),
    File(PathBuf, String),
    Symbol(Symbol),
}
impl Action {
    pub fn label(&self) -> String {
        match self {
            Self::Function(f) => format!(
                "{} · {} lines · depth {}",
                f.qualified_name, f.physical_lines, f.max_control_depth
            ),
            Self::File(_, s) => s.clone(),
            Self::Symbol(s) => format!("Previous: {}", super::super::app::qualified_symbol(s)),
        }
    }
    pub fn location(&self) -> String {
        match self {
            Self::Function(f) => format!(
                "{}:{}:{}",
                f.path.display(),
                f.identifier_line,
                f.identifier_column
            ),
            Self::File(p, _) => p.display().to_string(),
            Self::Symbol(s) => format!("{}:{}", s.file.display(), s.selection_range.start.line + 1),
        }
    }
}
pub(in crate::tui) struct Picker {
    pub title: String,
    pub actions: Vec<Action>,
    pub selected: usize,
    pub calls: bool,
}
pub(super) fn priorities(app: &App) -> Vec<Action> {
    let Some(e) = app.module_map.index.nodes.get(&app.module_map.selected) else {
        return vec![];
    };
    let mut a = vec![];
    let mut seen = BTreeSet::new();
    if let Some(r) = &app.maintainability {
        for f in r
            .longest_functions
            .iter()
            .chain(&r.deepest_functions)
            .filter(|f| e.files.contains(&f.path))
        {
            if seen.insert((f.path.clone(), f.identifier_line, f.identifier_column)) {
                a.push(Action::Function(f.clone()));
            }
        }
    }
    if let Some(h) = &app.hotspots {
        for f in h
            .hotspots
            .iter()
            .filter(|f| e.files.contains(&f.path) && f.present_in_tree)
            .take(5)
        {
            a.push(Action::File(
                f.path.clone(),
                format!(
                    "Hotspot: {} · {} changes",
                    f.path.display(),
                    f.change_commits
                ),
            ));
        }
    }
    if let Some(m) = &app.metrics {
        let mut files: Vec<_> = m
            .source_files
            .iter()
            .filter(|f| e.files.contains(&f.path))
            .collect();
        files.sort_by(|a, b| {
            b.nonblank_lines
                .cmp(&a.nonblank_lines)
                .then(a.path.cmp(&b.path))
        });
        for f in files.into_iter().take(3) {
            a.push(Action::File(
                f.path.clone(),
                format!(
                    "Large file: {} · {} nonblank",
                    f.path.display(),
                    f.nonblank_lines
                ),
            ));
        }
    }
    a.truncate(32);
    a
}
#[derive(Clone, Copy)]
pub(in crate::tui) enum Start {
    Entry,
    Longest,
    Deepest,
    Hotspots,
    Previous,
}
pub(super) fn routes(calls: bool) -> Vec<Start> {
    if calls {
        vec![Start::Entry, Start::Longest, Start::Previous]
    } else {
        vec![
            Start::Entry,
            Start::Longest,
            Start::Deepest,
            Start::Hotspots,
        ]
    }
}
pub(super) fn available(app: &App, route: Start, calls: bool) -> (String, Vec<Action>) {
    let mut a = vec![];
    let label = match route {
        Start::Entry => {
            for t in app
                .project
                .architecture
                .nodes
                .iter()
                .flat_map(|n| &n.targets)
                .filter(|t| {
                    t.kinds.iter().any(|k| {
                        k == "bin"
                            || (!calls
                                && matches!(
                                    k.as_str(),
                                    "lib"
                                        | "rlib"
                                        | "proc-macro"
                                        | "cdylib"
                                        | "dylib"
                                        | "staticlib"
                                ))
                    })
                })
            {
                if calls {
                    if let Some(r) = &app.maintainability {
                        for f in r
                            .longest_functions
                            .iter()
                            .chain(&r.deepest_functions)
                            .filter(|f| f.path == t.path && f.name == "main")
                        {
                            a.push(Action::Function(f.clone()));
                        }
                    }
                } else {
                    a.push(Action::File(
                        t.path.clone(),
                        format!("Cargo entrypoint: {}", t.name),
                    ));
                }
            }
            if calls && a.is_empty() {
                "Main entrypoint · no known exact anchor"
            } else {
                "Main entrypoint"
            }
        }
        Start::Longest | Start::Deepest => {
            if let Some(r) = &app.maintainability {
                for f in if matches!(route, Start::Longest) {
                    &r.longest_functions
                } else {
                    &r.deepest_functions
                } {
                    a.push(Action::Function(f.clone()));
                }
            }
            if app.maintainability_error.is_some() {
                "Maintainability: unavailable"
            } else if app.maintainability.is_none() {
                "Maintainability: analyzing..."
            } else if matches!(route, Start::Longest) {
                "Longest known functions"
            } else {
                "Deepest known functions"
            }
        }
        Start::Hotspots => {
            if let Some(h) = &app.hotspots {
                for f in h.hotspots.iter().filter(|f| f.present_in_tree) {
                    a.push(Action::File(
                        f.path.clone(),
                        format!("{} · {} changes", f.path.display(), f.change_commits),
                    ));
                }
            }
            if app.hotspots_error.is_some() {
                "Hotspots: unavailable"
            } else if app.hotspots.is_none() {
                "Hotspots: analyzing..."
            } else {
                "Review hotspot files"
            }
        }
        Start::Previous => {
            if let Some(i) = app
                .inspector
                .as_ref()
                .filter(|i| crate::model::is_callable_kind(&i.symbol.kind))
            {
                a.push(Action::Symbol(i.symbol.clone()));
            }
            "Previously inspected callable"
        }
    };
    if app.explorer.tree.is_none() {
        a.clear();
    }
    if calls && matches!(app.analyzer_state, AnalyzerState::Error(_)) {
        a.clear();
    }
    let mut seen = BTreeSet::new();
    a.retain(|a| seen.insert(a.location()));
    (label.into(), a)
}

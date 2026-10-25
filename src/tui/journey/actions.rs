use super::*;
use crate::{maintainability::FunctionMetrics, model::Symbol};
use std::collections::BTreeSet;
#[derive(Clone)]
pub(in crate::tui) enum Action {
    Function(FunctionMetrics),
    File(PathBuf, String),
    Symbol(Symbol),
    BinaryEntrypoint(BinaryEntrypoint),
}
/// Cargo identity, not semantic identity. Two targets may share a source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui) struct BinaryEntrypoint {
    pub crate_id: String,
    pub package: String,
    pub name: String,
    pub path: PathBuf,
}
impl BinaryEntrypoint {
    pub fn id(&self) -> String {
        format!(
            "{}/target:{}:bin:{}",
            self.crate_id,
            self.name,
            self.path.display()
        )
    }
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
            Self::BinaryEntrypoint(t) => format!("{} · {} · binary", t.name, t.package),
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
            Self::BinaryEntrypoint(t) => t.path.display().to_string(),
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
            for n in &app.project.architecture.nodes {
                for t in &n.targets {
                    if t.kinds.iter().any(|k| k == "bin") {
                        a.push(Action::BinaryEntrypoint(BinaryEntrypoint {
                            crate_id: n.id.clone(),
                            package: n.name.clone(),
                            name: t.name.clone(),
                            path: t.path.clone(),
                        }));
                    } else if !calls
                        && t.kinds.iter().any(|k| {
                            matches!(
                                k.as_str(),
                                "lib" | "rlib" | "proc-macro" | "cdylib" | "dylib" | "staticlib"
                            )
                        })
                    {
                        a.push(Action::File(
                            t.path.clone(),
                            format!("Open library root: {} · {}", n.name, t.name),
                        ));
                    }
                }
            }
            if a.is_empty() {
                if calls {
                    "No binary targets found"
                } else {
                    "No Cargo source roots found"
                }
            } else if calls {
                "Binary entrypoints"
            } else {
                "Cargo source roots"
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
            if matches!(route, Start::Longest) {
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
            "Review hotspot files"
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
        a.retain(|a| matches!(a, Action::BinaryEntrypoint(_)));
    }
    let mut seen = BTreeSet::new();
    a.retain(|a| {
        seen.insert(match a {
            Action::BinaryEntrypoint(t) => t.id(),
            _ => a.location(),
        })
    });
    (label.into(), a)
}
pub(super) fn unavailable_reason(app: &App, route: Start, calls: bool) -> String {
    if app.explorer.tree.is_none() {
        return "Repository tree: scanning; source navigation will be available when ready".into();
    }
    if calls
        && !matches!(route, Start::Entry)
        && matches!(app.analyzer_state, AnalyzerState::Error(_))
    {
        return "Call hierarchy unavailable: rust-analyzer not available. Binary source files remain accessible.".into();
    }
    match route {
        Start::Entry => {
            if calls {
                "No binary targets found. Library/test/example targets are not main entrypoints."
                    .into()
            } else {
                "No Cargo source roots found".into()
            }
        }
        Start::Longest | Start::Deepest => app.maintainability_error.as_ref().map_or_else(
            || {
                if app.maintainability.is_none() {
                    "Maintainability: analyzing...".into()
                } else {
                    "No known functions in the bounded maintainability rankings".into()
                }
            },
            |e| format!("Maintainability unavailable: {e}"),
        ),
        Start::Previous => "No previously inspected callable symbol".into(),
        Start::Hotspots => app.hotspots_error.as_ref().map_or_else(
            || {
                if app.hotspots.is_none() {
                    "Hotspots: analyzing...".into()
                } else {
                    "No current ranked hotspot files".into()
                }
            },
            |e| format!("Hotspots unavailable: {e}"),
        ),
    }
}

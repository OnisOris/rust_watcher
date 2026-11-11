use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui) enum Guide {
    Architecture,
    Entrypoint,
    Hotspots,
    Find,
}
impl Guide {
    pub fn label(self) -> &'static str {
        match self {
            Self::Architecture => "Explore architecture",
            Self::Entrypoint => "Explore execution",
            Self::Hotspots => "Review attention areas",
            Self::Find => "Find implementation",
        }
    }
}
impl Guide {
    pub fn description(self) -> &'static str {
        match self {
            Self::Entrypoint => "Binary entrypoint → real static calls",
            Self::Architecture => "Crates → modules → exact source",
            Self::Find => "Search Rust symbols → exact source",
            Self::Hotspots => "Measured evidence → source to study",
        }
    }
    pub fn availability(self, app: &App) -> String {
        match self {
            Self::Find => match &app.analyzer_state {
                AnalyzerState::Starting | AnalyzerState::Indexing => {
                    "Rust analysis is indexing · other routes remain available".into()
                }
                AnalyzerState::Error(_) => {
                    "Rust analysis unavailable · use architecture or source".into()
                }
                AnalyzerState::Ready => {
                    "Ready · search by symbol name, not natural language".into()
                }
            },
            Self::Entrypoint => {
                let binaries = app
                    .project
                    .architecture
                    .nodes
                    .iter()
                    .flat_map(|n| &n.targets)
                    .filter(|t| t.kinds.iter().any(|k| k == "bin"))
                    .count();
                if binaries == 0 {
                    "No binary targets · explore library modules instead".into()
                } else if app.explorer.tree.is_none() {
                    "Repository scanning · binary targets already known".into()
                } else if matches!(app.analyzer_state, AnalyzerState::Error(_)) {
                    "Source available · Calls unavailable without RA".into()
                } else {
                    format!("{binaries} binary target(s) · Enter chooses an entrypoint")
                }
            }
            Self::Architecture => {
                "Available from Cargo metadata · module scan is independent of RA".into()
            }
            Self::Hotspots => {
                if app.maintainability.is_some() || app.hotspots.is_some() {
                    "Enter shows Git or structural evidence · no quality verdict".into()
                } else {
                    "Local measurements loading · architecture remains available".into()
                }
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui) enum Group {
    Start,
    Hotspots,
    Largest,
    Longest,
    Deepest,
    Crates,
    Entrypoints,
    Languages,
    Historical,
    Analysis,
    Metrics,
}
impl Group {
    pub const ALL: [Self; 11] = [
        Self::Start,
        Self::Metrics,
        Self::Hotspots,
        Self::Largest,
        Self::Longest,
        Self::Deepest,
        Self::Crates,
        Self::Entrypoints,
        Self::Languages,
        Self::Historical,
        Self::Analysis,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "UNDERSTAND THIS REPOSITORY",
            Self::Hotspots => "GIT HOTSPOTS",
            Self::Largest => "LARGEST SOURCE FILES",
            Self::Longest => "LONGEST FUNCTIONS",
            Self::Deepest => "DEEPEST CONTROL FLOW",
            Self::Crates => "CRATES",
            Self::Entrypoints => "ENTRYPOINTS",
            Self::Languages => "LANGUAGES",
            Self::Historical => "HISTORICAL PATHS",
            Self::Analysis => "ANALYSIS & LIMITATIONS",
            Self::Metrics => "UNDERSTAND THE METRICS · ? details",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui) enum ItemId {
    Guide(Guide),
    Metric(super::super::explanations::Metric),
    Workspace,
    Group(Group),
    Path(Group, PathBuf),
    Package(String),
    Function(Group, PathBuf, usize, usize),
}
pub(in crate::tui) struct State {
    pub selected: ItemId,
    pub expanded: BTreeSet<Group>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            selected: ItemId::Guide(Guide::Entrypoint),
            expanded: BTreeSet::from([Group::Start]),
        }
    }
}
#[derive(Clone)]
pub(in crate::tui) enum Target {
    Guide(Guide),
    Metric(super::super::explanations::Metric),
    Workspace,
    Group(Group),
    Hotspot(PathBuf),
    Historical(PathBuf),
    Entrypoint(PathBuf),
    Package(String),
    File(PathBuf, u64),
    Unmeasured(UnmeasuredFile),
    Longest(FunctionMetrics),
    Deepest(FunctionMetrics),
}
impl Target {
    pub fn id(&self) -> ItemId {
        match self {
            Self::Guide(g) => ItemId::Guide(*g),
            Self::Metric(m) => ItemId::Metric(*m),
            Self::Workspace => ItemId::Workspace,
            Self::Group(g) => ItemId::Group(*g),
            Self::Package(n) => ItemId::Package(n.clone()),
            Self::Hotspot(p) => ItemId::Path(Group::Hotspots, p.clone()),
            Self::Historical(p) => ItemId::Path(Group::Historical, p.clone()),
            Self::File(p, _) => ItemId::Path(Group::Largest, p.clone()),
            Self::Unmeasured(f) => ItemId::Path(Group::Largest, f.path.clone()),
            Self::Entrypoint(p) => ItemId::Path(Group::Entrypoints, p.clone()),
            Self::Longest(f) => ItemId::Function(
                Group::Longest,
                f.path.clone(),
                f.identifier_line,
                f.identifier_column,
            ),
            Self::Deepest(f) => ItemId::Function(
                Group::Deepest,
                f.path.clone(),
                f.identifier_line,
                f.identifier_column,
            ),
        }
    }
}
pub(in crate::tui) fn targets(app: &App) -> Vec<Target> {
    let mut items = vec![];
    for group in Group::ALL {
        items.push(Target::Group(group));
        if !app.overview.expanded.contains(&group) {
            continue;
        }
        match group {
            Group::Metrics => items.extend(
                super::super::explanations::Metric::ALL
                    .into_iter()
                    .map(Target::Metric),
            ),
            Group::Start => items.extend(
                [
                    Guide::Entrypoint,
                    Guide::Architecture,
                    Guide::Find,
                    Guide::Hotspots,
                ]
                .into_iter()
                .map(Target::Guide),
            ),
            Group::Hotspots => {
                if let Some(r) = &app.hotspots {
                    items.extend(r.rows().iter().map(|f| Target::Hotspot(f.path.clone())));
                }
            }
            Group::Largest => {
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
            }
            Group::Longest => {
                if let Some(r) = &app.maintainability {
                    items.extend(r.longest_functions.iter().cloned().map(Target::Longest));
                }
            }
            Group::Deepest => {
                if let Some(r) = &app.maintainability {
                    items.extend(r.deepest_functions.iter().cloned().map(Target::Deepest));
                }
            }
            Group::Crates => items.extend(
                app.project
                    .packages
                    .iter()
                    .map(|p| Target::Package(p.name.clone())),
            ),
            Group::Historical => {
                if let Some(r) = &app.hotspots {
                    items.extend(
                        r.historical_files
                            .iter()
                            .map(|f| Target::Historical(f.path.clone())),
                    );
                }
            }
            Group::Entrypoints => items.extend(
                app.project
                    .binary_entrypoints()
                    .into_iter()
                    .map(Target::Entrypoint),
            ),
            _ => {}
        }
    }
    items.push(Target::Workspace);
    items
}
pub(in crate::tui) fn selected(app: &App) -> Option<Target> {
    targets(app)
        .into_iter()
        .find(|t| t.id() == app.overview.selected)
}
pub(in crate::tui) fn index(app: &App) -> Option<usize> {
    targets(app)
        .iter()
        .position(|t| t.id() == app.overview.selected)
}

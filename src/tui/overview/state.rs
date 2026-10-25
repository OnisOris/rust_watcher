use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui) enum Group {
    Hotspots,
    Largest,
    Longest,
    Deepest,
    Crates,
    Entrypoints,
    Languages,
    Historical,
    Analysis,
}
impl Group {
    pub const ALL: [Self; 9] = [
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
            Self::Hotspots => "GIT HOTSPOTS",
            Self::Largest => "LARGEST SOURCE FILES",
            Self::Longest => "LONGEST FUNCTIONS",
            Self::Deepest => "DEEPEST CONTROL FLOW",
            Self::Crates => "CRATES",
            Self::Entrypoints => "ENTRYPOINTS",
            Self::Languages => "LANGUAGES",
            Self::Historical => "HISTORICAL PATHS",
            Self::Analysis => "ANALYSIS & LIMITATIONS",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui) enum ItemId {
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
            selected: ItemId::Workspace,
            expanded: BTreeSet::from([Group::Hotspots]),
        }
    }
}
#[derive(Clone)]
pub(in crate::tui) enum Target {
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
    let mut items = vec![Target::Workspace];
    for group in Group::ALL {
        items.push(Target::Group(group));
        if !app.overview.expanded.contains(&group) {
            continue;
        }
        match group {
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

use crate::model::{CallScope, CallTarget, CallTargets, Range};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub(in crate::tui) const MAX_NODES: usize = 40;
pub(in crate::tui) const MAX_EDGES: usize = 80;
pub(in crate::tui) const MAX_EXPANSIONS: usize = 12;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui) struct Identity {
    pub file: PathBuf,
    pub selection_range: Range,
    pub range: Range,
    pub kind: String,
    pub name: String,
}
impl From<&CallTarget> for Identity {
    fn from(t: &CallTarget) -> Self {
        Self {
            file: t.file.clone(),
            selection_range: t.selection_range,
            range: t.range,
            kind: t.kind.clone(),
            name: t.name.clone(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::tui) enum Expansion {
    #[default]
    Idle,
    Loading,
    Ready {
        count: usize,
        truncated: bool,
    },
    Unsupported,
    Error(String),
    Cancelled,
}
impl Expansion {
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "Not requested".into(),
            Self::Loading => "Loading".into(),
            Self::Ready { count, truncated } => format!(
                "{count}{}",
                if *truncated {
                    "+ · truncated"
                } else {
                    " · complete"
                }
            ),
            Self::Unsupported => "Unsupported call hierarchy".into(),
            Self::Error(e) => format!("RPC error: {e}"),
            Self::Cancelled => "Cancelled · x retries".into(),
        }
    }
}
#[derive(Debug, Clone)]
pub(in crate::tui) struct Node {
    pub target: CallTarget,
    pub layer: i8,
    pub via: Option<Identity>,
    pub sides: [Expansion; 2],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tui) struct Request {
    pub id: u64,
    pub generation: u64,
    pub identity: Identity,
    pub target: CallTarget,
    pub incoming: bool,
    pub scope: CallScope,
}
#[derive(Debug, Clone, Default)]
pub(in crate::tui) struct State {
    pub enabled: bool,
    pub focus: Option<Identity>,
    pub selected: Option<Identity>,
    pub nodes: BTreeMap<Identity, Node>,
    pub edges: BTreeSet<(Identity, Identity)>,
    pub pending: Option<Request>,
    pub limited: bool,
    pub revision: u64,
    pub page: Cell<usize>,
    pub rows: Cell<usize>,
    pub viewport: Cell<(u16, u16)>,
    pub details_visible: Cell<bool>,
    pub(super) cache: RefCell<Option<(super::layout::Key, super::layout::Drawing)>>,
}
impl State {
    pub fn new(target: &CallTarget, enabled: bool) -> Self {
        let id = Identity::from(target);
        Self {
            enabled,
            focus: Some(id.clone()),
            selected: Some(id.clone()),
            rows: Cell::new(3),
            nodes: BTreeMap::from([(
                id,
                Node {
                    target: target.clone(),
                    layer: 0,
                    via: None,
                    sides: [Expansion::Loading, Expansion::Loading],
                },
            )]),
            ..Default::default()
        }
    }
    pub fn selected_target(&self) -> Option<&CallTarget> {
        self.nodes.get(self.selected.as_ref()?).map(|n| &n.target)
    }
    pub fn ingest(
        &mut self,
        owner: &Identity,
        incoming: bool,
        result: Result<Option<CallTargets>, String>,
    ) {
        let Some(node) = self.nodes.get_mut(owner) else {
            return;
        };
        let layer = node.layer;
        let side = usize::from(!incoming);
        node.sides[side] = match &result {
            Ok(Some(r)) => Expansion::Ready {
                count: r.items.len().min(20),
                truncated: r.truncated || r.items.len() > 20,
            },
            Ok(None) => Expansion::Unsupported,
            Err(e) => Expansion::Error(e.clone()),
        };
        self.revision += 1;
        let Ok(Some(mut result)) = result else { return };
        result.items.sort_by_key(|t| Identity::from(t));
        for target in result.items.into_iter().take(20) {
            let id = Identity::from(&target);
            let edge = if incoming {
                (id.clone(), owner.clone())
            } else {
                (owner.clone(), id.clone())
            };
            if self.edges.len() >= MAX_EDGES && !self.edges.contains(&edge) {
                self.limited = true;
                continue;
            }
            let next_layer = if layer == 0 {
                if incoming {
                    -1
                } else {
                    1
                }
            } else {
                layer.signum() * 2
            };
            // Canonical placement is independent of reply/expansion order. Direct
            // nodes win, then incoming, then the least exact parent identity.
            if let Some(old) = self.nodes.get_mut(&id) {
                if (next_layer.abs(), next_layer, Some(owner))
                    < (old.layer.abs(), old.layer, old.via.as_ref())
                {
                    old.layer = next_layer;
                    old.via = Some(owner.clone());
                }
            }
            if !self.nodes.contains_key(&id) {
                if self.nodes.len() >= MAX_NODES {
                    self.limited = true;
                    continue;
                }
                self.nodes.insert(
                    id.clone(),
                    Node {
                        target,
                        via: Some(owner.clone()),
                        layer: next_layer,
                        sides: Default::default(),
                    },
                );
            }
            self.edges.insert(edge);
        }
        self.reconcile_page();
    }
    pub fn begin(
        &mut self,
        id: u64,
        generation: u64,
        scope: CallScope,
    ) -> Result<Request, &'static str> {
        if self.pending.is_some() {
            return Err("A graph expansion is already loading");
        }
        let identity = self.selected.clone().ok_or("No selected function")?;
        let node = self
            .nodes
            .get_mut(&identity)
            .ok_or("No selected function")?;
        if node.layer == 0 {
            return Err("Focus already has its first level; select a direct caller or callee");
        }
        if node.layer.abs() != 1 {
            return Err("Maximum expansion depth is 2; Enter explicitly focuses this function");
        }
        if self.limited {
            return Err("Graph budget reached; focus a neighbor to explore another neighborhood");
        }
        let incoming = node.layer < 0;
        if !crate::model::is_callable_kind(&node.target.kind) {
            return Err("Call hierarchy requires a function or method");
        }
        let state = &mut node.sides[usize::from(!incoming)];
        if matches!(
            state,
            Expansion::Loading | Expansion::Ready { .. } | Expansion::Unsupported
        ) {
            return Err("This direction is already loaded or loading");
        }
        *state = Expansion::Loading;
        let request = Request {
            id,
            generation,
            identity,
            target: node.target.clone(),
            incoming,
            scope,
        };
        self.pending = Some(request.clone());
        Ok(request)
    }
    pub fn cancel(&mut self) {
        if let Some(r) = self.pending.take() {
            if let Some(n) = self.nodes.get_mut(&r.identity) {
                n.sides[usize::from(!r.incoming)] = Expansion::Cancelled;
            }
        }
    }
    pub fn neighbors(&self, layer: i8) -> Vec<Identity> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.layer == layer)
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub fn move_by(&mut self, d: isize) {
        let ids = self.visible();
        let at = ids
            .iter()
            .position(|id| Some(id) == self.selected.as_ref())
            .unwrap_or(0);
        self.selected = ids
            .get(at.saturating_add_signed(d).min(ids.len().saturating_sub(1)))
            .cloned();
    }
    pub fn change_page(&mut self, d: isize) {
        let next = self
            .page
            .get()
            .saturating_add_signed(d)
            .min(self.pages() - 1);
        if self.page.replace(next) != next {
            self.selected = self
                .visible()
                .get(1)
                .cloned()
                .or_else(|| self.focus.clone());
        }
    }
    pub fn viewport(&self, w: u16, h: u16) {
        if self.viewport.replace((w, h)) != (w, h) {
            self.rows
                .set((h.saturating_sub(2) / 6).clamp(1, 3) as usize);
            if self.selected != self.focus {
                if let Some(page) = self
                    .plan()
                    .iter()
                    .position(|ids| ids.iter().any(|id| Some(id) == self.selected.as_ref()))
                {
                    self.page.set(page);
                }
            }
            self.page.set(self.page.get().min(self.pages() - 1));
        }
    }
}

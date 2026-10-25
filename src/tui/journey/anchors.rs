//! Exact-file selectors. Only rust-analyzer supplies semantic symbols.
use super::*;
use crate::model::Symbol;

pub(in crate::tui) enum Selector {
    Function(FunctionMetrics),
    Binary(BinaryEntrypoint),
    Exact(Symbol),
}
impl Selector {
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Function(f) => &f.path,
            Self::Binary(t) => &t.path,
            Self::Exact(s) => &s.file,
        }
    }
    pub fn matches(&self, symbol: &Symbol) -> bool {
        match self {
            Self::Function(f) => super::super::explorer::anchor_matches(f, symbol),
            Self::Binary(t) => main_candidate(&t.path, symbol),
            Self::Exact(s) => super::super::explorer::exact_symbol(s, symbol),
        }
    }
}
fn main_candidate(path: &std::path::Path, s: &Symbol) -> bool {
    s.file == path
        && s.name.trim_start_matches("r#") == "main"
        && s.kind == "function"
        && s.container.is_none()
        && s.range.start <= s.selection_range.start
        && s.selection_range.start < s.selection_range.end
        && s.selection_range.end <= s.range.end
}
/// Inspect the complete documentSymbol response before Explorer's presentation cap.
/// Retain a unique exact semantic anchor even if it lies beyond the visible window.
pub(in crate::tui) fn prepare(app: &mut App, id: u64, path: &std::path::Path, symbols: &[Symbol]) {
    let Some(p) = app
        .journey
        .pending
        .as_ref()
        .filter(|p| p.id == id && p.selector.path() == path)
    else {
        return;
    };
    if !matches!(p.selector, Selector::Binary(_)) {
        return;
    }
    let Some(f) = app
        .explorer
        .file
        .as_mut()
        .filter(|f| f.request_id == id && f.path == path)
    else {
        return;
    };
    let mut matches = symbols.iter().filter(|s| main_candidate(path, s));
    let first = matches.next();
    let ambiguous = matches.next().is_some();
    f.symbol_anchor = first.filter(|_| !ambiguous).cloned();
    f.anchor_notice=Some(if ambiguous {
        "Ambiguous entrypoint: multiple top-level main functions from RA; select a symbol manually."
    } else if first.is_none() && symbols.len()>super::super::explorer::FILE_SYMBOL_LIMIT {
        "Entrypoint unresolved / document symbols truncated; absence of main is not proven."
    } else if first.is_none() {
        "Entrypoint unresolved: RA returned no top-level callable main. Macro-generated/conditional entrypoints may be unavailable."
    } else {"Exact RA main found; validating readiness."}.into());
}

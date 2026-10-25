//! Grouped exploration with stable identity, independent of result arrival order.
mod analysis;
mod details;
mod render;
mod state;
use super::app::{project_name, AnalyzerState, App, Pane};
use super::{
    components, theme,
    ui::{heading, pane_block},
};
use crate::{maintainability::FunctionMetrics, metrics::UnmeasuredFile};
pub(super) use details::details;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};
pub(super) use render::{render_details, render_project, render_summary};
#[cfg(test)]
pub(super) use state::ItemId;
pub(super) use state::{index, selected, targets, Group, State, Target};
use std::{collections::BTreeSet, path::PathBuf};

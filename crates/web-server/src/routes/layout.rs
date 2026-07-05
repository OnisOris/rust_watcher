use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use graph_core::{GraphNode, GraphSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::state::AppStateHandle;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutStore {
    pub(crate) nodes: HashMap<String, LayoutNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutNode {
    pub(crate) node_id: String,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) vx: f64,
    pub(crate) vy: f64,
    pub(crate) pinned: Option<bool>,
    pub(crate) updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutNodeInput {
    node_id: String,
    x: f64,
    y: f64,
    vx: Option<f64>,
    vy: Option<f64>,
    pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveLayoutRequest {
    nodes: Vec<LayoutNodeInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveNodeLayoutRequest {
    node: LayoutNodeInput,
}

pub(crate) async fn layout_get(State(state): State<AppStateHandle>) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    match load_layout(&project_root) {
        Ok(layout) => (StatusCode::OK, Json(layout)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load layout: {error}"),
        )
            .into_response(),
    }
}

pub(crate) async fn layout_save(
    State(state): State<AppStateHandle>,
    Json(request): Json<SaveLayoutRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let updated_at = timestamp();
    let mut layout = LayoutStore::default();
    for node in request.nodes {
        layout.nodes.insert(
            node.node_id.clone(),
            layout_node_from_input(node, updated_at.clone()),
        );
    }
    if let Err(error) = save_layout(&project_root, &layout) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save layout: {error}"),
        )
            .into_response();
    }
    apply_layout_store_to_snapshot(&mut state.graph.write(), &layout);
    (StatusCode::OK, Json(layout)).into_response()
}

pub(crate) async fn layout_save_node(
    State(state): State<AppStateHandle>,
    Json(request): Json<SaveNodeLayoutRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut layout = match load_layout(&project_root) {
        Ok(layout) => layout,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load layout: {error}"),
            )
                .into_response();
        }
    };
    let node = layout_node_from_input(request.node, timestamp());
    layout.nodes.insert(node.node_id.clone(), node.clone());
    if let Err(error) = save_layout(&project_root, &layout) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save layout: {error}"),
        )
            .into_response();
    }
    apply_layout_node_to_snapshot(&mut state.graph.write(), &node);
    (StatusCode::OK, Json(node)).into_response()
}

pub(crate) async fn layout_clear(State(state): State<AppStateHandle>) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    match clear_layout(&project_root) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to clear layout: {error}"),
        )
            .into_response(),
    }
}

pub(crate) fn layout_node_from_input(input: LayoutNodeInput, updated_at: String) -> LayoutNode {
    LayoutNode {
        node_id: input.node_id,
        x: input.x,
        y: input.y,
        vx: input.vx.unwrap_or_default(),
        vy: input.vy.unwrap_or_default(),
        pinned: input.pinned,
        updated_at,
    }
}

pub(crate) fn storage_dir_for_project(project_root: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    project_root.display().to_string().hash(&mut hasher);
    let project_hash = format!("{:016x}", hasher.finish());
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("rust-watcher").join(project_hash)
}

pub(crate) fn layout_path(project_root: &Path) -> PathBuf {
    storage_dir_for_project(project_root).join("layout.json")
}

pub(crate) fn views_path(project_root: &Path) -> PathBuf {
    storage_dir_for_project(project_root).join("views.json")
}

pub(crate) fn load_layout(project_root: &Path) -> Result<LayoutStore> {
    let path = layout_path(project_root);
    if !path.exists() {
        return Ok(LayoutStore::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    match serde_json::from_str(&text) {
        Ok(layout) => Ok(layout),
        Err(error) => {
            let corrupt_path = path.with_file_name(format!(
                "layout.corrupt.{}.json",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .unwrap_or_default()
            ));
            let _ = std::fs::rename(&path, &corrupt_path);
            warn!(
                error = %error,
                layout = %path.display(),
                backup = %corrupt_path.display(),
                "corrupt layout cache renamed; starting fresh"
            );
            Ok(LayoutStore::default())
        }
    }
}

pub(crate) fn save_layout(project_root: &Path, layout: &LayoutStore) -> Result<()> {
    let path = layout_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(layout).context("failed to serialize layout")?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn clear_layout(project_root: &Path) -> Result<()> {
    let path = layout_path(project_root);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn apply_saved_layout(snapshot: &mut GraphSnapshot, project_root: &Path) {
    match load_layout(project_root) {
        Ok(layout) => apply_layout_store_to_snapshot(snapshot, &layout),
        Err(error) => warn!(?error, "failed to load saved layout"),
    }
}

pub(crate) fn apply_layout_store_to_snapshot(snapshot: &mut GraphSnapshot, layout: &LayoutStore) {
    for node in &mut snapshot.nodes {
        if let Some(layout_node) = layout.nodes.get(&node.id) {
            apply_layout_node(node, layout_node);
        }
    }
}

pub(crate) fn apply_layout_node_to_snapshot(
    snapshot: &mut GraphSnapshot,
    layout_node: &LayoutNode,
) {
    if let Some(node) = snapshot
        .nodes
        .iter_mut()
        .find(|node| node.id == layout_node.node_id)
    {
        apply_layout_node(node, layout_node);
    }
}

pub(crate) fn apply_layout_node(node: &mut GraphNode, layout_node: &LayoutNode) {
    node.x = layout_node.x;
    node.y = layout_node.y;
    node.vx = layout_node.vx;
    node.vy = layout_node.vy;
    if layout_node.pinned.is_some() {
        node.pinned = layout_node.pinned;
    }
}

pub(crate) fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

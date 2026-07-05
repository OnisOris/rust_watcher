use anyhow::{Context, Result};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

use crate::routes::layout::{timestamp, views_path};
use crate::state::AppStateHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) filters: serde_json::Value,
    pub(crate) focused_node_id: Option<String>,
    pub(crate) collapsed_groups: Vec<String>,
    pub(crate) layout_overrides: serde_json::Value,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedViewsStore {
    pub(crate) views: Vec<SavedView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedViewRequest {
    name: String,
    #[serde(default)]
    filters: serde_json::Value,
    focused_node_id: Option<String>,
    #[serde(default)]
    collapsed_groups: Vec<String>,
    #[serde(default)]
    layout_overrides: serde_json::Value,
}

pub(crate) async fn views_get(State(state): State<AppStateHandle>) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    match load_views(&project_root) {
        Ok(views) => (StatusCode::OK, Json(views)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load views: {error}"),
        )
            .into_response(),
    }
}

pub(crate) async fn views_create(
    State(state): State<AppStateHandle>,
    Json(request): Json<SavedViewRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut store = match load_views(&project_root) {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load views: {error}"),
            )
                .into_response();
        }
    };
    let now = timestamp();
    let view = SavedView {
        id: Uuid::new_v4().to_string(),
        name: request.name,
        filters: request.filters,
        focused_node_id: request.focused_node_id,
        collapsed_groups: request.collapsed_groups,
        layout_overrides: request.layout_overrides,
        created_at: now.clone(),
        updated_at: now,
    };
    store.views.push(view.clone());
    if let Err(error) = save_views(&project_root, &store) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save views: {error}"),
        )
            .into_response();
    }
    (StatusCode::CREATED, Json(view)).into_response()
}

pub(crate) async fn views_update(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SavedViewRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut store = match load_views(&project_root) {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load views: {error}"),
            )
                .into_response();
        }
    };
    let Some(view) = store.views.iter_mut().find(|view| view.id == id) else {
        return (StatusCode::NOT_FOUND, "view not found").into_response();
    };
    view.name = request.name;
    view.filters = request.filters;
    view.focused_node_id = request.focused_node_id;
    view.collapsed_groups = request.collapsed_groups;
    view.layout_overrides = request.layout_overrides;
    view.updated_at = timestamp();
    let response = view.clone();
    if let Err(error) = save_views(&project_root, &store) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save views: {error}"),
        )
            .into_response();
    }
    (StatusCode::OK, Json(response)).into_response()
}

pub(crate) async fn views_delete(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut store = match load_views(&project_root) {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load views: {error}"),
            )
                .into_response();
        }
    };
    let old_len = store.views.len();
    store.views.retain(|view| view.id != id);
    if old_len == store.views.len() {
        return (StatusCode::NOT_FOUND, "view not found").into_response();
    }
    if let Err(error) = save_views(&project_root, &store) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save views: {error}"),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) fn load_views(project_root: &Path) -> Result<SavedViewsStore> {
    let path = views_path(project_root);
    if !path.exists() {
        return Ok(SavedViewsStore::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn save_views(project_root: &Path, views: &SavedViewsStore) -> Result<()> {
    let path = views_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(views).context("failed to serialize views")?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use std::path::PathBuf;

use crate::analysis;
use crate::server::install_watcher;
use crate::state::AppStateHandle;

#[derive(Debug, Deserialize)]
pub(crate) struct OpenProjectRequest {
    path: Option<PathBuf>,
}

pub(crate) async fn open_project(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenProjectRequest>,
) -> impl IntoResponse {
    let root = request
        .path
        .unwrap_or_else(|| state.project_root.read().clone());
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to canonicalize project path: {error}"),
            )
                .into_response();
        }
    };
    *state.project_root.write() = root.clone();
    state.analyzer.set_root(root.clone()).await;
    state.python_ty.set_root(root.clone()).await;
    state.typescript_lsp.set_root(root.clone()).await;
    state.qml_lsp.set_root(root.clone()).await;
    install_watcher(&state, root.clone());
    let index_state = state.clone();
    tokio::spawn(async move {
        analysis::index_and_publish(index_state, root).await;
    });
    (StatusCode::ACCEPTED, Json(state.status.read().clone())).into_response()
}

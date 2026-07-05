use axum::extract::State;
use axum::Json;
use graph_core::AppStatus;
use serde::Serialize;

use crate::state::AppStateHandle;

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    ok: bool,
    version: &'static str,
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: "0.1.0",
    })
}

pub(crate) async fn status(State(state): State<AppStateHandle>) -> Json<AppStatus> {
    Json(state.status.read().clone())
}

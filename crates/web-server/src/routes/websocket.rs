use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use graph_core::ServerMessage;
use tracing::{info, warn};

use crate::state::AppStateHandle;

pub(crate) async fn ws_handler(
    State(state): State<AppStateHandle>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, state))
}

pub(crate) async fn websocket(socket: WebSocket, state: AppStateHandle) {
    info!("websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let initial = ServerMessage::GraphSnapshot(state.graph.read().clone());
    if let Ok(text) = serde_json::to_string(&initial) {
        let _ = sender.send(Message::Text(text.into())).await;
    }

    let mut rx = state.ws_tx.subscribe();
    let forward = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            match serde_json::to_string(&message) {
                Ok(text) => {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(error) => warn!(?error, "failed to serialize websocket message"),
            }
        }
    });

    while let Some(message) = receiver.next().await {
        if matches!(message, Ok(Message::Close(_)) | Err(_)) {
            break;
        }
    }
    forward.abort();
    info!("websocket disconnected");
}

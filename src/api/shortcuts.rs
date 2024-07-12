use racoon::core::websocket::WebSocket;
use serde_json::json;

pub async fn internal_server_error(websocket: &WebSocket) {
    let _ = websocket
        .send_json(&json!({
            "status": "failed",
            "status_code": "internal_server_error",
            "message": "Internal Server Error",
        }))
        .await;
}

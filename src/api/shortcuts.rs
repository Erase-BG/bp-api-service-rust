use racoon::core::websocket::WebSocket;
use serde_json::json;

pub async fn internal_server_error(websocket: &WebSocket) {
    websocket.send_json(&json!({
        "status": "failed",
        "status_code": "internal_server_error",
        "message": "Internal Server Error",
    }));
}

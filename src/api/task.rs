use std::str::FromStr;
use std::sync::Arc;

use racoon::core::websocket::{Message, WebSocket};

use serde_json::{json, Value};
use tej_protoc::protoc::File;

use tokio::fs;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::api::shortcuts;
use crate::clients::bp_request_client::BPRequestClient;
use crate::db::models::BackgroundRemoverTask;
use crate::SharedContext;

///
/// The abstraction for `BPRequestClient` to send task. Takes `BackgroundRemoverTask` instance, preprocesses and sends image to bp server for
/// processing.
///
pub async fn send(
    bp_request_client: Arc<BPRequestClient>,
    task: &BackgroundRemoverTask,
) -> std::io::Result<()> {
    let message = json!({
        "task_id": task.key.to_string(),
    });

    let mut original_image_file = fs::File::open(&task.original_image_path).await?;
    let mut buffer = vec![];
    original_image_file.read_to_end(&mut buffer).await?;
    let file = File::new(b"original.jpg".to_vec(), buffer);
    let files = [file];

    // Sends files to BP Server.
    bp_request_client.send(&files, &message).await?;
    Ok(())
}

pub async fn handle_ws_received_message(
    task_group: &Uuid,
    websocket: &WebSocket,
    shared_context: &SharedContext,
    message: Message,
) {
    match message {
        Message::Text(text) => {
            println!("Received: {}", text);

            let json = match Value::from_str(&text) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("Failed to parse text to JSON. Error: {}", error);

                    // Invalid JSON message is received. Returns error response to the client.
                    let _ = websocket
                        .send_json(&json!({
                            "status": "failed",
                            "status_code": "invalid_message_format",
                            "message": "Not a valid message format. Expected type JSON.",
                        }))
                        .await;
                    return;
                }
            };

            if let Some(key) = json.get("key") {
                let key = match Uuid::parse_str(&key.to_string()) {
                    Ok(uuid) => uuid,
                    Err(error) => {
                        eprint!("Failed to parse key to UUID. Error: {}", error);

                        let _ = websocket
                            .send_json(&json!({
                                "status": "failed",
                                "status_code": "invalid_message_format",
                                "message": "Invalid key format.",
                            }))
                            .await;
                        return;
                    }
                };

                handle_process_image_command(task_group, key, websocket, shared_context).await;
            }
        }
        _ => {}
    }
}

pub async fn handle_process_image_command(
    task_group: &Uuid,
    key: Uuid,
    websocket: &WebSocket,
    shared_context: &SharedContext,
) {
    let db_wrapper = shared_context.db_wrapper.clone();
    let instance = match BackgroundRemoverTask::fetch(db_wrapper, &key).await {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!("Failed to fetch instance. Error: {}", error);
            shortcuts::internal_server_error(websocket).await;
            return;
        }
    };

    if &instance.task_group != task_group {

    }
}

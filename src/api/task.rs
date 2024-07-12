use std::env;
use std::str::FromStr;
use std::sync::Arc;

use racoon::core::websocket::{Message, WebSocket};

use serde_json::{json, Value};
use tej_protoc::protoc::File;

use tokio::fs;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::api::shortcuts::{self, internal_server_error};
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

            let key;
            if let Some(value) = json.get("key") {
                key = value;
            } else {
                return;
            }

            if let Some(key) = key.as_str() {
                let key = match Uuid::parse_str(key) {
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
    let instance = match BackgroundRemoverTask::fetch(db_wrapper.clone(), &key).await {
        Ok(instance) => instance,
        Err(error) => {
            match error {
                sqlx::Error::RowNotFound => {
                    let _ = websocket
                        .send_json(&json!({
                            "status": "failed",
                            "status_code": "not_found",
                            "message": "Image with this key does not exist."
                        }))
                        .await;
                }
                _ => {
                    eprintln!("Failed to fetch instance. Error: {}", error);
                    shortcuts::internal_server_error(websocket).await;
                }
            }
            return;
        }
    };

    if &instance.task_group != task_group {
        let _ = websocket
            .send_json(&json!({
                "status": "failed",
                "status_code": "permission_error",
                "message": "This task_group does not have permission to process image with this key."
            }))
            .await;
        return;
    }

    let hard_process_var = env::var("PROCESS_HARD").unwrap_or("false".to_string());
    let is_process_hard = hard_process_var.to_lowercase() == "true";
    let is_processing = instance.processing.unwrap_or(false);

    // Requires image processing if env var PROCESS_HARD is specified or processed_image_path is
    // None.
    let need_processing = is_process_hard || !is_processing;

    if !need_processing {
        // Image is already processed.
        let serialized = match instance.serialize() {
            Ok(serialized) => serialized,
            Err(error) => {
                eprintln!("Failed to serialize data. Error: {}", error);
                internal_server_error(websocket).await;
                return;
            }
        };

        let _ = websocket
            .send_json(&json!({
                "status": "success",
                "status_code": "result",
                "data": serialized,
            }))
            .await;
    } else {
        // Send this image for processing.
        println!("Sending task: {} to Bp Server.", instance.task_id);
        match send(shared_context.bp_request_client.clone(), &instance).await {
            Ok(()) => {
                println!("Sent task successfully for processing.");
                let _ = BackgroundRemoverTask::update_processing_state(
                    db_wrapper.clone(),
                    &instance.key,
                    true,
                )
                .await;
            }
            Err(error) => {
                eprintln!("Failed to send task to bp server. Error: {}", error);
            }
        };
    }
}

pub async fn handle_response_received_from_bp_server(
    shared_context: SharedContext,
    files: Vec<File>,
    messsage: Value,
) {
    println!("Received from bp server: {}", messsage);

    let task_id_option = messsage.get("task_id");
    let task_id_str;

    if let Some(task_id_value) = task_id_option {
        if let Some(str_value) = task_id_value.as_str() {
            task_id_str = str_value;
        } else {
            eprintln!("The received task id is not a string.");
            return;
        }
    } else {
        eprintln!("Ignoring result. The task_id in bp server response is missing.");
        return;
    }

    let task_id = match Uuid::parse_str(task_id_str) {
        Ok(uuid) => uuid,
        Err(error) => {
            eprintln!("Failed to parse received task_id to UUID. Error: {}", error);
            return;
        }
    };

    let instance = match BackgroundRemoverTask::fetch(shared_context.db_wrapper, &task_id).await {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!("Failed to fetch background remover task. Error: {}", error);

            // Nothing can be done.
            return;
        }
    };

    let ws_clients = shared_context.ws_clients;
    let websockets = ws_clients.get_all(&instance.task_group).await;

    for websocket in websockets {
        let _ = websocket.send_json(&messsage).await;
    }
}

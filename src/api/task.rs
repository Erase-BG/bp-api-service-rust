use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use racoon::core::websocket::{Message, WebSocket};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tej_protoc::protoc::File;

use tokio::fs;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::api::shortcuts::{self, internal_server_error};
use crate::clients::bp_request_client::BPRequestClient;
use crate::db::models::{BackgroundRemoverTask, UpdateBackgroundRemoverTask};
use crate::utils::{path_utils, save_utils};
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

    let media_root = match env::var("MEDIA_ROOT") {
        Ok(path) => PathBuf::from(path),
        Err(error) => {
            eprintln!("MEDIA_ROOT environment variable is missing.");
            return Err(std::io::Error::other(error));
        }
    };

    let original_image_file_path = path_utils::file_path_from_relative_url(
        media_root,
        PathBuf::from(&task.original_image_path),
    );
    let mut original_image_file = fs::File::open(&original_image_file_path).await?;
    let mut buffer = vec![];
    original_image_file.read_to_end(&mut buffer).await?;
    let file = File::new(b"original.jpg".to_vec(), buffer);
    let files = [file];

    // Sends files to BP Server.
    let result = tokio::time::timeout(
        Duration::from_secs(12),
        bp_request_client.send(&files, &message),
    )
    .await?;

    println!("Send task result: {:?}", result);
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

#[derive(Serialize, Deserialize, Debug)]
pub struct BPResponse {
    task_id: Uuid,
    status: String,
    status_code: String,
    message: Option<String>,
    timestamps: Option<Value>,
}

pub async fn handle_response_received_from_bp_server(
    shared_context: SharedContext,
    files: Vec<File>,
    messsage: Value,
) {
    println!("Received from bp server: {}", messsage);
    let bp_response: BPResponse = match serde_json::from_value(messsage) {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!(
                "Invalid format message received from BP Server. Error: {}",
                error
            );
            return;
        }
    };

    let instance =
        match BackgroundRemoverTask::fetch(shared_context.db_wrapper.clone(), &bp_response.task_id)
            .await
        {
            Ok(instance) => instance,
            Err(error) => {
                eprintln!("Failed to fetch background remover task. Error: {}", error);

                // Nothing can be done.
                return;
            }
        };

    if bp_response.status == "success" {
        let is_fake_processed = bp_response.status_code == "fake_process_completed";
        handle_files_received_from_bp_server(shared_context, instance, &files, is_fake_processed)
            .await;
    } else {
        let websockets = shared_context
            .ws_clients
            .get_all(&instance.task_group)
            .await;

        for websocket in websockets {
            let _ = websocket
                .send_json(&json!({
                    "status": bp_response.status,
                    "status_code": bp_response.status_code,
                    "message": bp_response.message,
                }))
                .await;
        }
    }
}

async fn handle_files_received_from_bp_server(
    shared_context: SharedContext,
    instance: BackgroundRemoverTask,
    files: &Vec<File>,
    is_fake_processed: bool,
) {
    // Saves files received from BP Server. These paths are absolute and should not be used for
    // saving in database.
    let (transparent_image_path, mask_image_path, preview_transparent_image_path) =
        match save_utils::save_files_received_from_bp_server(&instance, &files, is_fake_processed)
            .await
        {
            Ok(paths) => paths,
            Err(error) => {
                eprintln!(
                    "Failed to save files received from bp server. Error: {}",
                    error
                );

                broadcast_internal_server_error(shared_context.clone(), &instance.task_group).await;
                return;
            }
        };

    let media_root = match env::var("MEDIA_ROOT") {
        Ok(path) => PathBuf::from(path),
        Err(error) => {
            eprintln!(
                "The MEDIA_ROOT path is not specified in environment variable. Error: {}",
                error
            );
            broadcast_internal_server_error(shared_context.clone(), &instance.task_group).await;
            return;
        }
    };

    // Converts to relative media url for saving in database.
    let relative_mask_image_path =
        path_utils::relative_media_url_from_full_path(&media_root, &mask_image_path);
    let relative_transparent_image_path =
        path_utils::relative_media_url_from_full_path(&media_root, &transparent_image_path);
    let relative_preview_transparent_image_path =
        path_utils::relative_media_url_from_full_path(&media_root, &preview_transparent_image_path);

    let update_task = UpdateBackgroundRemoverTask {
        key: instance.key,
        logs: instance.logs,
        mask_image_path: relative_mask_image_path.to_string_lossy().to_string(),
        processed_image_path: relative_transparent_image_path
            .to_string_lossy()
            .to_string(),
        preview_processed_image_path: relative_preview_transparent_image_path
            .to_string_lossy()
            .to_string(),
    };

    match BackgroundRemoverTask::update_task(shared_context.db_wrapper.clone(), &update_task).await
    {
        Ok(()) => {}
        Err(error) => {
            eprintln!("Failed to update task record in database. Error: {}", error);
            broadcast_internal_server_error(shared_context.clone(), &instance.task_group).await;
            return;
        }
    };

    // Marks this task as completed.
    match BackgroundRemoverTask::update_processing_state(
        shared_context.db_wrapper.clone(),
        &instance.key,
        false,
    )
    .await
    {
        Ok(()) => {}
        Err(error) => {
            eprintln!("Failed to update processing state. Error: {}", error);
            broadcast_internal_server_error(shared_context.clone(), &instance.task_group).await;
            return;
        }
    }

    let fresh_instance = match BackgroundRemoverTask::fetch(
        shared_context.db_wrapper.clone(),
        &instance.key,
    )
    .await
    {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!(
                "Failed to fetch background remover task instance. Error: {}",
                error
            );
            broadcast_internal_server_error(shared_context.clone(), &instance.task_group).await;
            return;
        }
    };

    let serialized = match fresh_instance.serialize() {
        Ok(serialized) => serialized,
        Err(error) => {
            eprintln!(
                "Failed to serialize background remover task instance. Error: {}",
                error
            );
            broadcast_internal_server_error(shared_context, &fresh_instance.task_group).await;
            return;
        }
    };

    let websockets = shared_context
        .ws_clients
        .get_all(&fresh_instance.task_group)
        .await;

    // Broadcasts response to all websocket clients.
    for websocket in websockets {
        let _ = websocket
            .send_json(&json!({
                "status": "success",
                "status_code": "result",
                "data": serialized
            }))
            .await;
    }
}

async fn broadcast_internal_server_error(shared_context: SharedContext, task_group: &Uuid) {
    // Broadcast internal server error to all clients.
    let websockets = shared_context.ws_clients.get_all(&task_group).await;
    for websocket in websockets {
        shortcuts::internal_server_error(&websocket).await;
    }
}

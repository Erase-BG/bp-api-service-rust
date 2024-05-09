use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use uuid::Uuid;

use tej_protoc::protoc::decoder::DecodedResponse;
use tej_protoc::protoc::File;

use crate::db::models::{BackgroundRemoverTask, UpdateBackgroundRemoverTask};
use crate::implementations::websocket::services::build_standard_response;
use crate::implementations::websocket::services::task_ws::update_processing_state;

use crate::ResponseHandlerSharedData;
use crate::utils::file_utils::media_root_relative;
use crate::utils::image_utils;

pub async fn handle_response_received_from_server(decoded_response: DecodedResponse,
                                                  shared_data: Arc<ResponseHandlerSharedData>) {
    let message = String::from_utf8_lossy(&decoded_response.message).to_string();
    log::info!("Received from bp server: {}", message);

    let parsed: Value = serde_json::from_str(&message).unwrap();
    let task_id;

    if let Some(status) = parsed.get("status").and_then(Value::as_str) {

        // Extracts task id from the bp server response
        if let Some(value) = parsed.get("task_id").and_then(Value::as_str) {
            match Uuid::from_str(value) {
                Ok(uuid) => {
                    task_id = uuid;
                }
                Err(error) => {
                    log::error!("Failed to convert task_id value to UUID. Error: {}", error);
                    return;
                }
            }
        } else {
            log::warn!("Task Id is missing from the bp server response. So no handler is called.");
            return;
        };

        let files = decoded_response.files;

        if status == "success" {
            handle_success_result(task_id, parsed, files, shared_data).await;
        } else if status == "progress_update" {
            handle_progress_update(task_id, parsed, shared_data).await;
        } else if status == "failed" {
            handle_failed_result(task_id, parsed, shared_data).await;
        } else {
            log::warn!("Event type {:?} not handled", status);
        }
    } else {
        log::warn!("Status is missing from the response. No further action performed.");
    }
}

async fn handle_success_result(task_id: Uuid, message: Value, files: Vec<File>, shared_data: Arc<ResponseHandlerSharedData>) {
    let db_wrapper = &shared_data.db_wrapper;

    let mut logs = Value::from_str("{}").unwrap();
    if let Some(logs_value) = logs.as_object_mut() {
        if let Some(value) = message.get("timestamps") {
            logs_value.insert("timestamps".to_string(), value.clone());
        }
    }

    // Fetching old instance before update
    let old_instance = match BackgroundRemoverTask::fetch(
        db_wrapper.clone(), &task_id).await {
        Ok(task) => task,
        Err(error) => {
            log::error!(
                "Failed to fetch old background remover task instance from database. Error {}",
                error
            );
            return;
        }
    };

    // Extracts file filename from original image path
    let filename;
    if let Some(value) = PathBuf::from(&old_instance.original_image_path).file_name() {
        filename = value.to_string_lossy().to_string();
    } else {
        log::error!("Unable to get filename from path: {}", &old_instance.original_image_path);
        return;
    }

    let sub_dir = PathBuf::from("background-remover");

    // Save images in separate task
    let save_handler = tokio::task::spawn_blocking(
        move || {
            return image_utils::save_task_images_to_media(
                &filename.clone(),
                &old_instance.key.clone(),
                &files,
                Some(&sub_dir.clone()),
            );
        }
    ).await;

    let (processed_image_path, preview_processed_image_path, mask_image_path);

    match save_handler {
        Ok(save_result) => {
            match save_result {
                Ok(paths) => {
                    processed_image_path = paths.0;
                    preview_processed_image_path = paths.1;
                    mask_image_path = paths.2;
                }
                Err(error) => {
                    log::error!("Failed to save images. Error: {}", error);
                    return;
                }
            };
        }
        Err(error) => {
            log::error!("Failed to save images in separate thread. {}", error);
            return;
        }
    };

    // Converts absolute mask image path to relative
    let mask_image_path_relative = match media_root_relative(&PathBuf::from(&mask_image_path)) {
        Ok(value) => value.to_string_lossy().to_string(),
        Err(error) => {
            log::error!("Failed to generate relative media url for mask image. Error: {}", error);
            handle_failed_result(task_id, message, shared_data).await;
            return;
        }
    };

    // Converts absolute processed image path to relative
    let processed_image_path_relative = match media_root_relative(
        &PathBuf::from(&processed_image_path)) {
        Ok(value) => value.to_string_lossy().to_string(),
        Err(error) => {
            log::error!("Failed to generate relative media url for processed image. Error: {}", error);
            handle_failed_result(task_id, message, shared_data).await;
            return;
        }
    };

    // Converts absolute preview processed mask image path to relative
    let preview_processed_image_path_relative = match media_root_relative(
        &PathBuf::from(&preview_processed_image_path)) {
        Ok(value) => value.to_string_lossy().to_string(),
        Err(error) => {
            log::error!("Failed to generate relative media url for preview processed image. Error: {}", error);
            handle_failed_result(task_id, message, shared_data).await;
            return;
        }
    };

    let update_task = UpdateBackgroundRemoverTask {
        key: task_id,
        mask_image_path: mask_image_path_relative,
        processed_image_path: processed_image_path_relative,
        preview_processed_image_path: preview_processed_image_path_relative,
        logs: Some(logs),
    };

    // Update task record in database
    match BackgroundRemoverTask::update_task(db_wrapper.clone(), &update_task).await {
        Ok(_) => {}
        Err(error) => {
            log::error!("Failed to update task in database. Error {}", error);
            return;
        }
    }

    let background_remover_task = match BackgroundRemoverTask::fetch(
        db_wrapper.clone(), &task_id).await {
        Ok(task) => task,
        Err(error) => {
            log::error!("Failed to fetch background remover task from database. Error {}", error);
            return;
        }
    };

    log::info!("Updating processing state");
    update_processing_state(db_wrapper.clone(), &task_id, false).await;
    broadcast_ws_success(background_remover_task, message, shared_data).await;
}

async fn handle_progress_update(task_id: Uuid, message: Value, shared_data: Arc<ResponseHandlerSharedData>) {
    let background_remover_task = match BackgroundRemoverTask::fetch(
        shared_data.db_wrapper.clone(), &task_id).await {
        Ok(task) => task,
        Err(error) => {
            log::error!("Failed to fetch background remover task from database. Error {}", error);
            return;
        }
    };

    broadcast_ws(&background_remover_task.task_group, message, shared_data).await;
}

async fn handle_failed_result(task_id: Uuid, message: Value, shared_data: Arc<ResponseHandlerSharedData>) {
    log::error!("Error message received from BP Server: {}", message);

    let instance = match BackgroundRemoverTask::fetch(shared_data.db_wrapper.clone(), &task_id).await {
        Ok(value) => value,
        Err(error) => {
            log::error!(
                "No background remover task instance found. Ignoring error message. Error: {}",
                error
            );
            return;
        }
    };

    let mut data: Option<Value> = None;
    match instance.serialize() {
        Ok(value) => {
            data = Some(value);
        }
        Err(error) => {
            log::error!("Failed to serialize instance. Error: {}", error);
        }
    };

    // Actual reason hidden to user
    let response = build_standard_response(
        "failed",
        "internal_server_error",
        Some("Failed to process image. Reason: Internal Server Error"),
        data,
        None,
    );

    println!("Sending");
    broadcast_ws(&instance.task_group, response, shared_data).await;
    println!("Sent");
}

async fn broadcast_ws_success(instance: BackgroundRemoverTask, _: Value,
                              shared_data: Arc<ResponseHandlerSharedData>) {
    let serialized = match instance.serialize() {
        Ok(serialized) => serialized,
        Err(error) => {
            log::error!(
                "Failed to serialize background remover task instance to JSON. Error: {}",
                error
            );
            return;
        }
    };

    let response = build_standard_response("success", "result", None,
                                           Some(serialized), None);

    broadcast_ws(&instance.task_group, response, shared_data).await;
}

async fn broadcast_ws(task_group: &Uuid, message: Value, shared_data: Arc<ResponseHandlerSharedData>) {
    let mut sessions = shared_data.websocket_connections.sessions
        .lock().await;

    if let Some(ws_wrappers) = sessions.get_mut(&task_group.to_string()) {
        log::debug!("WS client found in ws sessions by task id(key): {}", task_group);

        for ws_wrapper in ws_wrappers.iter() {
            match ws_wrapper.websocket.send_json(&message).await {
                Ok(()) => {
                    log::debug!("Sent from handler to websocket client: {}", message);
                }
                Err(closed) => {
                    log::debug!(
                    "Failed to send message to ws client. Connection closed. More info: {}",
                    closed
                );
                }
            };
        }
    } else {
        log::error!("Keys: {:?} Searched {:?}", sessions.keys(), task_group);
        log::debug!("WS client not found. May be connection no more exist. Task id: {}", task_group);
    }
}
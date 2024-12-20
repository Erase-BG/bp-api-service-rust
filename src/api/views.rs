use std::env;
use std::path::PathBuf;

use racoon::core::request::Request;
use racoon::core::response::status::ResponseStatus;
use racoon::core::response::{HttpResponse, JsonResponse, Response};
use racoon::core::shortcuts::SingleText;
use racoon::core::websocket::WebSocket;
use racoon::forms::FormValidator;

use serde_json::json;
use uuid::Uuid;

use crate::api::forms::PublicImageUploadForm;
use crate::db::models::{BackgroundRemoverTask, NewBackgroundRemoverTask};
use crate::utils::path_utils;
use crate::SharedContext;

use super::task;

pub async fn public_upload(request: Request) -> Response {
    if request.method != "POST" {
        return HttpResponse::ok().body("This request method is not supported.");
    }

    let form = PublicImageUploadForm::new();

    // If form contains error, returns error response.
    let validated_form = match form.validate(&request).await {
        Ok(form) => form,
        Err(error) => {
            eprintln!("Errors: {:?}", error);

            return JsonResponse::bad_request().body(json!({
                "status": "failed",
                "status_code": "form_error",
                "field_errors": error.field_errors,
                "other_errors": error.others,
            }));
        }
    };

    // Handles validated form data
    let original_image = validated_form.original_image.value().await;
    let shared_context: &SharedContext = request.context().expect("SharedContext is missing.");

    // Unique id for each task. Used for database lookup and saving files.
    let task_id = Uuid::new_v4();

    let original_image_save_path = match path_utils::generate_save_path(
        path_utils::ForImage::OriginalImage(&task_id, &original_image.filename),
    ) {
        Ok(path) => path,
        Err(error) => {
            eprintln!(
                "Failed to generate save path for original image. Error: {}",
                error
            );

            return JsonResponse::internal_server_error().body(json!({
                "status": "failed",
                "status_code": "internal_server_error"
            }));
        }
    };

    // Moves original image to the configured destination.
    println!(
        "Moving file from: {:?} to {:?}",
        original_image.temp_path, original_image_save_path
    );
    let result = tokio::fs::copy(original_image.temp_path, &original_image_save_path).await;

    let destination = std::path::PathBuf::from(&original_image_save_path);
    if !destination.exists() {
        eprintln!("File move called but not moved. More info:");
        eprintln!("{:?}", result);

        return JsonResponse::internal_server_error().body(json!({
            "status": "failed",
            "message": "Internal server error.",
        }))
    }

    // Saves to database
    let task_group = validated_form.task_group.value().await;
    let country = validated_form.country.value().await;
    let user_identifier = validated_form.user_identifier.value().await;

    let media_root = match env::var("MEDIA_ROOT") {
        Ok(path) => PathBuf::from(path),
        Err(error) => {
            eprintln!(
                "The MEDIA_ROOT environment variable is missing. Error: {}",
                error
            );
            return JsonResponse::internal_server_error().body(json!({
                "status": "failed",
                "status_code": "internal_server_error",
                "message": "Internal Server Error"
            }));
        }
    };

    let relative_original_image_media_url =
        path_utils::relative_media_url_from_full_path(&media_root, &original_image_save_path);

    let preview_original_image_media_url =
        path_utils::relative_media_url_from_full_path(&media_root, &original_image_save_path);

    let new_task = NewBackgroundRemoverTask {
        country,
        key: task_id,
        original_image_path: relative_original_image_media_url
            .to_string_lossy()
            .to_string(),
        preview_original_image_path: preview_original_image_media_url
            .to_string_lossy()
            .to_string(),
        task_group,
        user_identifier,
    };

    match BackgroundRemoverTask::insert_new_task(shared_context.db_wrapper.clone(), &new_task).await
    {
        Ok(()) => {}
        Err(error) => {
            eprint!("Failed to insert new task to database. Error: {}", error);
            return JsonResponse::ok().body(json!({
                "status": "success",
                "filename": original_image.filename
            }));
        }
    };

    // Sends this image for processing.
    JsonResponse::ok().body(json!({
        "status": "success",
        "status_code": "image_upload",
        "data": {
            "key": new_task.key,
            "task_group": new_task.task_group,
        }
    }))
}

pub async fn task_details_view(request: Request) -> Response {
    let context = request.context::<SharedContext>().unwrap();
    let task_id = match Uuid::parse_str(request.path_params.value("task_id").unwrap()) {
        Ok(uuid) => uuid,
        Err(error) => {
            log::error!("{}", error);

            return JsonResponse::bad_request().body(json!({
                "error": "Not a valid task id format."
            }));
        }
    };

    let instance = match BackgroundRemoverTask::fetch(context.db_wrapper.clone(), &task_id).await {
        Ok(instance) => instance,
        Err(error) => {
            log::error!("{}", error);

            return JsonResponse::not_found().body(json!({
                "error": "Invalid task id."
            }));
        }
    };

    let serialized = match instance.serialize() {
        Ok(serialized) => serialized,
        Err(error) => {
            log::error!("{}", error);
            return JsonResponse::internal_server_error().empty();
        }
    };

    JsonResponse::ok().body(serialized)
}

pub async fn listen_processing_ws(request: Request) -> Response {
    let (websocket, connected) = WebSocket::from(&request).await;
    if !connected {
        return websocket.bad_request().await;
    }

    let task_group_str = request
        .path_params
        .value("task_group")
        .expect("Task Group is missing.");

    // If invalid task group is received, sends error response and shutdowns websocket connection.
    let task_group = match Uuid::parse_str(task_group_str) {
        Ok(uuid) => uuid,
        Err(error) => {
            eprintln!("Failed to parse task_group to UUID. Error: {}", error);

            let _ = websocket
                .send_json(&json!({
                    "status": "failed",
                    "status_code": "invalid_path_format",
                    "message": "Invalid task group."
                }))
                .await;
            return websocket.exit();
        }
    };

    // Access shared resources.
    let shared_context: &SharedContext = request.context().expect("SharedContext is missing.");
    let ws_clients = shared_context.ws_clients.clone();

    // Adds this websocket connection to ws_clients. Until all references are dropped, it will stay
    // alive.
    ws_clients.add(&task_group, websocket.clone()).await;

    while let Some(message) = websocket.message().await {
        task::handle_ws_received_message(&task_group, &websocket, shared_context, message).await;
    }

    // Removes websocket instance from ws_clients.
    ws_clients.remove(&task_group, websocket.clone()).await;
    websocket.exit()
}

///
/// Endpoint for displaying all the background remover tasks.
///
pub async fn tasks_view(request: Request) -> Response {
    let shared_context = request.context::<SharedContext>().unwrap();

    let page_num: u32;
    if let Some(param_page) = request.query_params.value("page") {
        // Type casts page string to u32. If fails returns JSON error
        page_num = match param_page.parse::<u32>() {
            Ok(value) => value,
            Err(error) => {
                log::error!(
                    "Page number string to u32 conversion error. Error: {:?}",
                    error
                );
                return JsonResponse::bad_request().body(json!({
                    "status": "failed",
                    "status_code": "bad_query",
                    "message": "Invalid page format",
                }));
            }
        };
    } else {
        page_num = 1;
    }

    let models =
        match BackgroundRemoverTask::fetch_by_page(shared_context.db_wrapper.clone(), page_num)
            .await
        {
            Ok(models) => models,
            Err(error) => {
                println!("Failed to fetch models. Error: {}", error);

                return JsonResponse::internal_server_error().body(json!({
                    "status": "failed",
                    "status_code": "internal_server_error",
                }));
            }
        };

    let mut values = vec![];
    for instance in models {
        match instance.serialize_full() {
            Ok(serialized) => {
                values.push(serialized);
            }

            Err(error) => {
                log::error!("Failed to serialize. Error: {}", error);
            }
        }
    }

    let total = match BackgroundRemoverTask::length(shared_context.db_wrapper.clone()).await {
        Ok(value) => value,
        Err(error) => {
            log::error!("Failed to get length: Error: {}", error);
            return JsonResponse::internal_server_error().empty();
        }
    };

    // Hard coded base url
    let base_url = "https://apistaging.erasebg.org/v1/remove-tasks/";
    let next_url = format!("{}?page=", page_num + 1);
    let previous_url;

    if page_num == 0 {
        previous_url = Some(format!("{}?page={}", base_url, page_num - 1));
    } else {
        previous_url = None;
    }

    JsonResponse::ok().body(json!({
        "count": total,
        "next": next_url,
        "previous": previous_url,
        "results": values
    }))
}

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use log::debug;
use racoon::core::forms::{Files, FormData};
use racoon::core::request::Request;
use racoon::core::response::status::ResponseStatus;
use racoon::core::response::{HttpResponse, JsonResponse, Response};
use racoon::core::shortcuts::SingleText;
use racoon::core::websocket::WebSocket;

use serde_json::json;

use tokio::time::sleep;
use uuid::Uuid;

use crate::db::models::{BackgroundRemoverTask, NewBackgroundRemoverTask};
use crate::forms::ImageUploadForm;
use crate::implementations::websocket::services::build_standard_response;
use crate::implementations::websocket::services::task_ws::listen_ws_message;
use crate::utils::file_utils::{media_root_relative, save_temp_file_to_media};
use crate::utils::image_utils;
use crate::SharedContext;

async fn upload_common(
    shared_context: &SharedContext,
    form_data: &FormData,
    files: &Files,
) -> Result<BackgroundRemoverTask, Response> {
    // Returns validated form
    let validated_form = match ImageUploadForm::validate(&form_data, &files) {
        Ok(form) => form,
        Err(error) => {
            return Err(JsonResponse::bad_request().body(build_standard_response(
                "failed",
                "form_error",
                None,
                None,
                Some(error),
            )));
        }
    };

    Ok(save_instance(shared_context, &validated_form).await?)
}

async fn save_instance(
    shared_context: &SharedContext,
    validated_form: &ImageUploadForm<'_>,
) -> Result<BackgroundRemoverTask, Response> {
    let task_id = Uuid::new_v4();
    let user_identifier = None;

    // Saves uploaded image to the disk
    // The save dir looks like this /{project_name}/{media_root}/{task_id}/original/
    let sub_dir = Some(format!(
        "background-remover/{}/original/",
        task_id.to_string()
    ));
    log::debug!("Saving temp image to database.");

    // Path where the original image is saved
    let original_image_path = match save_temp_file_to_media(&validated_form.original_image, sub_dir)
    {
        Ok(path) => path,
        Err(error) => {
            log::error!("Failed to save temp media file. Error: {}", error);
            return Err(JsonResponse::bad_request().empty());
        }
    };

    // Saves preview original image. Creates background-remover directory after media root dir.
    let original_image_path_buf = PathBuf::from(&original_image_path);
    let sub_dir = PathBuf::from("background-remover");
    let preview_original_saved_path = match image_utils::save_media_preview_original_image(
        &task_id,
        Some(&sub_dir),
        &original_image_path_buf,
        (600, 600),
    ) {
        Ok(path) => path,
        Err(error) => {
            log::error!(
                "Failed to save preview original image {:?}. Error: {}",
                original_image_path_buf,
                error
            );
            return Err(JsonResponse::bad_request().empty());
        }
    };

    // Converts Task group String to UUID
    let task_group = match Uuid::from_str(validated_form.task_group.as_str()) {
        Ok(uuid) => uuid,
        Err(error) => {
            log::error!(
                "Failed to convert task group to UUID format. Error {:?}",
                error
            );
            let error_response = build_standard_response(
                "failed",
                "invalid_format",
                Some("Invalid task group format"),
                None,
                None,
            );
            return Err(JsonResponse::bad_request().body(error_response));
        }
    };

    // Extract country value if present
    let country;
    if let Some(value) = validated_form.country {
        country = Some(value.to_string());
    } else {
        country = None;
    }

    // Above path can be absolute if set in MEDIA_ROOT
    // Converts absolute path to relative path to store in database.
    let original_image_path_relative = match media_root_relative(&original_image_path) {
        Ok(value) => value.to_string_lossy().to_string(),
        Err(error) => {
            log::error!(
                "Failed to generate relative media url for original image. Error: {}",
                error
            );
            return Err(JsonResponse::internal_server_error().empty());
        }
    };

    // Extracts relative media root folder
    let preview_original_image_path_relative =
        match media_root_relative(&preview_original_saved_path) {
            Ok(value) => value.to_string_lossy().to_string(),
            Err(error) => {
                log::error!(
                    "Failed to generate relative media url for preview original image. Error: {}",
                    error
                );
                return Err(JsonResponse::internal_server_error().empty());
            }
        };

    // Prepares necessary fields required for inserting new task record in the database.
    let new_background_remover_task = NewBackgroundRemoverTask {
        key: task_id,
        task_group,
        original_image_path: original_image_path_relative,
        preview_original_image_path: preview_original_image_path_relative,
        country,
        user_identifier,
    };

    // Inserts the new task record in the database
    log::debug!("Inserting new task to db");
    match BackgroundRemoverTask::insert_new_task(
        shared_context.db_wrapper.clone(),
        &new_background_remover_task,
    )
    .await
    {
        Ok(_) => {
            log::debug!(
                "New task saved in database. Task id: {:?}",
                new_background_remover_task.key
            );
        }
        Err(error) => {
            log::error!("Failed to insert new task. Error: {}", error);
            return Err(JsonResponse::internal_server_error().empty());
        }
    }

    // Returns full instance of BackgroundRemoverTask model if successful
    return match BackgroundRemoverTask::fetch(shared_context.db_wrapper.clone(), &task_id).await {
        Ok(task) => Ok(task),
        Err(error) => {
            log::error!("Failed to fetch background remover task. Error: {}", error);
            Err(JsonResponse::internal_server_error().empty())
        }
    };
}

pub async fn public_upload_view(mut request: Request) -> Response {
    if request.method != "POST" {
        return JsonResponse::not_found().body(json!({
            "error": "Page not found"
        }));
    }

    let (form_data, files) = request.parse().await;

    let context = request.context::<SharedContext>().unwrap();
    let instance = match upload_common(context, &form_data, &files).await {
        Ok(instance) => instance,
        Err(error) => {
            return error;
        }
    };

    let serialized = match instance.serialize() {
        Ok(serialized) => serialized,
        Err(error) => {
            log::error!("{}", error);
            return JsonResponse::internal_server_error().empty();
        }
    };

    let response = build_standard_response("success", "image_upload", None, Some(serialized), None);

    JsonResponse::ok().body(response)
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
                return JsonResponse::bad_request().body(build_standard_response(
                    "failed",
                    "bad_query",
                    Some("Invalid page format"),
                    None,
                    None,
                ));
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

                return JsonResponse::internal_server_error().body(build_standard_response(
                    "failed",
                    "internal_server_error",
                    None,
                    None,
                    None,
                ));
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

pub async fn ws_view(request: Request) -> Response {
    let (mut websocket, connected) = WebSocket::from_opt(&request, false).await;

    if !connected {
        return HttpResponse::bad_request().body("Bad request");
    }

    log::debug!("Connected ws");

    let task_group_value = request.path_params.value("task_group").unwrap();
    let task_group = match Uuid::from_str(task_group_value) {
        Ok(uuid) => uuid,
        Err(error) => {
            log::error!("Not a valid task group. {}", error);
            let error_response = build_standard_response(
                "failed",
                "invalid_task_group",
                Some("Not a valid task group format"),
                None,
                None,
            );
            let _ = websocket.send_json(&error_response).await;
            return websocket.response();
        }
    };

    let shared_context = request.context::<SharedContext>().unwrap();
    listen_ws_message(shared_context, &task_group, &mut websocket).await;

    let ws_connections = shared_context.websocket_connections.clone();
    ws_connections
        .unsubscribe(&task_group.to_string(), &websocket.uid)
        .await;
    websocket.response()
}

async fn login(request: Request) -> Response {
    let context = request.context::<SharedContext>().unwrap();
    todo!()
}

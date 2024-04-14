mod shortcut;

use std::path::PathBuf;
use std::str::FromStr;

use actix_multipart::form::MultipartForm;
use actix_web::{get, HttpRequest, HttpResponse, post, Responder, web};
use actix_web::web::{Payload, Query};

use uuid::Uuid;

use crate::{AppData};
use crate::db::models::{BackgroundRemoverTask, NewBackgroundRemoverTask};
use crate::forms::{AuthImageUploadQueryParams, ImageUploadForm};
use crate::routes::shortcut::ShortcutResponse;

use crate::implementations;
use crate::implementations::services::build_standard_response;

use crate::utils::file_utils::{save_temp_file_to_media, media_root_relative};
use crate::utils::image_utils;

///
/// Handles the common implementation for saving and insertion of uploaded image.
/// If save is successful, returns instance of BackgroundRemoverTask model else returns HttpResponse
/// error for public view. The error response is generic and does not contain any critical information.
///
/// Checkout log for the specific errors.
///
async fn handle_upload_common(data: &mut web::Data<AppData>, form: &MultipartForm<ImageUploadForm>,
                              user_identifier: Option<String>) -> Result<BackgroundRemoverTask, HttpResponse> {
    // A unique ID to identify this image publicly
    let task_id = Uuid::new_v4();

    // Saves uploaded image to the disk
    // The save dir looks like this /{project_name}/{media_root}/{task_id}/original/
    let sub_dir = Some(format!("background-remover/{}/original/", task_id.to_string()));
    log::debug!("Saving temp image to database.");

    // Path where the original image is saved
    let original_image_path = match save_temp_file_to_media(&form.original_image, sub_dir) {
        Ok(path) => path,
        Err(error) => {
            log::error!("Failed to save temp media file. Error: {}", error);
            return Err(HttpResponse::json_internal_server_error());
        }
    };

    // Saves preview original image. Creates background-remover directory after media root dir.
    let original_image_path_buf = PathBuf::from(&original_image_path);
    let sub_dir = PathBuf::from("background-remover");
    let preview_original_saved_path = match image_utils::save_media_preview_original_image(
        &task_id, Some(&sub_dir), &original_image_path_buf, (600, 600)) {
        Ok(path) => path,
        Err(error) => {
            log::error!("Failed to save preview original image {:?}. Error: {}",
                original_image_path_buf,  error);
            return Err(HttpResponse::json_internal_server_error());
        }
    };

    // Converts Task group String to UUID
    let task_group = match Uuid::from_str(form.task_group.as_str()) {
        Ok(uuid) => uuid,
        Err(error) => {
            log::error!("Failed to convert task group to UUID format. Error {:?}", error);
            let error_response = build_standard_response(
                "failed",
                "invalid_format",
                Some("Invalid task group format"),
                None,
                None,
            );
            return Err(HttpResponse::BadRequest().json(error_response));
        }
    };

    // Extract country value if present
    let country;
    if let Some(value) = &form.country {
        country = Some(value.to_string());
    } else {
        country = None;
    }

    // Above path can be absolute if set in MEDIA_ROOT
    // Converts absolute path to relative path to store in database.
    let original_image_path_relative = match media_root_relative(&original_image_path) {
        Ok(value) => value.to_string_lossy().to_string(),
        Err(error) => {
            log::error!("Failed to generate relative media url for original image. Error: {}", error);
            return Err(HttpResponse::json_internal_server_error());
        }
    };

    // Extracts relative media root folder
    let preview_original_image_path_relative = match media_root_relative(&preview_original_saved_path) {
        Ok(value) => value.to_string_lossy().to_string(),
        Err(error) => {
            log::error!("Failed to generate relative media url for preview original image. Error: {}", error);
            return Err(HttpResponse::json_internal_server_error());
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
    match BackgroundRemoverTask::insert_new_task(data.db_wrapper.clone(),
                                                 &new_background_remover_task).await {
        Ok(_) => {
            log::debug!("New task saved in database. Task id: {:?}", new_background_remover_task.key);
        }
        Err(error) => {
            log::error!("Failed to insert new task. Error: {}", error);
            return Err(HttpResponse::json_internal_server_error());
        }
    }

    // Returns full instance of BackgroundRemoverTask model if successful
    return match BackgroundRemoverTask::fetch(
        data.db_wrapper.clone(), &task_id).await {
        Ok(task) => {
            Ok(task)
        }
        Err(error) => {
            log::error!("Failed to fetch background remover task. Error: {}", error);
            Err(HttpResponse::json_internal_server_error())
        }
    };
}

///
/// Image uploading endpoint for all the users without API Key. Uploading image form itself won't start
/// processing image. Request background processing request from websocket endpoint
/// `/v1/remove-background/upload/`
///
#[post("/v1/bp/u/")]
async fn non_auth_upload(mut data: web::Data<AppData>, mut form: MultipartForm<ImageUploadForm>)
                         -> impl Responder {
    // Saves uploaded image and inserts new record in the database
    let background_remover_task = match handle_upload_common(&mut data, &mut form, None).await {
        Ok(new_task) => new_task,
        Err(error) => {
            log::error!("Failed to update task in database.");
            return error;
        }
    };

    // Converts instance to JSON. The serialized response is modified and does not represent exact
    // fields as in the instance.
    let serialized = match background_remover_task.serialize() {
        Ok(serialized) => serialized,
        Err(error) => {
            log::error!("Failed to convert instance to JSON. Error: {}", error);
            return HttpResponse::json_internal_server_error();
        }
    };

    let response = build_standard_response(
        "success",
        "image_upload",
        None,
        Some(serialized),
        None,
    );

    HttpResponse::Ok().json(response)
}

///
/// Endpoint for uploading file with API key. Currently not implemented API key checks.
///
#[post("/v1/remove-background/upload/")]
async fn auth_upload(mut data: web::Data<AppData>, request: HttpRequest,
                     form: MultipartForm<ImageUploadForm>) -> impl Responder {
    // Extracts API key from the request query params
    let _api_key: String = match Query::<AuthImageUploadQueryParams>::from_query(&request.query_string()) {
        Ok(query_params) => query_params.api_key.clone(),
        Err(error) => {
            log::error!("Error occurred: {}", error);
            return HttpResponse::Unauthorized().body("Api key is missing.");
        }
    };

    // Extracts BackgroundRemoverTask model instance from the upload result
    let background_remover_task = match handle_upload_common(&mut data,
                                                             &form, None).await {
        Ok(task) => task,
        Err(error) => {
            return error;
        }
    };

    // Converts instance to JSON. The serialized response is modified and does not represent exact
    // fields as in the instance.
    let serialized = match background_remover_task.serialize() {
        Ok(serialized) => serialized,
        Err(error) => {
            log::error!("Failed to convert instance to JSON. Error: {}", error);
            return HttpResponse::json_internal_server_error();
        }
    };

    // Sends this task instance for processing to the bp server
    let _ = data.tx_image_mpsc_channel.send(background_remover_task).await;
    HttpResponse::Ok().json(serialized)
}

#[get("/v1/remove-background/details/{task_id}/")]
async fn task_details(task_id_path: web::Path<String>, data: web::Data<AppData>) -> impl Responder {
    // Converts task id path to UUID
    let task_id = match Uuid::from_str(&task_id_path) {
        Ok(uuid) => uuid,
        Err(error) => {
            log::error!("Invalid task id format. Error: {}", error);
            let error_response = build_standard_response(
                "failed",
                "invalid_format",
                Some("Invalid task id format"),
                None,
                None,
            );
            return HttpResponse::BadRequest().json(error_response);
        }
    };

    // Fetches background remover task instance with task_id. Here, the task_id represents key column
    // in the database.
    let instance = match BackgroundRemoverTask::fetch(data.db_wrapper.clone(), &task_id).await {
        Ok(task) => task,
        Err(error) => {
            log::error!("Failed to fetch task id. Error: {}", error);
            return HttpResponse::NotFound().body("This task id does not exist.");
        }
    };

    HttpResponse::Ok().json(instance)
}


///
/// WebSocket endpoint for staring processing operation and receiving results.
///
///
/// Send this JSON from WebSocket client.
/// ```markdown
/// {
///     "key": "<uuid>"
/// }
/// ```
///
#[get("/ws/remove-background/{task_group}/")]
async fn ws_result_websocket(task_group_path: web::Path<String>, data: web::Data<AppData>,
                             request: HttpRequest, stream: Payload) -> impl Responder {
    // Extracts task_group from query params
    let task_group = match Uuid::from_str(&task_group_path) {
        Ok(uuid) => uuid,
        Err(error) => {
            // Task group is not a valid UUID
            log::error!("Failed to convert String to UUID conversion. Error: {}", error);
            let error_response = build_standard_response(
                "failed",
                "invalid_format",
                Some("Invalid task group format"),
                None,
                None,
            );
            return HttpResponse::BadRequest().json(error_response);
        }
    };

    let (response, session, message_stream) =
        match actix_ws::handle(&request, stream) {
            Ok(values) => values,
            Err(error) => {
                log::error!("Failed to handle websocket: {}", error);
                return HttpResponse::json_internal_server_error();
            }
        };

    let websocket_connections = &data.websocket_connections;
    let ws_sessions = websocket_connections.sessions.clone();
    log::info!("Client connected to websocket.");

    // Spawns new task after upgrading to WebSocket connection
    actix_web::rt::spawn(async move {
        // Calls implementation logic for websocket
        implementations::services::task_ws::listen_ws_message(
            data,
            task_group,
            session,
            message_stream,
            ws_sessions,
        ).await;
    });
    response
}
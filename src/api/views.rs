use racoon::core::request::Request;
use racoon::core::response::status::ResponseStatus;
use racoon::core::response::{HttpResponse, JsonResponse, Response};
use racoon::forms::FormValidator;

use serde_json::json;
use uuid::Uuid;

use crate::api::forms::PublicImageUploadForm;
use crate::db::models::{BackgroundRemoverTask, NewBackgroundRemoverTask};
use crate::utils::path_utils::{self, ForImage};
use crate::SharedContext;

pub async fn public_upload(request: Request) -> Response {
    if request.method != "POST" {
        return HttpResponse::ok().body("This request method is not supported.");
    }

    let form = PublicImageUploadForm::new();

    // If form contains error, returns error response.
    let validated_form = match form.validate(&request).await {
        Ok(form) => form,
        Err(error) => {
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

    let original_image_save_path = match path_utils::generate_save_path(ForImage::OriginalImage(
        &task_id,
        &original_image.filename,
    )) {
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
    let _ = tokio::fs::rename(original_image.temp_path, &original_image_save_path).await;

    // Saves to database
    let task_group = validated_form.task_group.value().await;
    let country = validated_form.country.value().await;
    let user_identifier = validated_form.user_identifier.value().await;

    let new_task = NewBackgroundRemoverTask {
        country,
        key: task_id,
        original_image_path: original_image_save_path.to_string_lossy().to_string(),
        preview_original_image_path: original_image_save_path.to_string_lossy().to_string(),
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
        "filename": original_image.filename
    }))
}

pub async fn listen_processing_ws(request: Request) -> Response {
    todo!()
}

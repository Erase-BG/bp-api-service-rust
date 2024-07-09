use racoon::core::request::Request;
use racoon::core::response::status::ResponseStatus;
use racoon::core::response::{HttpResponse, JsonResponse, Response};
use racoon::forms::FormValidator;

use serde_json::json;

use crate::api::forms::PublicImageUploadForm;
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
            return JsonResponse::bad_request().body(json!({
                "field_errors": error.field_errors,
                "other_errors": error.others,
            }));
        }
    };

    // Handles validated form data
    let original_image = validated_form.original_image.value().await;
    let shared_context: &SharedContext = request.context().expect("SharedContext is missing.");

    // Sends this image for processing.
    task::send(shared_context.bp_request_client.clone()).await;

    return JsonResponse::ok().body(json!({
        "status": "success",
        "filename": original_image.filename
    }));
}

pub async fn listen_processing_ws(request: Request) -> Response {
    todo!()
}

use racoon::core::request::Request;
use racoon::core::response::status::ResponseStatus;
use racoon::core::response::{HttpResponse, JsonResponse, Response};
use racoon::forms::FormValidator;

use serde_json::json;

use crate::api::forms::PublicImageUploadForm;

pub async fn public_upload(request: Request) -> Response {
    if request.method != "POST" {
        return HttpResponse::ok().body("This request method is not supported.")
    }

    let form = PublicImageUploadForm::new();

    match form.validate(&request).await {
        Ok(form) => {
            let original_image = form.original_image.value().await;

            return JsonResponse::ok().body(json!({
                "status": "success",
                "filename": original_image.filename
            }));
        }
        Err(error) => {
            return JsonResponse::bad_request().body(json!({
                "field_errors": error.field_errors,
                "other_errors": error.others,
            }));
        }
    }
}

pub async fn listen_processing_ws(request: Request) -> Response {
    todo!()
}

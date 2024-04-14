use actix_multipart::form::MultipartForm;
use actix_multipart::form::tempfile::TempFile;
use actix_multipart::form::text::Text;

use serde::Deserialize;

///
/// Query params where it expects to have `api_key` field.
///
#[derive(Debug, Deserialize)]
pub struct AuthImageUploadQueryParams {
    pub api_key: String,
}

///
/// Multipart form for uploading image with some information.
///
#[derive(MultipartForm)]
pub struct ImageUploadForm {
    pub task_group: Text<String>,
    pub original_image: TempFile,
    pub country: Option<Text<String>>,
}

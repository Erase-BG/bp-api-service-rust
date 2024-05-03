use racoon::core::forms::{FileField, FileFieldShortcut, Files, FormData};
use racoon::core::shortcuts::SingleText;

use serde::Deserialize;
use serde_json::Value;

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
pub struct ImageUploadForm<'a> {
    pub task_group: &'a String,
    pub original_image: &'a FileField,
    pub country: Option<&'a String>,
}

impl<'a> ImageUploadForm<'a> {
    pub fn validate(form_data: &'a FormData, files: &'a Files) -> Result<Self, Value> {
        let task_group;

        if let Some(value) = form_data.value("task_group") {
            task_group = value;
        } else {
            let error = serde_json::json!({
                "task_group": "Field is missing."
            });
            return Err(error);
        }

        let original_image;
        if let Some(field) = files.value("original_image") {
            original_image = field;
        } else {
            let error = serde_json::json!({
                    "original_image": "Field is missing."
            });
            return Err(error);
        }

        let country;
        if let Some(value) = form_data.value("country") {
            country = Some(value);
        } else {
            country = None;
        }

        Ok(Self {
            task_group,
            original_image,
            country,
        })
    }
}

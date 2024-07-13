use racoon::core::path::Path;
use racoon::view;

use crate::api::views::{listen_processing_ws, public_upload, task_details_view};

pub fn register_urls() -> Vec<Path> {
    vec![
        Path::new("/v1/bp/u/", view!(public_upload)),
        Path::new(
            "/v1/remove-background/details/{task_id}/",
            view!(task_details_view),
        ),
        Path::new(
            "/ws/remove-background/{task_group}/",
            view!(listen_processing_ws),
        ),
    ]
}

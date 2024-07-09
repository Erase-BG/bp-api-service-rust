use racoon::{core::path::Path, view};

use crate::api::views::{
    listen_processing_ws,
    public_upload
};

pub fn register_urls() -> Vec<Path> {
    vec![
        Path::new("/bp/u/", view!(public_upload)),
        Path::new("/ws/", view!(listen_processing_ws)),
    ]
}

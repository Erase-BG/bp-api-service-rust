use std::sync::Arc;

use serde_json::json;
use tej_protoc::protoc::File;
use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::clients::bp_request_client::BPRequestClient;
use crate::db::models::BackgroundRemoverTask;

///
///
///
pub async fn send(
    bp_request_client: Arc<BPRequestClient>,
    task: &BackgroundRemoverTask,
) -> std::io::Result<()> {
    let message = json!({
        "task_id": task.key.to_string(),
    });

    let mut original_image_file = fs::File::open(&task.original_image_path).await?;
    let mut buffer = vec![];
    original_image_file.read_to_end(&mut buffer).await?;
    let file = File::new(b"original.jpg".to_vec(), buffer);
    let files = [file];

    // Sends files to BP Server.
    bp_request_client.send(&files, &message).await?;
    Ok(())
}

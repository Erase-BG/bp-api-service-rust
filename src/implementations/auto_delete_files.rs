use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use chrono::{DateTime, Utc};
use tokio::time::sleep;

use crate::db::DBWrapper;
use crate::db::models::BackgroundRemoverTask;
use crate::utils::file_utils::media_file_path;
use crate::utils::urls::path_to_full_media_url;

pub async fn run_auto_delete(dbwrapper: DBWrapper) {
    log::debug!("Watching files for auto deletion");

    loop {
        let current_date = Utc::now();

        // Past 6 days
        let from_range = current_date - Duration::from_secs(20 * 86400);
        // Past 2 days
        let to_range = current_date - Duration::from_secs(2 * 86400);

        let models = match BackgroundRemoverTask::fetch_by_date_from(
            dbwrapper.clone(), &from_range, &to_range).await {
            Ok(models) => models,
            Err(error) => {
                log::error!("Failed to fetch models. Error: {:?}", error);
                break;
            }
        };

        println!("len {}", models.len());

        for model in models {
            let relative_original_image_path = PathBuf::from(model.original_image_path);
            let mut task_image_dir = match media_file_path(&relative_original_image_path) {
                Ok(path) => PathBuf::from(path),
                _ => {
                    continue;
                }
            };

            task_image_dir.pop();
            task_image_dir.pop();

            if task_image_dir.exists() {
                log::debug!("Removing: {:?}", task_image_dir);

                match fs::remove_dir_all(&task_image_dir) {
                    Ok(()) => {}
                    Err(error) => {
                        log::error!("Error: {}", error);
                    }
                };
            }
        }
        sleep(Duration::from_secs(30 * 60)).await;
    }
}

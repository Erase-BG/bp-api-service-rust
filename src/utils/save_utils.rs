use std::ffi::OsStr;
use std::path::PathBuf;

use tej_protoc::protoc::File;
use tokio::io::AsyncWriteExt;

use crate::db::models::BackgroundRemoverTask;

use super::path_utils::{self, ForImage};

///
/// Returns (transparent_image_path, mask_image_path, preview_transparent_image_path)
///
pub async fn save_files_received_from_bp_server(
    instance: &BackgroundRemoverTask,
    files: &Vec<File>,
    is_fake_processed: bool,
) -> std::io::Result<(PathBuf, PathBuf, PathBuf)> {
    println!("Is fake processed: {}", is_fake_processed);

    if is_fake_processed {
        if files.len() < 2 {
            return Err(std::io::Error::other(format!(
                "Minimum 2 files required for fake processed. But received {}.",
                files.len()
            )));
        }
    } else {
        if files.len() < 3 {
            return Err(std::io::Error::other(format!(
                "Minimum 3 files required. But received {}.",
                files.len()
            )));
        }
    }

    let original_image_path = PathBuf::from(&instance.original_image_path);
    let filename_without_extension;
    if let Some(filename_str) = original_image_path.file_stem() {
        filename_without_extension = filename_str;
    } else {
        filename_without_extension = &OsStr::new("image.jpg");
    };

    let transparent_image = &files[0];
    let mask_image = &files[1];
    let preview_transparent_image = &files[0];

    let png_filename = format!("{}.png", filename_without_extension.to_string_lossy());

    // ======== Transparent image save begins ==========
    let transparent_image_save_path = path_utils::generate_save_path(ForImage::TransparentImage(
        &instance.key,
        &png_filename.to_string(),
    ))?;

    if transparent_image_save_path.exists() {
        println!("Transparent image file already exists. Removing file.");
        let _ = tokio::fs::remove_file(&transparent_image_save_path).await;
    }

    println!(
        "Writing transparent image to {:?}.",
        transparent_image_save_path
    );

    let mut transparent_image_file =
        tokio::fs::File::create_new(&transparent_image_save_path).await?;
    transparent_image_file
        .write_all(&transparent_image.data)
        .await?;
    // Transparent image save ends.

    // ============= Mask image save begins ==============
    let mask_image_save_path = path_utils::generate_save_path(ForImage::MaskImage(
        &instance.key,
        &png_filename.to_string(),
    ))?;

    if mask_image_save_path.exists() {
        println!("Mask image file already exists. Removing file.");
        let _ = tokio::fs::remove_file(&mask_image_save_path).await;
    }

    println!("Writing mask image to {:?}.", mask_image_save_path);
    let mut mask_image_file = tokio::fs::File::create_new(&mask_image_save_path).await?;
    mask_image_file.write_all(&mask_image.data).await?;
    // Mask image save ends

    // ========== Preview transparent image save begins ===============

    // Preview transparent image save ends
    let preview_transparent_image_save_path = path_utils::generate_save_path(
        ForImage::PreviewTransparentImage(&instance.key, &png_filename.to_string()),
    )?;

    if preview_transparent_image_save_path.exists() {
        println!("Preview transparent image file already exists. Removing file.");
        let _ = tokio::fs::remove_file(&preview_transparent_image_save_path).await;
    }

    println!(
        "Writing preview transparent image to {:?}.",
        preview_transparent_image_save_path
    );

    let mut mask_image_file =
        tokio::fs::File::create_new(&preview_transparent_image_save_path).await?;
    mask_image_file
        .write_all(&preview_transparent_image.data)
        .await?;
    // Ends transaprent image save.

    Ok((
        transparent_image_save_path,
        mask_image_save_path,
        preview_transparent_image_save_path,
    ))
}

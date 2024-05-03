pub mod urls {
    use std::env;
    use std::path::PathBuf;

    ///
    /// Returns full media url including scheme and host.
    /// For example: `https://example.com/media/apple.png`
    /// Input example:
    /// ```rust
    /// Example:
    /// let full_url = full_media_url('/media/apple.png')
    /// ```
    ///
    ///
    fn full_media_url(media_url: &str) -> Result<String, String> {
        return match env::var("MEDIA_SERVE_HOST") {
            Ok(media_serve_host) => {
                Ok(format!("{}{}", media_serve_host, media_url))
            }
            Err(error) => {
                Err(error.to_string())
            }
        };
    }

    ///
    /// Converts relative path which does not include `MEDIA_URL` to media url.
    /// For example: `/images/apple.png` will be converted to `/media/images/apple.png`.
    ///
    pub fn path_to_relative_media_url(relative_path: &String) -> Result<String, String> {
        return match env::var("MEDIA_URL") {
            Ok(media_url) => {
                let mut media_path = PathBuf::from("/");
                media_path.push(media_url);
                media_path.push(relative_path);
                Ok(media_path.to_string_lossy().to_string())
            }
            Err(error) => {
                Err(error.to_string())
            }
        };
    }

    ///
    /// It takes relative path which does not include `MEDIA_URL` and returns the full url
    /// including schemes and host.
    ///
    /// For example: `/images/apple.png` will be converted to `https://example.com/media/images/apple.png`.
    ///
    pub fn path_to_full_media_url(relative_path: &String) -> Result<String, String> {
        let relative_media_url = path_to_relative_media_url(relative_path)?;
        Ok(full_media_url(relative_media_url.as_str())?)
    }

    ///
    /// It takes relative path which does not include `MEDIA_URL` and returns the full url
    /// including schemes and host.
    ///
    /// For example: `/images/apple.png` will be converted to `https://example.com/media/images/apple.png`.
    ///
    /// If path is `None`, it simply returns `None`.
    ///
    ///
    pub fn path_to_absolute_media_url_optional(path: &Option<String>) -> Result<Option<String>, String> {
        if let Some(path) = path {
            return match path_to_full_media_url(path) {
                Ok(url) => {
                    Ok(Some(url))
                }
                Err(error) => {
                    Err(error)
                }
            };
        }
        return Ok(None);
    }
}

pub mod file_utils {
    use std::env;
    use std::path::{Path, PathBuf};

    use racoon::core::forms::FileField;

    ///
    /// Takes input as a path and creates all required directories if not exist.
    ///
    pub fn create_dir_from_path(path: &PathBuf) -> std::io::Result<()> {
        if !path.exists() {
            // Clone and remove filename from destination path if any.
            let mut to_create_dirs = path.clone();

            if to_create_dirs.file_name().is_some() {
                // POP filename from the destination path
                to_create_dirs.pop();
            }

            std::fs::create_dir_all(to_create_dirs)?;
        }

        Ok(())
    }

    ///
    /// Returns relative path from the media root path.
    ///
    /// For example: `/media/images/apple.png` results `images/apple.png`
    ///
    pub fn media_root_relative(path: &PathBuf) -> Result<PathBuf, String> {
        // Extracts MEDIA_ROOT directory
        let media_root = match env::var("MEDIA_ROOT") {
            Ok(value) => PathBuf::from(value),
            Err(error) => {
                log::error!("Failed to read MEDIA_ROOT.");
                return Err(error.to_string());
            }
        };

        // Extracts relative path and returns
        return match path.strip_prefix(media_root) {
            Ok(value) => {
                Ok(value.to_owned())
            }
            Err(error) => {
                Err(error.to_string())
            }
        };
    }


    ///
    /// Returns media file path from the relative path based on `MEDIA_ROOT`.
    /// For example: `images/apple.png` results `/var/www/public/media/images/apple.png`
    ///
    pub fn media_file_path(relative_media_path: &PathBuf) -> Result<PathBuf, String> {
        return match env::var("MEDIA_ROOT") {
            Ok(media_root) => {
                let mut absolute = PathBuf::from(media_root);
                absolute.push(relative_media_path);
                Ok(absolute)
            }
            Err(error) => {
                log::error!("Failed to read MEDIA_ROOT.");
                Err(error.to_string())
            }
        };
    }


    ///
    /// Moves temporary file to Disk. If the destination directory does not exist, the directories
    /// are created.
    ///
    pub fn move_temp_file(from: &Path, destination: &PathBuf) -> std::io::Result<()> {
        create_dir_from_path(destination)?;
        // Move temp file to the destination
        std::fs::rename(&from, destination)
    }

    ///
    /// Saves temporary file to MEDIA_ROOT from environment variable with relative path if given.
    ///
    pub fn save_temp_file_to_media(file_field: &FileField, sub_dir: Option<String>) -> Result<PathBuf, String> {
        // Extracts media root directory to save files
        let media_root = match env::var("MEDIA_ROOT") {
            Ok(path) => path,
            Err(error) => {
                return Err(
                    format!(
                        "Failed to read MEDIA_ROOT from environment variable. Error: {}",
                        error.to_string()
                    )
                );
            }
        };

        let filename = &file_field.name;

        // Saves uploaded file to the media root directory
        let mut save_dir = PathBuf::new();
        save_dir.push(media_root);

        if let Some(relative_path) = sub_dir {
            save_dir.push(relative_path);
        }

        save_dir.push(filename.clone());

        // Moves temporary file to the given destination
        let move_from = file_field.temp_file.path();
        if move_temp_file(file_field.temp_file.path(), &save_dir).is_err() {
            let error = format!("Failed to move file from {:?}  to {:?}",
                                move_from, save_dir);
            return Err(error.to_owned());
        }

        return Ok(save_dir);
    }
}

#[cfg(test)]
pub mod tests {
    use std::env;
    use std::path::PathBuf;

    use crate::utils::file_utils::media_root_relative;

    #[test]
    pub fn relative_test() {
        // Setups env var
        env::set_var("MEDIA_ROOT", "/home/username/bp-api-service/media/background-remover/");

        // Test 1
        let full_path = PathBuf::from("/home/username/bp-api-service/media/background-remover/file");
        let path = media_root_relative(&full_path);
        println!("{:?}", path);
        assert_eq!(true, path.is_ok());
        assert_eq!("background-remover/file", path.unwrap().to_string_lossy().to_string().as_str());


        env::set_var("MEDIA_ROOT", "media/background-remover/");

        // Test 2
        let full_path = PathBuf::from("media/background-remover/file");
        let path = media_root_relative(&full_path);
        println!("{:?}", path);
        assert_eq!(true, path.is_ok());
        assert_eq!("background-remover/file", path.unwrap().to_string_lossy().to_string().as_str());
    }
}

pub mod image_utils {
    use std::env;
    use std::io::Write;
    use std::path::PathBuf;

    use image::{DynamicImage, ImageResult};
    use image::imageops::FilterType;

    use tej_protoc::protoc::File;
    use uuid::Uuid;

    use crate::utils::file_utils::create_dir_from_path;

    ///
    /// Converts raw bytes to DynamicImage
    ///
    pub fn bytes_to_image(bytes: &Vec<u8>) -> ImageResult<DynamicImage> {
        image::load_from_memory(bytes)
    }

    ///
    /// Saves raw bytes to file.
    ///
    pub fn save_bytes_to_file(bytes: &Vec<u8>, path: &PathBuf) -> std::io::Result<()> {
        // Creates dirs if not exist
        create_dir_from_path(path)?;

        let mut file = std::fs::File::create(path)?;
        file.write_all(bytes)?;
        Ok(())
    }

    ///
    /// Saves preview image by resizing original image to the specified size.
    ///
    pub fn save_media_preview_original_image(task_id: &Uuid,
                                             sub_dir: Option<&PathBuf>,
                                             original_image_path: &PathBuf,
                                             resize_to: (u32, u32)) -> std::io::Result<PathBuf> {
        let media_root = match env::var("MEDIA_ROOT") {
            Ok(path) => path,
            Err(error) => {
                return Err(
                    std::io::Error::other(
                        format!(
                            "Failed to extract MEDIA_ROOT dir. Error: {}",
                            error.to_string()
                        )
                    )
                );
            }
        };

        // Extract filename from the original image path
        let filename;
        if let Some(value) = original_image_path.file_name() {
            filename = value;
        } else {
            return Err(
                std::io::Error::other(
                    format!("Failed to extract filename from {:?}", original_image_path)
                )
            );
        }

        // Original image save dir
        let mut out_dir = PathBuf::from(media_root);
        if let Some(sub_dir) = sub_dir {
            out_dir.extend(sub_dir);
        }
        out_dir.push(task_id.to_string());
        out_dir.push("preview-original");
        out_dir.push(filename);

        // Loads original image from path
        let image = match image::open(original_image_path) {
            Ok(image) => image,
            Err(error) => {
                return Err(
                    std::io::Error::other(
                        format!(
                            "Failed to load image from path. Error: {}",
                            error
                        ),
                    )
                );
            }
        };

        // Resizes image to given size
        let resized_image = image.resize(resize_to.0, resize_to.1, FilterType::Triangle);

        // Creates directory if not exist
        create_dir_from_path(&out_dir)?;
        return match resized_image.save(&out_dir) {
            Ok(()) => {
                Ok(out_dir)
            }
            Err(error) => {
                log::error!("Failed to save preview original image");
                Err(std::io::Error::other(error.to_string()))
            }
        };
    }

    ///
    /// Saves preview image by resizing processed image to the given size.
    ///
    pub fn save_preview_processed_image(original_size: &Vec<u8>, resize_to: (u32, u32),
                                        file_path: &PathBuf) -> std::io::Result<()> {
        let image = match bytes_to_image(original_size) {
            Ok(value) => value,
            Err(error) => {
                return Err(std::io::Error::other(error.to_string()));
            }
        };

        // Creates sub dir if not exist
        create_dir_from_path(file_path)?;

        let resized_image = image.resize(resize_to.0, resize_to.1, FilterType::Triangle);
        return match resized_image.save(file_path) {
            Ok(()) => {
                Ok(())
            }
            Err(error) => {
                Err(std::io::Error::other(error.to_string()))
            }
        };
    }

    ///
    /// Saves images to media path from files received from bp server.
    ///
    /// Order of files:
    /// - transparent image
    /// - mask image
    ///
    ///
    fn save_images(files: &Vec<File>, processed_image_path: &PathBuf, preview_processed_path: &PathBuf,
                   mask_image_path: &PathBuf) -> std::io::Result<()> {
        // Extracts processed file
        let processed_file;
        if let Some(file) = files.get(0) {
            processed_file = file;
        } else {
            return Err(std::io::Error::other("Failed to extract mask image path of index 0."));
        }

        // Extracts mask file
        let mask_file;
        if let Some(file) = files.get(1) {
            mask_file = file;
        } else {
            return Err(std::io::Error::other("Failed to extract mask image path of index 0."));
        }

        // Save processed image
        match save_bytes_to_file(&processed_file.data, processed_image_path) {
            Ok(()) => {}
            Err(error) => {
                return Err(std::io::Error::other(
                    format!(
                        "Failed to save processed image file {:?}. Error: {:?}",
                        processed_image_path,
                        error
                    ))
                );
            }
        };

        // Save preview processed image
        match save_preview_processed_image(&processed_file.data, (600, 600), preview_processed_path) {
            Ok(()) => {}
            Err(error) => {
                log::error!("Failed to save preview processed image. Error: {}", error);
                return Err(std::io::Error::other(error));
            }
        };

        // Saves mask image
        return match save_bytes_to_file(&mask_file.data, mask_image_path) {
            Ok(()) => {
                Ok(())
            }
            Err(error) => {
                Err(std::io::Error::other(
                    format!(
                        "Failed to save mask image file {:?}. Error: {:?}",
                        mask_image_path,
                        error
                    )
                ))
            }
        };
    }

    ///
    /// Saves received images to media path and returns full path
    /// `(mask_image, processed_image, preview_processed_image)`.
    ///
    /// All these images are saved as PNG extension and format.
    ///
    pub fn save_task_images_to_media(original_filename: &str, task_id: &Uuid, files: &Vec<File>, sub_dir: Option<&PathBuf>)
                                     -> Result<(String, String, String), String> {
        if files.len() < 2 {
            return Err("Minimum 2 files required: mask Image, processed image.".to_owned());
        }

        // Extracts media root directory to save files
        let media_root = match env::var("MEDIA_ROOT") {
            Ok(path) => path,
            Err(error) => {
                return Err(
                    format!(
                        "Failed to read MEDIA_ROOT from environment variable. Error: {}",
                        error.to_string()
                    )
                );
            }
        };

        let mut filename;

        // Adds png extension to file
        if let Some(dot_index) = original_filename.rfind(".") {
            filename = original_filename[..dot_index].to_string();
            filename.push_str(".png");
        } else {
            filename = original_filename.to_string();
            filename.push_str(&".png".to_owned());
        }

        // Subdirectory for storing images for the tasks
        let mut out_dir = PathBuf::from(media_root);
        if let Some(sub_dir) = sub_dir {
            out_dir.extend(sub_dir)
        };
        out_dir.push(task_id.to_string());

        // Constructing processed image path
        let mut processed_image_path = out_dir.clone();
        processed_image_path.push("processed");
        processed_image_path.push(&filename);

        // Constructing preview processed image path
        let mut preview_processed_path = out_dir.clone();
        preview_processed_path.push("preview-processed");
        preview_processed_path.push(&filename);


        // Constructing mask image path
        let mut mask_image_path = out_dir.clone();
        mask_image_path.push("mask");
        mask_image_path.push(&filename);

        match save_images(files, &processed_image_path, &preview_processed_path, &mask_image_path) {
            Ok(()) => {}
            Err(error) => {
                log::error!("Failed to save images received from bp server.");
                log::error!("Processed path: {:?}", processed_image_path);
                log::error!("Preview processed path: {:?}", preview_processed_path);
                log::error!("Mask image path: {:?}", mask_image_path);
                return Err(error.to_string());
            }
        }

        Ok((
            processed_image_path.to_string_lossy().to_string(),
            preview_processed_path.to_string_lossy().to_string(),
            mask_image_path.to_string_lossy().to_string()
        ))
    }
}

use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

use uuid::Uuid;

///
/// Returns file path with the help of `base_relative_url`.
///
/// `base_relative_url` means the base url from where the files are served. For example:
/// `media/example.txt`.
///
/// Used for reading/writing files by knowing `base_relative_url` from database and base root path.
///
/// # Example
/// ```
/// let path = PathBuf::from("/var/www/public/example.com/media/");
/// let base_relative_url = PathBuf::from("media/example.txt");
///
/// let expected = PathBuf::from("/var/www/public/example.com/media/example.txt");
/// let result = super::relative_url_from_file_path(path, base_relative_url);
/// assert_eq!(expected, result);
/// ```
///
///
pub fn file_path_from_relative_url(mut path: PathBuf, base_relative_url: PathBuf) -> PathBuf {
    let cloned_path = path.clone();
    let path_parts: Vec<&OsStr> = cloned_path.iter().collect();
    let base_relative_url_parts: Vec<&OsStr> = base_relative_url.iter().collect();

    let last_path_part = path_parts.last();
    let first_relative_path_part = base_relative_url_parts.first();

    // Compares last path part from path with first path part from base relative url.
    if last_path_part.is_some() && first_relative_path_part.is_some() {
        if last_path_part
            .clone()
            .unwrap()
            .eq(first_relative_path_part.clone().unwrap())
        {
            path.pop();
        }
    }

    for base_part in base_relative_url_parts {
        path.push(base_part);
    }

    path
}

///
/// Returns `relative_path` to full media url including host.
///
/// Here `relative_path` means the relative url with the base directory. Example:
/// `media/image.jpg`.
///
/// Used for converting relative path/url information saved in database to full media url with
/// host. Example:`https://example.com/media/image.jpg`.
///
pub fn full_media_url_from_relative_path<S>(scheme: S, host: S, relative_url: PathBuf) -> String
where
    S: AsRef<str>,
{
    let scheme = scheme.as_ref();
    let host = host.as_ref();
    format!("{}://{}/{}", scheme, host, relative_url.to_string_lossy())
}

pub enum ForImage<'a> {
    OriginalImage(&'a Uuid, &'a String),
}

///
/// Returns path.
/// Depends on environment variables.
///
pub fn generate_save_path(from_image: ForImage) -> std::io::Result<PathBuf> {
    let media_root = match env::var("MEDIA_ROOT") {
        Ok(dir) => dir,
        Err(error) => {
            return Err(std::io::Error::other(error));
        }
    };

    let mut relative_url = PathBuf::new();
    relative_url.push(&media_root);
    relative_url.push("background-remover");

    match from_image {
        ForImage::OriginalImage(uuid, filename) => {
            relative_url.push(uuid.to_string());
            relative_url.push("original");

            // Creates directories if not exists.
            if !relative_url.exists() {
                std::fs::create_dir_all(&relative_url)?;
            }

            relative_url.push(filename);

            Ok(file_path_from_relative_url(
                PathBuf::from(media_root),
                relative_url,
            ))
        }
    }
}

#[cfg(test)]
pub mod test {
    use std::path::PathBuf;

    #[test]
    pub fn test_file_path_from_relative_url() {
        let path = PathBuf::from("/var/www/public/example.com/media/");
        let base_relative_url = PathBuf::from("media/example.txt");

        let expected = PathBuf::from("/var/www/public/example.com/media/example.txt");
        let result = super::file_path_from_relative_url(path, base_relative_url);
        assert_eq!(expected, result);

        let path = PathBuf::from("/media/");
        let base_relative_url = PathBuf::from("media/example.txt");

        let expected = PathBuf::from("/media/example.txt");
        let result = super::file_path_from_relative_url(path, base_relative_url);
        assert_eq!(expected, result);
    }

    #[test]
    pub fn test_full_media_url_from_relative_path() {
        let scheme = "https";
        let host = "example.com";
        let relative_url = PathBuf::from("media/img.jpg");

        let expected = "https://example.com/media/img.jpg".to_string();
        let result = super::full_media_url_from_relative_path(scheme, host, relative_url);
        assert_eq!(expected, result);
    }
}

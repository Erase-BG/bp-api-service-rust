use std::{ffi::OsStr, path::PathBuf};

use crate::utils::path_utils;

///
/// Returns relative url including `base_relative_url`.
///
/// `base_relative_url` means the base url from where the files are served. For example:
/// `media/example.txt`.
///
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
pub fn relative_url_from_file_path(mut path: PathBuf, base_relative_url: PathBuf) -> PathBuf {
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

#[cfg(test)]
pub mod test {
    use std::path::PathBuf;

    #[test]
    pub fn test_url() {
        let path = PathBuf::from("/var/www/public/example.com/media/");
        let base_relative_url = PathBuf::from("media/example.txt");

        let expected = PathBuf::from("/var/www/public/example.com/media/example.txt");
        let result = super::relative_url_from_file_path(path, base_relative_url);
        assert_eq!(expected, result);
    }
}

use std::os::unix::fs::MetadataExt;

use racoon::forms::fields::file_field::{FileField, UploadedFile};
use racoon::forms::fields::input_field::InputField;
use racoon::forms::fields::uuid_field::UuidField;
use racoon::forms::fields::AbstractFields;
use racoon::forms::FormValidator;

use uuid::Uuid;

pub struct PublicImageUploadForm {
    pub task_group: UuidField<Uuid>,
    pub original_image: FileField<UploadedFile>,
    pub country: InputField<Option<String>>,
    pub user_identifier: InputField<Option<String>>,
}

impl FormValidator for PublicImageUploadForm {
    fn new() -> Self {
        Self {
            task_group: UuidField::new("task_group"),
            original_image: FileField::new("original_image").post_validate(
                |uploaded_file: UploadedFile| {
                    let temp_path = &uploaded_file.temp_path;

                    let file = match std::fs::File::open(temp_path) {
                        Ok(file) => file,
                        Err(error) => {
                            eprintln!("Failed to open file. Error: {}", error);
                            return Err(vec!["Unable to read file size.".to_string()]);
                        }
                    };

                    match file.metadata() {
                        Ok(metadata) => {
                            if metadata.size() > 60 * 1024 * 1024 {
                                return Err(vec!["File size is too large.".to_string()]);
                            }
                        }
                        Err(error) => {
                            eprintln!("Failed to read file metadata. Error: {}", error);
                            return Err(vec!["Unable to read file size.".to_string()]);
                        }
                    }
                    Ok(uploaded_file)
                },
            ),
            country: InputField::new("country"),
            user_identifier: InputField::new("user_identifier"),
        }
    }

    fn form_fields(&mut self) -> racoon::forms::FormFields {
        vec![
            self.task_group.wrap(),
            self.original_image.wrap(),
            self.country.wrap(),
            self.user_identifier.wrap(),
        ]
    }
}

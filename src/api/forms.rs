use racoon::forms::fields::file_field::{FileField, UploadedFile};
use racoon::forms::fields::uuid_field::UuidField;
use racoon::forms::fields::AbstractFields;
use racoon::forms::FormValidator;

use uuid::Uuid;

pub struct PublicImageUploadForm {
    pub task_group: UuidField<Uuid>,
    pub original_image: FileField<UploadedFile>,
}

impl FormValidator for PublicImageUploadForm {
    fn new() -> Self {
        Self {
            task_group: UuidField::new("task_group"),
            original_image: FileField::new("original_image"),
        }
    }

    fn form_fields(&mut self) -> racoon::forms::FormFields {
        vec![self.task_group.wrap(), self.original_image.wrap()]
    }
}

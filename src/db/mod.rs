use sqlx::{Executor, PgPool};

///
/// Connection pool for database connection to safely pass around threads.
///
pub struct DBWrapper {
    pub connection: PgPool,
}


///
/// Clone trait implementation for thread safe passing.
///
impl Clone for DBWrapper {
    fn clone(&self) -> Self {
        Self {
            connection: self.connection.clone(),
        }
    }
}


// Table creation query
const CREATE_TABLE_BACKGROUND_REMOVER_TASK_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS background_remover_task(
        task_id BIGSERIAL PRIMARY KEY,
        date_created TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
        key UUID UNIQUE NOT NULL,
        task_group UUID NOT NULL,
        original_image_path TEXT NOT NULL,
        preview_original_image_path TEXT NOT NULL,
        mask_image_path TEXT,
        processed_image_path TEXT,
        preview_processed_image_path TEXT,
        processing BOOLEAN DEFAULT FALSE,
        result_status VARCHAR(255),
        user_identifier TEXT,
        country VARCHAR(255),
        logs JSONB
    )
"#;


///
/// Configures initial database operations such as creating a table if not exist.
///
pub async fn setup(db_wrapper: DBWrapper) -> Result<(), sqlx::Error> {
    let connection = db_wrapper.connection;
    connection.execute(CREATE_TABLE_BACKGROUND_REMOVER_TASK_SQL).await?;
    Ok(())
}

pub mod models {
    use std::fmt::Debug;

    use serde::{Serialize, Serializer};
    use serde::ser::{SerializeStruct};
    use serde_json::Value;

    use sqlx::{Executor, Row};
    use sqlx::types::chrono::Utc;
    use sqlx::types::JsonValue;

    use chrono::{DateTime};
    use futures_util::TryStreamExt;
    use serde::de::Error;
    use sqlx::postgres::PgRow;
    use uuid::Uuid;

    use crate::db::{DBWrapper};
    use crate::utils::urls::{
        path_to_full_media_url,
        path_to_absolute_media_url_optional,
    };

    ///
    /// This struct is the mapped columns of table `background_remover_task`.
    ///
    #[derive(Debug)]
    #[derive(sqlx::FromRow)]
    pub struct BackgroundRemoverTask {
        pub task_id: i64,
        pub date_created: DateTime<Utc>,
        pub key: Uuid,
        pub task_group: Uuid,
        pub original_image_path: String,
        pub preview_original_image_path: Option<String>,
        pub mask_image_path: Option<String>,
        pub processed_image_path: Option<String>,
        pub preview_processed_image_path: Option<String>,
        pub processing: Option<bool>,
        pub user_identifier: Option<String>,
        pub logs: Option<Value>,
    }

    ///
    /// Serde JSON custom serialize implementation. It modifies field values for `path` fields.
    ///
    impl Serialize for BackgroundRemoverTask {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
            let mut state = serializer.serialize_struct(
                "BackgroundRemoverTask", 11)?;
            state.serialize_field("task_id", &self.task_id)?;
            state.serialize_field("date_created", &self.date_created.to_string())?;
            state.serialize_field("key", &self.key)?;
            state.serialize_field("task_group", &self.task_group)?;

            match path_to_full_media_url(&self.original_image_path) {
                Ok(value) => {
                    state.serialize_field("original_image", &value)?;
                }
                Err(error) => {
                    return Err(serde::ser::Error::custom(error));
                }
            }

            match path_to_absolute_media_url_optional(&self.preview_original_image_path) {
                Ok(value) => {
                    state.serialize_field("preview_original_image", &value)?;
                }
                Err(error) => {
                    return Err(serde::ser::Error::custom(error));
                }
            }

            match path_to_absolute_media_url_optional(&self.mask_image_path) {
                Ok(value) => {
                    state.serialize_field("mask_image", &value)?;
                }
                Err(error) => {
                    return Err(serde::ser::Error::custom(error));
                }
            }

            match path_to_absolute_media_url_optional(&self.processed_image_path) {
                Ok(value) => {
                    state.serialize_field("processed_image", &value)?;
                }
                Err(error) => {
                    return Err(serde::ser::Error::custom(error));
                }
            }

            match path_to_absolute_media_url_optional(&self.preview_processed_image_path) {
                Ok(value) => {
                    state.serialize_field("preview_processed_image", &value)?;
                }
                Err(error) => {
                    return Err(serde::ser::Error::custom(error));
                }
            }

            state.serialize_field("processing", &self.processing)?;
            state.serialize_field("user_identifier", &self.user_identifier)?;
            state.serialize_field("logs", &self.logs)?;
            state.end()
        }
    }

    ///
    /// Partially mapped column for table `background_remover_task`.
    /// Contains necessary fields required for new record insertion in the database.
    ///
    pub struct NewBackgroundRemoverTask {
        pub key: Uuid,
        pub task_group: Uuid,
        pub original_image_path: String,
        pub preview_original_image_path: String,
        pub country: Option<String>,
        pub user_identifier: Option<String>,
    }

    ///
    /// Partially mapped column for table `background_remover_task`.
    /// Contains necessary fields required for updating existing record in the database.
    ///
    pub struct UpdateBackgroundRemoverTask {
        pub key: Uuid,
        pub mask_image_path: String,
        pub processed_image_path: String,
        pub preview_processed_image_path: String,
        pub logs: Option<Value>,
    }

    ///
    /// Implementations for `BackgroundRemoverTask` model
    ///
    impl BackgroundRemoverTask {
        ///
        /// Also serialized auto increment column `task_id` and `logs` which may leak actual
        /// available items count if accessible to users.
        ///
        pub fn serialize_full(&self) -> Result<Value, serde_json::Error> {
            serde_json::to_value(&self)
        }

        ///
        /// This does not include `task_id` and `logs` field and values.
        ///
        pub fn serialize(&self) -> Result<Value, serde_json::Error> {
            let mut serialized_full = match self.serialize_full() {
                Ok(value) => value,
                Err(error) => {
                    return Err(error);
                }
            };

            const REMOVE_FIELDS: [&str; 2] = ["task_id", "logs"];
            let map_object = serialized_full.as_object_mut();

            if let Some(map) = map_object {
                REMOVE_FIELDS.iter().for_each(|field| {
                    map.remove(*field);
                });

                return Ok(Value::from(map.clone()));
            }

            return Err(serde_json::Error::custom("Failed to parse while removing some fields."));
        }

        ///
        /// Inserts new record to the database.
        ///
        pub async fn insert_new_task(db_wrapper: DBWrapper, new_task: &NewBackgroundRemoverTask)
                                     -> Result<(), sqlx::Error> {
            let connection = db_wrapper.connection;

            const INSERT_QUERY: &str = r#"
                INSERT INTO background_remover_task(
                    key,
                    task_group,
                    original_image_path,
                    preview_original_image_path,
                    country,
                    user_identifier
                ) VALUES ($1, $2, $3, $4, $5, $6)
            "#;

            connection.execute(
                sqlx::query(&INSERT_QUERY)
                    .bind(&new_task.key)
                    .bind(&new_task.task_group)
                    .bind(&new_task.original_image_path)
                    .bind(&new_task.preview_original_image_path)
                    .bind(&new_task.country.clone())
                    .bind(&new_task.user_identifier.clone())
            ).await?;

            Ok(())
        }

        ///
        /// Updates existing record in the database of matching `key`.
        ///
        pub async fn update_task(db_wrapper: DBWrapper, update_task: &UpdateBackgroundRemoverTask)
                                 -> Result<(), sqlx::Error> {
            let connection = db_wrapper.connection;

            const UPDATE_QUERY: &str = r#"
                UPDATE background_remover_task
                SET
                    mask_image_path=$1,
                    processed_image_path=$2,
                    preview_processed_image_path=$3,
                    logs=$4
                WHERE
                    key=$5
            "#;

            connection.execute(
                sqlx::query(UPDATE_QUERY)
                    .bind(&update_task.mask_image_path)
                    .bind(&update_task.processed_image_path)
                    .bind(&update_task.preview_processed_image_path)
                    .bind(&update_task.logs)
                    .bind(&update_task.key)
            ).await?;
            Ok(())
        }

        ///
        /// Updates processing state of the task.
        ///
        pub async fn update_processing_state(db_wrapper: DBWrapper, key: &Uuid, state: bool)
                                             -> Result<(), sqlx::Error> {
            let connection = db_wrapper.connection;

            const UPDATE_QUERY: &str = r#"
                UPDATE background_remover_task
                SET
                    processing=$1
                WHERE
                    key=$2
            "#;

            connection.execute(
                sqlx::query(UPDATE_QUERY)
                    .bind(state)
                    .bind(key)
            ).await?;
            Ok(())
        }

        ///
        /// Returns instance of `BackgroundRemoverTask` of matching `key`.
        ///
        pub async fn fetch(db_wrapper: DBWrapper, key: &Uuid)
                           -> Result<BackgroundRemoverTask, sqlx::Error> {
            let connection = db_wrapper.connection;
            const FETCH_QUERY: &str = r#"
                SELECT * FROM background_remover_task WHERE key=$1 LIMIT 1
            "#;

            let instance: BackgroundRemoverTask = sqlx::query_as(FETCH_QUERY)
                .bind(key)
                .fetch_one(&connection).await?;

            Ok(instance)
        }

        pub async fn fetch_by_page(db_wrapper: DBWrapper, page: u32) -> Result<BackgroundRemoverTask, sqlx::Error> {
            todo!()
        }

        pub async fn fetch_by_date_from(db_wrapper: DBWrapper,
                                        from_past: &DateTime<Utc>,
                                        to_present: &DateTime<Utc>,
        ) -> Result<Vec<BackgroundRemoverTask>, sqlx::Error> {
            let connection = db_wrapper.connection;

            let fetch_query = r#"
                SELECT * FROM background_remover_task
                    WHERE date_created BETWEEN $1 AND $2
            "#;

            let models = sqlx::query_as(&fetch_query)
                .bind(from_past)
                .bind(to_present)
                .fetch_all(&connection).await?;

            Ok(models)
        }
    }
}

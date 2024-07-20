use std::env;

use sqlx::{Executor, PgPool};

///
/// Connection pool for database connection to safely pass around threads.
///
pub struct DBWrapper {
    pub pool: PgPool,
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
pub async fn setup() -> Result<DBWrapper, std::io::Error> {
    // Extract postgres url
    let postgres_url = match env::var("POSTGRES_URL") {
        Ok(value) => value,
        Err(error) => {
            log::error!("Failed to read POSTGRES_URL from environment variable. Probably missing.");
            return Err(std::io::Error::other(error));
        }
    };

    return match PgPool::connect(&postgres_url).await {
        Ok(pool) => match pool.execute(CREATE_TABLE_BACKGROUND_REMOVER_TASK_SQL).await {
            Ok(_) => Ok(DBWrapper { pool }),
            Err(error) => {
                println!("Failed to create required tables.");
                return Err(std::io::Error::other(error));
            }
        },
        Err(error) => {
            return Err(std::io::Error::other(error));
        }
    };
}

pub mod models {
    use std::env;
    use std::fmt::Debug;
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde::ser::{Error, SerializeStruct};
    use serde::{Serialize, Serializer};
    use serde_json::Value;

    use sqlx::types::chrono::Utc;
    use sqlx::Executor;

    use chrono::DateTime;
    use uuid::Uuid;

    use crate::db::DBWrapper;
    use crate::utils::path_utils;

    ///
    /// This struct is the mapped columns of table `background_remover_task`.
    ///
    #[derive(Debug, sqlx::FromRow)]
    pub struct BackgroundRemoverTask {
        /// Auto incremented unique integer for each background removal task.
        pub task_id: i64,
        /// Date when this removal task is created.
        pub date_created: DateTime<Utc>,
        /// Unique string for each task.
        pub key: Uuid,
        /// Unique string for websocket group used for listening websocket messags.
        pub task_group: Uuid,
        /// Relative path: media/image.jpg
        pub original_image_path: String,
        /// Relative path: media/image.png
        pub preview_original_image_path: Option<String>,
        /// Relative path: media/image.png
        pub mask_image_path: Option<String>,
        /// Relative path: media/image.png
        pub processed_image_path: Option<String>,
        /// Relative path: media/image.png
        pub preview_processed_image_path: Option<String>,
        /// Background removal status.
        pub processing: Option<bool>,
        /// Country from where photo is uploaded.
        pub country: Option<String>,
        /// Encoded string to identiy user.
        pub user_identifier: Option<String>,
        /// Task logs.
        pub logs: Option<Value>,
    }

    ///
    /// Serde JSON custom serialize implementation. It modifies field values for `path` fields.
    ///
    impl Serialize for BackgroundRemoverTask {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut state = serializer.serialize_struct("BackgroundRemoverTask", 11)?;
            state.serialize_field("task_id", &self.task_id)?;
            state.serialize_field("date_created", &self.date_created.to_string())?;
            state.serialize_field("key", &self.key)?;
            state.serialize_field("task_group", &self.task_group)?;

            // Url configurations from environment variables.
            let scheme = "https";
            let host = match env::var("HOST") {
                Ok(value) => value,
                Err(error) => {
                    return Err(Error::custom(error));
                }
            };

            // Adds full original image url to JSON object.
            let full_original_image_url = path_utils::full_media_url_from_relative_path(
                scheme,
                &host,
                PathBuf::from(&self.original_image_path),
            );
            state.serialize_field("original_image", &full_original_image_url)?;

            // Adds full media image url to JSON object.
            let full_media_preview_image_url;
            if let Some(preview_original_path) = &self.preview_original_image_path {
                full_media_preview_image_url = Some(path_utils::full_media_url_from_relative_path(
                    scheme,
                    &host,
                    PathBuf::from(preview_original_path),
                ));
            } else {
                full_media_preview_image_url = None;
            }
            state.serialize_field("preview_original_image", &full_media_preview_image_url)?;

            // Adds full processed image url to JSON object.
            let full_processed_original_image_url;
            if let Some(processed_original_path) = &self.processed_image_path {
                full_processed_original_image_url =
                    Some(path_utils::full_media_url_from_relative_path(
                        scheme,
                        &host,
                        PathBuf::from(processed_original_path),
                    ));
            } else {
                full_processed_original_image_url = None;
            }

            state.serialize_field("processed_image", &full_processed_original_image_url)?;

            let full_preview_processed_image_url;
            if let Some(preview_processed_path) = &self.preview_processed_image_path {
                full_preview_processed_image_url =
                    Some(path_utils::full_media_url_from_relative_path(
                        scheme,
                        &host,
                        PathBuf::from(preview_processed_path),
                    ));
            } else {
                full_preview_processed_image_url = None;
            }

            state.serialize_field("preview_processed_image", &full_preview_processed_image_url)?;

            let full_mask_image_url;
            if let Some(preview_mask_path) = &self.mask_image_path {
                full_mask_image_url = Some(path_utils::full_media_url_from_relative_path(
                    scheme,
                    &host,
                    PathBuf::from(preview_mask_path),
                ));
            } else {
                full_mask_image_url = None;
            }

            state.serialize_field("mask_image", &full_mask_image_url)?;

            state.serialize_field("processing", &self.processing)?;
            state.serialize_field("user_identifier", &self.user_identifier)?;
            state.serialize_field("country", &self.country)?;
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

            const REMOVE_FIELDS: [&str; 3] = ["task_id", "country", "logs"];
            let map_object = serialized_full.as_object_mut();

            if let Some(map) = map_object {
                REMOVE_FIELDS.iter().for_each(|field| {
                    map.remove(*field);
                });

                return Ok(Value::from(map.clone()));
            }

            return Err(serde_json::Error::custom(
                "Failed to parse while removing some fields.",
            ));
        }

        ///
        /// Inserts new record to the database.
        ///
        pub async fn insert_new_task(
            db_wrapper: Arc<DBWrapper>,
            new_task: &NewBackgroundRemoverTask,
        ) -> Result<(), sqlx::Error> {
            let connection = db_wrapper.pool.clone();

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

            connection
                .execute(
                    sqlx::query(&INSERT_QUERY)
                        .bind(&new_task.key)
                        .bind(&new_task.task_group)
                        .bind(&new_task.original_image_path)
                        .bind(&new_task.preview_original_image_path)
                        .bind(&new_task.country.clone())
                        .bind(&new_task.user_identifier.clone()),
                )
                .await?;

            Ok(())
        }

        ///
        /// Updates existing record in the database of matching `key`.
        ///
        pub async fn update_task(
            db_wrapper: Arc<DBWrapper>,
            update_task: &UpdateBackgroundRemoverTask,
        ) -> Result<(), sqlx::Error> {
            let connection = db_wrapper.pool.clone();

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

            connection
                .execute(
                    sqlx::query(UPDATE_QUERY)
                        .bind(&update_task.mask_image_path)
                        .bind(&update_task.processed_image_path)
                        .bind(&update_task.preview_processed_image_path)
                        .bind(&update_task.logs)
                        .bind(&update_task.key),
                )
                .await?;
            Ok(())
        }

        ///
        /// Updates processing state of the task.
        ///
        pub async fn update_processing_state(
            db_wrapper: Arc<DBWrapper>,
            key: &Uuid,
            state: bool,
        ) -> Result<(), sqlx::Error> {
            let connection = &db_wrapper.pool;

            const UPDATE_QUERY: &str = r#"
                UPDATE background_remover_task
                SET
                    processing=$1
                WHERE
                    key=$2
            "#;

            connection
                .execute(sqlx::query(UPDATE_QUERY).bind(state).bind(key))
                .await?;
            Ok(())
        }

        ///
        /// Returns instance of `BackgroundRemoverTask` of matching `key`.
        ///
        pub async fn fetch(
            db_wrapper: Arc<DBWrapper>,
            key: &Uuid,
        ) -> Result<BackgroundRemoverTask, sqlx::Error> {
            let connection = db_wrapper.pool.clone();

            const FETCH_QUERY: &str = r#"
                SELECT * FROM background_remover_task WHERE key=$1 LIMIT 1
            "#;

            let instance: BackgroundRemoverTask = sqlx::query_as(FETCH_QUERY)
                .bind(key)
                .fetch_one(&connection)
                .await?;

            Ok(instance)
        }

        pub async fn fetch_by_page(
            db_wrapper: Arc<DBWrapper>,
            page: u32,
        ) -> Result<Vec<BackgroundRemoverTask>, sqlx::Error> {
            let connection = db_wrapper.pool.clone();
            let tasks_per_page = 25;
            let offset = (page - 1) * tasks_per_page;

            const FETCH_QUERY: &str = r#"
                SELECT * FROM background_remover_task
                    ORDER BY task_id DESC
                    OFFSET $1
                    LIMIT $2
            "#;

            let models: Vec<BackgroundRemoverTask> = sqlx::query_as(FETCH_QUERY)
                .bind(offset as i64)
                .bind(tasks_per_page as i64)
                .fetch_all(&connection)
                .await?;

            Ok(models)
        }

        pub async fn length(db_wrapper: Arc<DBWrapper>) -> Result<u64, sqlx::Error> {
            let connection = db_wrapper.pool.clone();
            const COUNT_QUERY: &str = r#"
                SELECT COUNT(task_id) AS total FROM background_remover_task
            "#;

            let size: (i64,) = sqlx::query_as(COUNT_QUERY).fetch_one(&connection).await?;
            Ok(size.0 as u64)
        }

        pub async fn fetch_by_date_from(
            db_wrapper: DBWrapper,
            from_past: &DateTime<Utc>,
            to_present: &DateTime<Utc>,
        ) -> Result<Vec<BackgroundRemoverTask>, sqlx::Error> {
            let connection = db_wrapper.pool.clone();

            let fetch_query = r#"
                SELECT * FROM background_remover_task
                    WHERE date_created BETWEEN $1 AND $2
            "#;

            let models = sqlx::query_as(&fetch_query)
                .bind(from_past)
                .bind(to_present)
                .fetch_all(&connection)
                .await?;

            Ok(models)
        }
    }
}

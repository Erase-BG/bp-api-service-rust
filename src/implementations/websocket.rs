pub mod services {
    use std::collections::HashMap;
    use std::sync::{Arc};
    use racoon::core::websocket::Websocket;
    use serde_json::{Value};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    ///
    /// Well-formed JSON response style for better clarity for success and error response.
    ///
    /// The `message`, `data`, `errors` fields are optional and if values are not set, it does not
    /// presented in the response.
    ///
    pub fn build_standard_response(status: &str, status_code: &str, message: Option<&str>,
                                   data: Option<Value>, errors: Option<Value>) -> Value {
        let mut map = serde_json::map::Map::new();
        map.insert("status".to_owned(), status.into());
        map.insert("status_code".to_owned(), status_code.into());

        if let Some(message) = message {
            map.insert("message".to_owned(), message.into());
        }

        if let Some(data) = data {
            map.insert("data".to_owned(), data);
        }

        if let Some(errors) = errors {
            map.insert("errors".to_owned(), errors);
        }

        serde_json::Value::from(map)
    }

    ///
    /// Common function for sending message by passing `task_id` and `ws_sessions`.
    ///
    pub async fn send_message(task_group: &Uuid, ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>,
                              response: Value) {
        let mut sessions = ws_sessions.lock().await;
        // Session is there stored, but may not be live
        // If session is no more active, removes from the session

        if let Some(session) = sessions.get_mut(&task_group.to_string()) {
            // Send error message to websocket client
            if session.send_json(&response).await.is_err() {
                // Failed to send message. Client may be disconnected
                sessions.remove(&task_group.to_string());
            };
        }
    }

    pub mod task {
        use std::collections::HashMap;
        use std::net::TcpStream;
        use std::path::PathBuf;
        use std::sync::Arc;

        use racoon::core::websocket::Websocket;
        use tokio::sync::Mutex;

        use crate::clients::bp_request_client::BPRequestClient;
        use crate::db::models::BackgroundRemoverTask;
        use crate::implementations::websocket::services::build_standard_response;
        use crate::utils::file_utils::media_file_path;


        ///
        /// Sends new task for processing to the bp server.
        ///
        /// If for some reason sending task to the bp server is failed and error response to
        /// websocket client also fails, that websocket instance is removed from `ws_sessions`.
        ///
        pub async fn send_new_task_to_bp_server(tcp_stream: Arc<Mutex<Option<TcpStream>>>,
                                                instance: BackgroundRemoverTask,
                                                sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let original_image_abs_path = match media_file_path(
                &PathBuf::from(&instance.original_image_path)) {
                Ok(value) => value.to_string_lossy().to_string(),
                Err(error) => {
                    log::error!("Unable to get absolute path from file {}. Error: {}",
                        &instance.original_image_path, error);
                    send_internal_server_error(&instance, sessions.clone()).await;
                    return;
                }
            };

            match BPRequestClient::send_remove_task(tcp_stream, instance.key,
                                                    &original_image_abs_path).await {
                Ok(_) => {
                    log::info!("Sent Task: {}", instance.key);
                }
                Err(error) => {
                    log::error!("Failed to sent task {:?}. Error: {}", original_image_abs_path, error);
                    send_internal_server_error(&instance, sessions.clone()).await;
                }
            }
        }

        ///
        /// Generic `INTERNAL_SERVER_ERROR` message for sending to websocket client with the
        /// help of `ws_sessions` and instance `task_group`.
        ///
        pub async fn send_internal_server_error(instance: &BackgroundRemoverTask,
                                                sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let response = build_standard_response(
                "failed",
                "internal_server_error",
                Some("Failed to process result. Reason: Internal Server Error"),
                None,
                None,
            );

            // Extracts current task websocket session
            let mut sessions = sessions.lock().await;
            let ws_session = sessions.get_mut(&instance.task_group.to_string());

            if let Some(session) = ws_session {
                match session.send_json(&response).await {
                    Ok(_) => {
                        log::info!("Sent server error message");
                    }
                    Err(closed) => {
                        log::error!(
                            "Unable to send error message to websocket client {}",
                            closed
                        );
                        sessions.remove(&instance.task_group.to_string());
                    }
                }
            }
        }
    }

    ///
    /// Websocket functions implementation logic related to background remover task.
    ///
    pub mod task_ws {
        use std::collections::HashMap;
        use std::env;
        use std::sync::Arc;
        use racoon::core::websocket::{Message, Websocket};

        use tokio::sync::mpsc::Sender;
        use tokio::sync::Mutex;

        use serde_json::Value;
        use sqlx::Error;
        use uuid::Uuid;

        use crate::db::DBWrapper;
        use crate::db::models::{BackgroundRemoverTask};

        use crate::SharedContext;
        use crate::implementations::websocket::services::{build_standard_response, send_message};

        ///
        /// Waits for message from WebSocket client. New connection will be added to `ws_sessions`
        /// for later use. If connection is closed for any reason, the current `ws_session` will be
        /// removed from the `ws_sessions` list.
        ///
        /// If `key` is sent as message, it starts processing the task with that key. If env variable
        /// `HARD_PROCESS` is not set to `true`, it sends already processed result to the `ws_session` .
        ///
        pub async fn listen_ws_message(shared_context: &SharedContext, task_group: &Uuid, websocket: &mut Websocket) {
            let ws_sessions = shared_context.websocket_connections.sessions.clone();

            // Inserts the current websocket session to the HashMap.
            // These websocket sessions will be used by bp_request_client module handler to forward messages.
            let ws_sessions_set_ref = ws_sessions.clone();
            {
                let mut sessions_map = ws_sessions_set_ref.lock().await;
                sessions_map.insert(task_group.to_string(), websocket.clone());
            }

            let sessions_ref_messages = ws_sessions.clone();

            while let Some(message) = websocket.message().await {
                match message {
                    Message::Text(message) => {
                        handle_received_message(
                            shared_context.db_wrapper.clone(),
                            shared_context.tx_image_mpsc_channel.clone(),
                            &task_group,
                            sessions_ref_messages.clone(),
                            message.to_string(),
                        ).await;
                    }
                    Message::Close(close_code, reason) => {
                        println!("WS connection closed. Code: {} Reason: {}", close_code, reason);
                    }
                    _ => {}
                }
            }
        }

        ///
        /// Message is received from the websocket client. Performs `task_id`, `task_group` ownership
        /// check. Starts processing task if not processed. If `HARD_PROCESS` is set to `true`,
        /// it forces to process the task again.
        ///
        async fn handle_received_message(db_wrapper: DBWrapper,
                                         tx_image_channels: Sender<BackgroundRemoverTask>,
                                         task_group: &Uuid,
                                         ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>,
                                         message: String) {
            log::debug!("Received from websocket client: {}", message);

            // Parses string message to JSON
            let data: Value = match serde_json::from_str(&message) {
                Ok(value) => value,
                Err(error) => {
                    log::error!("Message received is not JSON. Error: {}", error);

                    let error_response = build_standard_response(
                        "failed",
                        "invalid_message_format",
                        Some("Message is not a valid JSON."),
                        None,
                        None,
                    );
                    send_message(task_group, ws_sessions, error_response).await;
                    return;
                }
            };

            // Extracts task id from websocket message
            let key;
            if let Some(value) = data.get("key").and_then(Value::as_str) {
                key = value;
            } else {
                return;
            }

            let task_id = match Uuid::parse_str(&key) {
                Ok(uuid) => uuid,
                Err(error) => {
                    log::error!("Unable to convert key to UUID. Error: {}", error);
                    let error_response = build_standard_response(
                        "failed",
                        "invalid_key_format",
                        Some("Invalid image key format."),
                        None,
                        None,
                    );

                    send_message(&task_group, ws_sessions, error_response).await;
                    return;
                }
            };

            let background_remover_task = match BackgroundRemoverTask::fetch(
                db_wrapper.clone(), &task_id).await {
                Ok(task) => task,
                Err(error) => {
                    match error {
                        Error::RowNotFound => {
                            notify_image_key_does_not_exist(task_group, ws_sessions).await;
                            log::error!("Invalid image key");
                            return;
                        }
                        _ => {
                            log::error!("Failed to fetch background remover instance. Error: {}", error);
                            notify_internal_server_error(task_group, ws_sessions).await;
                            return;
                        }
                    }
                }
            };

            if task_group.to_string() != background_remover_task.task_group.to_string() {
                notify_task_id_does_not_match(task_group, ws_sessions).await;
                return;
            }

            // Serializes to model instance to JSON
            let serialized = match background_remover_task.serialize() {
                Ok(serialized) => serialized,
                Err(error) => {
                    log::error!("Failed to serialize background remover task instance. Error: {}", error);
                    notify_internal_server_error(task_group, ws_sessions).await;
                    return;
                }
            };

            let is_already_processed = background_remover_task.processed_image_path.is_some();
            let is_process_hard = env::var("PROCESS_HARD")
                .unwrap_or_else(|_| "false".to_string()).to_lowercase() == "true";

            // Checks if image is already processed or not
            // If process hard is set, it sends for processing again.
            if is_already_processed && !is_process_hard {
                notify_image_already_processed(task_group, serialized, ws_sessions).await;
            } else {
                // Update processing result
                log::info!("Updating processing state");
                update_processing_state(db_wrapper.clone(), &task_id, false).await;

                log::info!("Sending image for processing to tx_image_channels");
                let _ = tx_image_channels.send(background_remover_task).await;
                notify_image_processing(task_group, serialized, ws_sessions).await;
            }
        }

        ///
        /// Updates processing state instance in the database.
        ///
        pub async fn update_processing_state(db_wrapper: DBWrapper, task_id: &Uuid, state: bool) {
            match BackgroundRemoverTask::update_processing_state(db_wrapper, &task_id,
                                                                 state).await {
                Ok(_) => {
                    log::debug!("Processing state updated");
                }
                Err(error) => {
                    log::error!("Failed to update processing state. Error: {}", error);
                }
            };
        }

        ///
        /// Sends generic  `INTERNAL_SERVER_ERROR` response to the websocket client.
        ///
        pub async fn notify_internal_server_error(task_group: &Uuid,
                                                  ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let response = build_standard_response(
                "failed",
                "internal_server_error",
                Some("Failed to process request. Reason: Internal Server Error"),
                None,
                None,
            );

            send_message(task_group, ws_sessions, response).await;
        }

        ///
        /// Sends mismatched task group error response to websocket client. This occurs when
        /// the image client requests to access or process task, but the task is uploaded with
        /// different task_group.
        ///
        pub async fn notify_task_id_does_not_match(task_group: &Uuid,
                                                   ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let response = build_standard_response(
                "failed",
                "mismatched_task_group",
                Some("This key does not belong to the task group."),
                None,
                None,
            );

            send_message(task_group, ws_sessions, response).await;
        }

        ///
        /// Sends image key does not exist message to the websocket client.
        ///
        pub async fn notify_image_key_does_not_exist(task_group: &Uuid,
                                                     ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let response = build_standard_response(
                "failed",
                "invalid_image_key",
                Some("This image key does not exist"),
                None,
                None,
            );

            send_message(task_group, ws_sessions, response).await;
        }

        ///
        /// Sends image already processed message with serialized task instance to the websocket client.
        ///
        pub async fn notify_image_already_processed(task_group: &Uuid, serialized_data: Value,
                                                    ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let response = build_standard_response(
                "success",
                "result",
                Some("Image already processed"),
                Some(serialized_data),
                None,
            );

            send_message(task_group, ws_sessions, response).await;
        }

        ///
        /// Sends image is processing message with serialized task instance to the websocket client.
        ///
        pub async fn notify_image_processing(task_group: &Uuid, serialized_data: Value,
                                             ws_sessions: Arc<Mutex<HashMap<String, Websocket>>>) {
            let response = build_standard_response(
                "pending",
                "result",
                Some("Please wait. Image is currently in processing status."),
                Some(serialized_data),
                None,
            );

            send_message(task_group, ws_sessions, response).await;
        }
    }
}
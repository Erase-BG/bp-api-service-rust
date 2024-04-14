use std::io::{Read, Write};
use std::net::TcpStream;
use std::{env, vec};
use std::path::Path;
use std::sync::{Arc};
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::sleep;

use tej_protoc::protoc::decoder::decode_tcp_stream;
use tej_protoc::protoc::encoder::{build_bytes_for_message, build_raw_bytes};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;

use crate::{ResponseHandlerSharedData};
use crate::clients::bp_request_client::handlers::handle_response_received_from_server;

mod handlers;


const NORMAL_STATUS: u8 = 1;
const PING_STATUS: u8 = 2;
const PROTOCOL_VERSION: u8 = 1;

#[derive(Serialize, Deserialize, Debug)]
pub struct Timestamps {
    pub request_client_to_bp_server_sent: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RequestRemoveTask {
    pub task_id: String,
    pub timestamps: Timestamps,
}

pub struct BPRequestClient {
    pub address: String,
    /// This stream should only be used for writing data. Reading is handled by calling `listen`
    /// function.
    pub tx_tcp_stream: Arc<Mutex<Option<TcpStream>>>,
    pub retry_duration: Duration,
}

impl BPRequestClient {
    pub fn new(address: String, retry_duration: Duration) -> Self {
        return Self {
            address,
            tx_tcp_stream: Arc::new(Mutex::new(None)),
            retry_duration,
        };
    }

    ///
    /// This function spawns new thread and creates the new TCP connection. If the connection fails,
    /// it tries to reconnect with the duration `retry_duration`.
    ///
    /// Whenever the new connection is established, `tcp_stream` field is set to new TcpStream.
    /// If connection fails, `tcp_stream` is set to `None`.
    ///
    pub async fn handle_response(&mut self, app_data: Arc<ResponseHandlerSharedData>) {
        let address = self.address.clone();
        let tx_tcp_stream_ref = self.tx_tcp_stream.clone();
        let retry_duration = self.retry_duration.clone();

        tokio::spawn(async move {
            let tcp_stream_ref = tx_tcp_stream_ref.clone();
            log::info!("Connecting to BP Server...");

            let callback_ref = app_data.clone();

            loop {
                let tcp_stream;
                let address_cloned = address.clone();

                // Unexpected behaviour of blocking Postgres pool is seen when std TCP stream is
                // executed in asynchronous runtime. To prevent deadlock, task is spawned in new
                // thread.
                let handler = tokio::task::spawn_blocking(move || {
                    return TcpStream::connect(address_cloned);
                });
                tcp_stream = handler.await.unwrap();

                let callback_ref = callback_ref.clone();

                let tx_tcp_stream_set_ref = tcp_stream_ref.clone();
                let tx_tcp_stream_ping_ref = tcp_stream_ref.clone();

                match tcp_stream {
                    Ok(mut tcp_stream) => {
                        log::info!("Connected successfully.");
                        Self::handshake(&mut tcp_stream);

                        // Acquires lock and sets new TcpStream to tcp_stream field.
                        // It is inside the scope to release lock instantly since another thread
                        // may want to access tcp_stream field for write operations.
                        {
                            let mut tcp_stream_lock = tx_tcp_stream_set_ref.lock().await;
                            *tcp_stream_lock = Some(tcp_stream.try_clone().unwrap());
                        }

                        let _ = tokio::spawn(async move {
                            log::info!("Sending periodic ping bytes...");
                            Self::ping(tx_tcp_stream_ping_ref.clone()).await;
                        });

                        Self::handle_bytes_read(tcp_stream.try_clone().unwrap(), callback_ref.clone()).await;
                    }

                    Err(error) => {
                        log::error!("Error: {:?}", error);
                    }
                }

                let mut tcp_stream_lock = tcp_stream_ref.lock().await;
                *tcp_stream_lock = None;

                log::info!("Reconnecting in {} seconds...", retry_duration.as_secs_f64());
                sleep(retry_duration.clone()).await;
            }
        });
    }

    ///
    /// Handshake as request client with the Server.
    ///
    /// It is done by sending following JSON message with `tej_protoc` protocol.
    /// ```
    /// {
    ///     "client_type": "request",
    ///     "auth_token": "secret_token"
    /// }
    /// ```
    ///
    fn handshake(tcp_stream: &mut TcpStream) {
        #[derive(Serialize, Deserialize, Debug)]
        struct HandshakeRequest {
            client_type: String,
            auth_token: String,
        }

        let bp_server_auth_token = match env::var("BP_SERVER_AUTH_TOKEN") {
            Ok(token) => token,
            Err(error) => {
                log::error!("BP_SERVER_AUTH_TOKEN is missing from environment variable.");
                log::error!("{}", error.to_string());
                std::process::exit(-1);
            }
        };

        let handshake_request = HandshakeRequest {
            client_type: "request".to_string(),
            auth_token: bp_server_auth_token,
        };

        let handshake_request_json = serde_json::to_string(&handshake_request).unwrap();
        let bytes = build_bytes_for_message(&handshake_request_json.as_bytes().to_vec());
        tcp_stream.write_all(&bytes).unwrap()
    }

    async fn ping(tcp_stream: Arc<Mutex<Option<TcpStream>>>) {
        loop {
            let ping_bytes = build_raw_bytes(
                PING_STATUS,
                PROTOCOL_VERSION,
                &vec![],
                &vec![],
            );

            {
                let mut lock = tcp_stream.lock().await;
                if let Some(tcp_stream) = lock.as_mut() {
                    match tcp_stream.write_all(&ping_bytes) {
                        Ok(_) => {}
                        Err(error) => {
                            log::error!("Error: {}", error.to_string());

                            // Error occurred, breaks this loop.
                            // The reader and decoder will also fail running in separate thread
                            // resulting next connection process to start.
                            break;
                        }
                    }
                }
            }

            sleep(Duration::from_secs(2)).await;
        };
    }

    ///
    /// This will call the mutable closure when new data is received from the server.
    ///
    /// If decoding response is failed, the function returns void.
    ///
    async fn handle_bytes_read(mut tcp_stream: TcpStream, shared_data: Arc<ResponseHandlerSharedData>) {
        loop {
            let decoded_response = decode_tcp_stream(&mut tcp_stream);
            match decoded_response {
                Ok(decoded_response) => {
                    handle_response_received_from_server(decoded_response, shared_data.clone()).await;
                }

                Err(error) => {
                    log::error!("Error: {}", error);
                    break;
                }
            }
        }
    }

    fn read_file_bytes(file_path: &str) -> Result<Vec<u8>, String> {
        // Tries to open file
        return match std::fs::File::open(file_path) {
            Ok(mut file) => {
                let mut buffer = Vec::new();

                // Reads file to end and put it in the buffer
                match &file.read_to_end(&mut buffer) {
                    Ok(_) => {
                        Ok(buffer)
                    }
                    Err(error) => {
                        Err(error.to_string())
                    }
                }
            }

            Err(error) => {
                Err(error.to_string())
            }
        };
    }

    pub async fn send_remove_task(tcp_stream: Arc<Mutex<Option<TcpStream>>>, task_id: Uuid,
                                  file_path: &str) -> Result<(), String> {
        let request_message = RequestRemoveTask {
            task_id: task_id.to_string(),
            timestamps: Timestamps {
                request_client_to_bp_server_sent: Utc::now().timestamp(),
            },
        };

        let path = Path::new(file_path);
        let filename;
        if let Some(filename_value) = path.file_name() {
            filename = filename_value.to_string_lossy().to_string();
        } else {
            return Err("Failed to extract filename from path".to_owned());
        }

        let file_bytes_read = match Self::read_file_bytes(file_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::error!("Failed to read bytes from file");
                return Err(error.to_string());
            }
        };

        // Access TcpStream
        let mut tcp_stream = tcp_stream.lock().await;
        if let Some(tcp_stream) = tcp_stream.as_mut() {
            // Construct message to bytes
            let message_bytes = match serde_json::to_string(&request_message) {
                Ok(json_text) => json_text.as_bytes().to_vec(),
                Err(error) => {
                    return Err(error.to_string());
                }
            };

            let file_bytes = tej_protoc::protoc::File::new(filename.as_bytes().to_vec(), file_bytes_read);
            let files = vec![&file_bytes];

            // Response bytes ready to send through stream.
            let response_bytes = build_raw_bytes(NORMAL_STATUS, PROTOCOL_VERSION, &files, &message_bytes);

            return match tcp_stream.write_all(&response_bytes) {
                Ok(()) => {
                    Ok(())
                }
                Err(error) => {
                    log::error!("Failed to send well formed protoc bytes to TcpStream.");
                    Err(error.to_string())
                }
            };
        }

        return Err("BP request client not connected to BP Server".to_owned());
    }
}

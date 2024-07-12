use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::lock::Mutex;
use futures_util::Future;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tej_protoc::protoc::encoder::build_bytes_for_message;
use tej_protoc::{protoc::File, stream::Stream};

use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::sleep;

pub struct BPRequestClient {
    address: String,
    buffer_size: usize,
    reconnect_duration: Duration,
    stream_holder: Arc<Mutex<Option<Arc<Stream>>>>,
}

impl BPRequestClient {
    pub fn new<S: AsRef<str>>(
        address: S,
        buffer_size: usize,
        reconnect_duration: Duration,
    ) -> Self {
        let address = address.as_ref().to_string();

        Self {
            address,
            buffer_size,
            reconnect_duration,
            stream_holder: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn listen<F, Fut>(&self, mut callback: F) -> JoinHandle<()>
    where
        F: FnMut(Vec<File>, Value) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        Fut::Output: Send + Sync + 'static,
    {
        let address = self.address.clone();
        let buffer_size = self.buffer_size.clone();
        let reconnect_duration = self.reconnect_duration.clone();

        let stream_holder = self.stream_holder.clone();

        tokio::spawn(async move {
            loop {
                // Creates TcpStream
                let tcp_stream = match TcpStream::connect(address.clone()).await {
                    Ok(tcp_stream) => tcp_stream,
                    Err(error) => {
                        eprintln!("Failed to connect to BP Server. Error: {}", error);
                        Self::wait_reconnect(reconnect_duration.clone()).await;
                        continue;
                    }
                };

                // Abstracts TcpStream with TcpStreamWrapper
                let tcp_stream_wrapper =
                    match tej_protoc::stream::TcpStreamWrapper::new(tcp_stream, buffer_size) {
                        Ok(tcp_stream_wrapper) => tcp_stream_wrapper,
                        Err(error) => {
                            eprintln!("Failed to wrap tcp stream. Error: {}", error);
                            Self::wait_reconnect(reconnect_duration.clone()).await;
                            continue;
                        }
                    };

                // Abstracted Stream type
                let stream: Arc<Stream> = Arc::new(Box::new(tcp_stream_wrapper));

                {
                    // Set same stream to allow sending data.
                    let mut stream_holder = stream_holder.lock().await;
                    *stream_holder = Some(stream.clone());
                }

                // Handshakes as request client.
                match Self::handshake(stream.clone()).await {
                    Ok(()) => {
                        println!("Handshake completed.");
                    }
                    Err(error) => {
                        eprintln!("Handshake failed with bp server. Error: {}", error);
                        Self::wait_reconnect(reconnect_duration.clone()).await;
                        continue;
                    }
                };

                // Listens response in loop
                Self::listen_stream_response(stream.clone(), &mut callback).await;

                {
                    // Set same stream to allow sending data.
                    let mut stream_holder = stream_holder.lock().await;
                    stream_holder.take();
                }

                Self::wait_reconnect(reconnect_duration).await;
            }
        })
    }

    ///
    /// Handshakes as request client with the Server.
    ///
    /// It is done by sending following JSON message with `tej_protoc` protocol.
    /// ```
    /// {
    ///     "client_type": "request",
    ///     "auth_token": "secret_token"
    /// }
    /// ```
    ///
    async fn handshake(tcp_stream: Arc<Stream>) -> std::io::Result<()> {
        #[derive(Serialize, Deserialize, Debug)]
        struct HandshakeRequest<'a> {
            client_type: &'a str,
            auth_token: String,
        }

        let bp_server_auth_token = match env::var("BP_SERVER_AUTH_TOKEN") {
            Ok(token) => token,
            Err(error) => {
                eprintln!(
                    "BP_SERVER_AUTH_TOKEN is missing from environment variable. Error: {}",
                    error
                );
                std::process::exit(-1);
            }
        };

        let handshake_request = HandshakeRequest {
            client_type: "request",
            auth_token: bp_server_auth_token,
        };

        let handshake_request_json = serde_json::to_string(&handshake_request).unwrap();

        let bytes = build_bytes_for_message(&handshake_request_json.as_bytes().to_vec());
        tcp_stream.write_chunk(&bytes).await?;

        Ok(())
    }

    async fn wait_reconnect(reconnect_duration: Duration) {
        println!("Reconnecting in {:?} ...", reconnect_duration);
        sleep(reconnect_duration).await;
    }

    async fn listen_stream_response<F, Fut>(stream: Arc<Stream>, callback: &mut F)
    where
        F: FnMut(Vec<File>, Value) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        Fut::Output: Send + Sync + 'static,
    {
        loop {
            let decoded_response =
                match tej_protoc::protoc::decoder::decode_tcp_stream(stream.clone()).await {
                    Ok(decoded_response) => decoded_response,
                    Err(error) => {
                        eprintln!("Failed to receive decoded response. Error: {}", error);
                        break;
                    }
                };

            let message = String::from_utf8_lossy(&decoded_response.message).to_string();
            let message_json = match Value::from_str(&message) {
                Ok(json_value) => json_value,
                Err(error) => {
                    eprintln!("Failed to parse message to JSON. Error: {}", error);
                    break;
                }
            };

            // Passes received data back to the caller.
            callback(decoded_response.files, message_json).await;
        }
    }

    pub async fn send(&self, files: &[File], message: &Value) -> std::io::Result<()> {
        let mut files_vec = vec![];
        for file in files {
            files_vec.push(file);
        }

        let message = message.to_string().as_bytes().to_vec();
        let encoded_bytes =
            tej_protoc::protoc::encoder::build_bytes(Some(&files_vec), Some(&message));

        {
            let stream_holder = self.stream_holder.lock().await;
            if let Some(stream) = stream_holder.as_ref() {
                stream.write_chunk(&encoded_bytes).await?;
            } else {
                return Err(std::io::Error::other(
                    "BP Request client not connected to server.",
                ));
            }
        }

        Ok(())
    }
}

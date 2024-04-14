mod routes;
mod forms;
mod utils;
mod db;
mod clients;
mod implementations;

use std::{env};
use std::collections::HashMap;
use std::io::{ErrorKind};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::{Arc};
use std::time::Duration;

use tokio;

use dotenv::dotenv;

use actix_web::{App, HttpServer, web};
use actix_ws::Session;
use actix_cors::Cors;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, Mutex};
use tokio::sync::mpsc::{Receiver, Sender};

use sqlx::{PgPool};

use crate::clients::bp_request_client::BPRequestClient;
use crate::routes::{auth_upload, non_auth_upload, task_details, ws_result_websocket};

use crate::db::DBWrapper;
use crate::db::models::{BackgroundRemoverTask};

use crate::implementations::services::task::send_new_task_to_bp_server;


///
/// Holds connected websocket sessions
///
pub struct WebSocketConnections {
    /// Task Group and WebSocket session
    pub sessions: Arc<Mutex<HashMap<String, Session>>>,
}

///
/// Implements `Clone` trait to safely transfer around threads.
///
impl Clone for WebSocketConnections {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone()
        }
    }
}

///
/// Shared AppState between routes.
///
pub struct AppData {
    /// Use this to transmit images to MPSC channel
    pub tx_image_mpsc_channel: Sender<BackgroundRemoverTask>,
    /// Database wrapper for holding postgres connection pools
    pub db_wrapper: DBWrapper,
    /// Holds websocket sessions
    pub websocket_connections: WebSocketConnections,
}

///
/// Shared data for response handler module to handle response received from bp server.
///
pub struct ResponseHandlerSharedData {
    pub db_wrapper: DBWrapper,
    pub websocket_connections: WebSocketConnections,
}

///
/// From API endpoint image is uploaded and transmitter is triggered.
/// If new background removal task is received, it will be sent the background processing
/// server.
///
/// The result is received in `BPRequestClient` listen function.
///
async fn new_image_uploaded(tx_tcp_stream: Arc<Mutex<Option<TcpStream>>>,
                            sessions: Arc<Mutex<HashMap<String, Session>>>)
                            -> Sender<BackgroundRemoverTask> {
    let (tx, mut rx): (Sender<BackgroundRemoverTask>, Receiver<BackgroundRemoverTask>) = mpsc::channel(100);
    let tcp_stream_ref = tx_tcp_stream.clone();

    // Spawning new task for receiving new tasks in background
    tokio::spawn(async move {
        loop {
            let tcp_stream_ref_clone = tcp_stream_ref.clone();
            let new_background_removal_task = rx.recv().await;

            if let Some(instance) = new_background_removal_task {
                send_new_task_to_bp_server(tcp_stream_ref_clone, instance, sessions.clone()).await;
            }
        }
    });
    return tx;
}

///
/// Initializes required items for `AppData` and `BPRequestClient`.
///
async fn init_app_data() -> Result<(AppData, BPRequestClient), String> {
    // Normally IP Address for connecting to background processing server.
    let bp_server_host;
    match env::var("BP_SERVER_HOST") {
        Ok(value) => {
            bp_server_host = value;
        }
        Err(error) => {
            return Err(error.to_string());
        }
    };

    // WebSocket connections instance
    let websocket_connections = WebSocketConnections {
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    // Extract postgres url
    let postgres_url = match env::var("POSTGRES_URL") {
        Ok(value) => value,
        Err(error) => {
            log::error!("Failed to read POSTGRES_URL from environment variable. Probably missing.");
            return Err(error.to_string());
        }
    };

    // Database pool
    let db_pool;
    match PgPool::connect(&postgres_url).await {
        Ok(pool) => {
            db_pool = pool;
        }
        Err(error) => {
            return Err(error.to_string());
        }
    }

    // BP Request Client instance
    let bp_request_client = BPRequestClient::new(
        bp_server_host,
        Duration::from_secs(2),
    );

    // Sender for uploading image
    let tx_image_mpsc_channel = new_image_uploaded(
        bp_request_client.tx_tcp_stream.clone(),
        websocket_connections.sessions.clone(),
    ).await;

    let db_wrapper = DBWrapper {
        connection: db_pool,
    };

    // Common shared instances for endpoint services.
    let app_data = AppData {
        tx_image_mpsc_channel,
        db_wrapper,
        websocket_connections,
    };

    Ok((app_data, bp_request_client))
}

///
/// Main entry point of application
///
#[tokio::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("debug"));

    // Initialize app data
    let (app_data, mut client) = init_app_data().await.unwrap_or_else(|error| {
        log::error!("Error: {}", error);
        std::process::exit(-1);
    });

    // Creates required tables
    match db::setup(app_data.db_wrapper.clone()).await {
        Ok(()) => {}
        Err(error) => {
            return Err(std::io::Error::new(ErrorKind::Other, error));
        }
    };

    // Data required for before and after response from server are kept here
    let response_handler_shared_data = Arc::new(
        ResponseHandlerSharedData {
            db_wrapper: app_data.db_wrapper.clone(),
            websocket_connections: app_data.websocket_connections.clone(),
        });

    // Runs in the background
    client.handle_response(response_handler_shared_data).await;
    let app_data = web::Data::new(app_data);

    // Shutdowns request client's TcpStream if connected after receiving interrupts signals
    tokio::spawn(async move {
        let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT listen failed");
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM listen failed");

        // Waits until any one of the signal is received
        tokio::select! {
            _ = sigint.recv() => {},
            _ = sigterm.recv() => {}
        }

        log::info!("Shutting down bp request client");
        let mut tx_tcp_stream = client.tx_tcp_stream.lock().await;
        if let Some(tcp_stream) = tx_tcp_stream.as_mut() {
            log::info!("Closing bp request client TcpStream");
            let _ = tcp_stream.shutdown(Shutdown::Both);
        }
    });

    let server = HttpServer::new(move || {
        let cors = Cors::permissive();

        App::new()
            .wrap(cors)
            .app_data(app_data.clone())
            .service(non_auth_upload)
            .service(auth_upload)
            .service(task_details)
            .service(ws_result_websocket)
    });

    match env::var("SOCK_PATH") {
        Ok(sock_path) => {
            // Started web server in unix socket
            log::info!("Server is running at {}", sock_path);

            // Deletes sock file if already exists
            if Path::new(&sock_path).exists() {
                std::fs::remove_file(&sock_path).expect("Failed to remove existing socket file");
            }

            // Binds server to unix socket
            // For some reason not working
            server.bind_uds(&sock_path)?.run().await?;
        }
        Err(_) => {
            log::info!("Server is running at http://127.0.0.1:8080");

            // Binds web server to port
            server.bind(("127.0.0.1", 8080))?.run().await?;
        }
    }
    Ok(())
}
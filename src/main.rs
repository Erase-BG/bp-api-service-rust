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
use std::sync::{Arc};
use std::time::Duration;

use tokio;

use dotenv::dotenv;

use racoon::core::path::{Path, View};
use racoon::core::request::Request;
use racoon::core::server::Server;
use racoon::core::websocket::Websocket;
use racoon::{view, wrap_view};
use racoon::core::headers::HeaderValue;
use racoon::core::response::Response;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, Mutex};
use tokio::sync::mpsc::{Receiver, Sender};

use sqlx::{PgPool};

use crate::clients::bp_request_client::BPRequestClient;

use crate::db::DBWrapper;
use crate::db::models::{BackgroundRemoverTask};

use crate::implementations::websocket::services::task::send_new_task_to_bp_server;
use crate::routes::{public_upload_view, task_details_view, ws_view};


///
/// Holds connected websocket sessions
///
pub struct WebSocketConnections {
    /// Task Group and WebSocket session
    pub sessions: Arc<Mutex<HashMap<String, Websocket>>>,
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
pub struct SharedContext {
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
                            sessions: Arc<Mutex<HashMap<String, Websocket>>>)
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
async fn init_app_data() -> Result<(SharedContext, BPRequestClient), String> {
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
    let app_data = SharedContext {
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
    env::set_var("RACOON_LOGGING", "true");
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
    client.handle_response(response_handler_shared_data.clone()).await;

    let db_wrapper_delete_scheduler = response_handler_shared_data.db_wrapper.clone();
    tokio::spawn(async move {
        implementations::auto_delete_files::run_auto_delete(db_wrapper_delete_scheduler).await;
    });

    let paths = vec![
        Path::new("/v1/bp/u/", view!(public_upload_view)),
        Path::new("/v1/remove-background/details/{task_id}/", view!(task_details_view)),
        Path::new("/ws/remove-background/{task_group}/", view!(ws_view)),
    ];

    async fn middleware(request: Request, view: Option<View>) -> Response {
        println!("-----------In------------------");
        println!("{:?} {:?}", request.method, request.path);
        for (k, v) in &request.headers {
            println!("{}: {:?}", k, String::from_utf8_lossy(&v[0]));
        }
        // println!("{:?}", request.headers);
        println!("-------------------------------");

        println!("---------response --------------");
        let mut response = Path::resolve(request, view).await;
        let mut headers = response.get_headers();
        headers.insert_single_value("Access-Control-Allow-Origin", b"*");
        headers.insert_single_value("Access-Control-Allow-Methods", b"GET, POST, PUT, DELETE");
        println!("Response: {:?}", String::from_utf8_lossy(&response.get_body()));
        response
    }

    let server = Server::bind("127.0.0.1:8080")
        .context(app_data)
        .wrap(wrap_view!(middleware))
        .urls(paths)
        .run().await;

    println!("Result: {:?}", server);
    Ok(())
}
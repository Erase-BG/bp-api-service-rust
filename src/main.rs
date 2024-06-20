mod clients;
mod db;
mod forms;
mod implementations;
mod routes;
mod utils;

use std::collections::HashMap;
use std::env;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use dotenv::dotenv;

use racoon::core::headers::HeaderValue;
use racoon::core::path::{Path, View};
use racoon::core::request::Request;
use racoon::core::response::Response;
use racoon::core::server::Server;
use racoon::core::websocket::WebSocket;
use racoon::{view, wrap_view};

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, Mutex};

use sqlx::PgPool;

use crate::clients::bp_request_client::BPRequestClient;

use crate::db::models::BackgroundRemoverTask;
use crate::db::DBWrapper;

use crate::implementations::websocket::services::task::send_new_task_to_bp_server;
use crate::routes::{public_upload_view, task_details_view, tasks_view, ws_view};

///
/// Holds connected websocket sessions
///
pub struct WebSocketConnections {
    /// Task Group and WebSocket session
    sessions: Arc<Mutex<HashMap<String, Vec<WebSocket>>>>,
}

impl WebSocketConnections {
    pub async fn subscribe(&self, task_group: String, websocket: WebSocket) {
        let mut sessions = self.sessions.lock().await;

        if let Some(ws_wrappers) = sessions.get_mut(&task_group) {
            ws_wrappers.push(websocket);
        } else {
            sessions.insert(task_group, vec![websocket]);
        }
    }

    pub async fn unsubscribe(&self, task_group: &String, uid: &String) {
        let mut sessions = self.sessions.lock().await;

        if let Some(ws_wrappers) = sessions.get_mut(task_group) {
            let mut index = 0;

            while index < ws_wrappers.len() {
                let ws_wrapper = &ws_wrappers[index];

                if ws_wrapper.uid.eq(uid) {
                    ws_wrappers.remove(index);
                }

                index += 1;
            }
        }
    }
}

///
/// Implements `Clone` trait to safely transfer around threads.
///
impl Clone for WebSocketConnections {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone(),
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
/// If new background removal task is received, it will be sent to the background processing
/// server.
///
/// The result is received in `BPRequestClient` listen function.
///
async fn new_image_uploaded(
    tx_tcp_stream: Arc<Mutex<Option<TcpStream>>>,
    ws_connections: WebSocketConnections,
) -> Sender<BackgroundRemoverTask> {
    let (tx, mut rx): (
        Sender<BackgroundRemoverTask>,
        Receiver<BackgroundRemoverTask>,
    ) = mpsc::channel(100);
    let tcp_stream_ref = tx_tcp_stream.clone();

    // Spawning new task for receiving new tasks in background
    tokio::spawn(async move {
        loop {
            let tcp_stream_ref_clone = tcp_stream_ref.clone();
            let new_background_removal_task = rx.recv().await;

            if let Some(instance) = new_background_removal_task {
                send_new_task_to_bp_server(tcp_stream_ref_clone, instance, ws_connections.clone())
                    .await;
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
    let bp_request_client = BPRequestClient::new(bp_server_host, Duration::from_secs(2));

    // Sender for uploading image
    let tx_image_mpsc_channel = new_image_uploaded(
        bp_request_client.tx_tcp_stream.clone(),
        websocket_connections.clone(),
    )
    .await;

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
            return Err(std::io::Error::other(error));
        }
    };

    // Data required for before and after response from server are kept here
    let response_handler_shared_data = Arc::new(ResponseHandlerSharedData {
        db_wrapper: app_data.db_wrapper.clone(),
        websocket_connections: app_data.websocket_connections.clone(),
    });

    // Runs in the background
    client
        .handle_response(response_handler_shared_data.clone())
        .await;

    let db_wrapper_delete_scheduler = response_handler_shared_data.db_wrapper.clone();
    tokio::spawn(async move {
        implementations::auto_delete_files::run_auto_delete(db_wrapper_delete_scheduler).await;
    });

    let paths = vec![
        Path::new("/v1/bp/u/", view!(public_upload_view)),
        Path::new(
            "/v1/remove-background/details/{task_id}/",
            view!(task_details_view),
        ),
        Path::new("/v1/remove-tasks/", view!(tasks_view)),
        Path::new("/ws/remove-background/{task_group}/", view!(ws_view)),
    ];

    async fn middleware(request: Request, view: Option<View>) -> Response {
        println!("Client IP: {:?}", request.remote_addr().await);
        let pid = std::process::id();
        let process_fds_dir = format!("/proc/{}/fd", pid);
        let path = std::fs::read_dir(process_fds_dir);
        match path {
            Ok(files) => {
                println!("---------------------------------------------");
                println!("Process_id: {} file descriptors: {}", pid, files.count());
                println!("---------------------------------------------");
            }
            _ => {}
        }

        let mut response = Path::resolve(request, view).await;
        let headers = response.get_headers();
        headers.set("Access-Control-Allow-Origin", "*");
        headers.set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE");
        response
    }

    let host = env::var("HOST").unwrap();
    let server = Server::bind(host)
        .context(app_data)
        .wrap(wrap_view!(middleware))
        .urls(paths)
        .run()
        .await;

    println!("Result: {:?}", server);
    Ok(())
}

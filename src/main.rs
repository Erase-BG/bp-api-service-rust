use std::sync::Arc;
use std::time::Duration;

use api::task;
use api::ws_clients::WsClients;

use clients::bp_request_client::BPRequestClient;
use db::DBWrapper;
use env_logger::Env;

mod api;
mod clients;
mod db;
mod utils;

#[derive(Clone)]
pub struct SharedContext {
    bp_request_client: Arc<BPRequestClient>,
    db_wrapper: Arc<DBWrapper>,
    ws_clients: Arc<WsClients>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    dotenv::dotenv().ok();

    let db_wrapper = Arc::new(db::setup().await?);
    let ws_clients = Arc::new(WsClients::new());
    let bp_request_client = Arc::new(BPRequestClient::new(
        "127.0.0.1:6789",
        8096,
        Duration::from_secs(3),
    ));

    // Resources shared across API views and task handlers.
    let shared_context = SharedContext {
        bp_request_client: bp_request_client.clone(),
        ws_clients,
        db_wrapper,
    };

    let shared_context_cloned = shared_context.clone();

    bp_request_client
        .listen(move |files, message| {
            let shared_context_cloned = shared_context_cloned.clone();

            async move {
                // Spawns new tokio task. Pros: functions even if crashed, runs tasks in concurrently in background.
                tokio::spawn(async move {
                    task::handle_response_received_from_bp_server(
                        shared_context_cloned,
                        files,
                        message,
                    )
                    .await;
                });
            }
        })
        .await;

    api::run_server(shared_context).await?;
    Ok(())
}

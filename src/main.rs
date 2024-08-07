use std::env;
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

    let bp_server_host = match env::var("BP_SERVER_HOST") {
        Ok(value) => value,
        Err(error) => {
            return Err(std::io::Error::other(format!(
                "BP_SERVER_HOST is missing from environment variable. Error: {}",
                error
            )))
        }
    };

    let db_wrapper = Arc::new(db::setup().await?);
    let ws_clients = Arc::new(WsClients::new());
    let bp_request_client = Arc::new(BPRequestClient::new(
        bp_server_host,
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
                    // These tasks may run for long time. So set timeout to prevent unintended bug
                    // which hangs runtime.
                    let result = tokio::time::timeout(
                        Duration::from_secs(6),
                        task::handle_response_received_from_bp_server(
                            shared_context_cloned,
                            files,
                            message,
                        ),
                    )
                    .await;
                    println!("Handle bp server response result: {:?}", result);
                });
            }
        })
        .await;

    api::run_server(shared_context).await?;
    Ok(())
}

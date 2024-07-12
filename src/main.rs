use std::{sync::Arc, time::Duration};

use api::ws_clients::WsClients;
use clients::bp_request_client::BPRequestClient;
use db::DBWrapper;
use env_logger::Env;

mod api;
mod clients;
mod db;
mod utils;

pub struct SharedContext {
    bp_request_client: Arc<BPRequestClient>,
    db_wrapper: Arc<DBWrapper>,
    ws_clients: Arc<WsClients>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    dotenv::dotenv().ok();

    let bp_request_client = BPRequestClient::new("127.0.0.1:6789", 8096, Duration::from_secs(3));

    bp_request_client
        .listen(|files, message| async move {
            println!("Files: {}", files.len());
            println!("Message: {}", message);
        })
        .await;

    let db_wrapper = db::setup().await?;

    // Resources shared across API views and task handlers.
    let shared_context = SharedContext {
        bp_request_client: Arc::new(bp_request_client),
        db_wrapper: Arc::new(db_wrapper),
        ws_clients: Arc::new(WsClients::new()),
    };

    api::run_server(shared_context).await?;
    Ok(())
}

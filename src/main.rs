use std::{sync::Arc, time::Duration};

use clients::bp_request_client::BPRequestClient;
use env_logger::Env;

mod api;
mod clients;
mod utils;

pub struct SharedContext {
    bp_request_client: Arc<BPRequestClient>,
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

    // Resources shared across API views and task handlers.
    let shared_context = SharedContext {
        bp_request_client: Arc::new(bp_request_client),
    };

    api::run_server(shared_context).await?;
    Ok(())
}

use std::env;

use racoon::core::headers::HeaderValue;
use racoon::core::path::Path;
use racoon::core::path::View;
use racoon::core::request::Request;
use racoon::core::response::Response;
use racoon::core::server::Server;
use racoon::wrap_view;

use crate::SharedContext;

pub mod forms;
pub mod shortcuts;
pub mod task;
pub mod urls;
pub mod views;
pub mod ws_clients;

pub async fn middleware(request: Request, view: Option<View>) -> Response {
    println!("Client IP: {:?}", request.remote_addr().await);

    let shared_context: &SharedContext = request.context().expect("SharedContext is missing.");
    let pid = std::process::id();
    let process_fds_dir = format!("/proc/{}/fd", pid);
    let path = std::fs::read_dir(process_fds_dir);

    match path {
        Ok(files) => {
            println!("---------------------------------------------");
            let db_wrapper = shared_context.db_wrapper.clone();
            println!(
                "Process_id: {} file descriptors: {} db pools: {}",
                pid,
                files.count(),
                db_wrapper.pool.size()
            );
            println!("Connection pool alive: {}", !db_wrapper.pool.is_closed());
            println!("---------------------------------------------");
        }
        _ => {}
    }

    let mut response = Path::resolve(request, view).await;
    let headers = response.get_headers();
    let sid = env::var("SID").unwrap();
    headers.set("SID", sid);
    headers.set("Access-Control-Allow-Origin", "*");
    headers.set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE");
    response
}

pub async fn run_server(shared_context: SharedContext) -> std::io::Result<()> {
    let bind_address =
        env::var("BIND_ADDRESS").expect("BIND_ADDRESS value not present in not found in environment variable.");

    // Available url routes served by the server.
    let urls = urls::register_urls();

    Server::enable_logging();

    Server::bind(bind_address)
        .context(shared_context)
        .wrap(wrap_view!(middleware))
        .urls(urls)
        .run()
        .await?;

    Ok(())
}

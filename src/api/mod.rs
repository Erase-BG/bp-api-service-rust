use std::env;

use racoon::core::path::Path;
use racoon::core::path::View;
use racoon::core::request::Request;
use racoon::core::response::Response;
use racoon::core::server::Server;
use racoon::wrap_view;

use crate::SharedContext;

pub mod forms;
pub mod task;
pub mod urls;
pub mod views;

pub async fn middleware(request: Request, view: Option<View>) -> Response {
    Path::resolve(request, view).await
}

pub async fn run_server(shared_context: SharedContext) -> std::io::Result<()> {
    let bind_address =
        env::var("HOST").expect("HOST value not present in not found in environment variable.");

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

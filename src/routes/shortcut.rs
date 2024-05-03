use racoon::core::headers::HeaderValue;
use racoon::core::response::{AbstractResponse, HttpResponse, Response};
use racoon::core::response::status::ResponseStatus;
use crate::implementations::services::build_standard_response;

///
/// Common implementation for resolving commonly used HttpResponse.
/// Use these shortcuts for returning generic response to users without leaking critical data and
/// error.,
///
/// ```rust
/// Example code:
///
/// HttpResponse:json_internal_server_error()
/// ```
///

pub trait ShortcutResponse {
    fn json_internal_server_error() -> Response;
    fn json(self, text: String) -> Response;
}

impl ShortcutResponse for HttpResponse {
    ///
    /// Returns `Internal Server Error` Response.
    ///
    fn json_internal_server_error() -> Response {
        let error_response = build_standard_response(
            "failed",
            "internal_server_error",
            Some("Internal Server Error"),
            None,
            None,
        );

        let mut response = HttpResponse::internal_server_error();
        let headers = response.get_headers();
        headers.insert_single_value("Content-Type", b"application/json");
        response.body(error_response.to_string().as_str())
    }

    fn json(mut self) -> Response {
        let headers = self.get_headers();
        headers.insert_single_value("Content-Type", b"application/json");
        self.body(error_response.to_string().as_str())
    }
}
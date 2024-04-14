use actix_web::HttpResponse;

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
    fn json_internal_server_error() -> HttpResponse;
}

impl ShortcutResponse for HttpResponse {
    ///
    /// Returns `Internal Server Error` Response.
    ///
    fn json_internal_server_error() -> HttpResponse {
        let error_response = build_standard_response(
            "failed",
            "internal_server_error",
            Some("Internal Server Error"),
            None,
            None,
        );

        HttpResponse::InternalServerError().json(error_response)
    }
}
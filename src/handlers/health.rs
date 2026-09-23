use crate::prelude::*;

/// API: Handles the server ping
#[log()]
pub async fn handle_ping() -> Response {
    Response::ok().text("pong")
}

use crate::prelude::*;

/// API: Handles server ping.
#[log()]
pub async fn handle_ping() -> Response {
    Response::ok().text("pong")
}

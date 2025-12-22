use axum::response::IntoResponse;
use http::StatusCode;

pub async fn handle_iroh_relay() -> impl IntoResponse {
    (StatusCode::OK, b"iroh-relay")
}

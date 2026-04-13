use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;

async fn health() -> StatusCode {
    StatusCode::OK
}

pub fn routes() -> Router {
    Router::new().route("/health", get(health))
}

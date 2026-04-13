use std::sync::Arc;

use axum::routing::post;
use axum::Router;

use crate::handlers::auth_handler::{self, AppState};

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/auth/register", post(auth_handler::register))
        .route("/auth/login", post(auth_handler::login))
        .with_state(state)
}

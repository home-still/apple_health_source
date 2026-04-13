use std::sync::Arc;

use axum::middleware;
use axum::routing::post;
use axum::Router;

use crate::auth::require_auth;
use crate::handlers::auth_handler::AppState;
use crate::handlers::device_handler;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/v1/devices/register",
            post(device_handler::register_device),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

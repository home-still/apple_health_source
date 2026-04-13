use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::post;

use crate::auth::require_auth;
use crate::handlers::auth_handler::AppState;
use crate::handlers::meals_handler;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/meals/parse", post(meals_handler::parse_meal))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

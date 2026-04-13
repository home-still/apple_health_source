pub mod auth_routes;
pub mod device_routes;
pub mod health_check;
pub mod meals_routes;
pub mod sync_routes;

use std::sync::Arc;

use axum::Router;

use crate::handlers::auth_handler::AppState;

pub fn app_router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(health_check::routes())
        .merge(auth_routes::routes(state.clone()))
        .merge(sync_routes::routes(state.clone()))
        .merge(meals_routes::routes(state.clone()))
        .merge(device_routes::routes(state))
}

mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod models;
mod routes;

use std::sync::Arc;

use handlers::auth_handler::AppState;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = config::Config::from_env().expect("failed to load config from environment");

    let pool = db::init_pool(&config.database_url)
        .await
        .expect("failed to connect to database");

    tracing::info!("connected to database");

    let state = Arc::new(AppState {
        pool,
        jwt_secret: config.jwt_secret,
    });

    let app = routes::app_router(state)
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let addr = format!("{}:{}", config.api_host, config.api_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind listener");

    tracing::info!("listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}

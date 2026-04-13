use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::Json;
use sqlx::PgPool;

use crate::auth::jwt;
use crate::error::AppError;
use crate::llm::LlmClient;
use crate::models::health_sample::AuthResponse;
use crate::models::{CreateUser, LoginRequest, User};

pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: String,
    pub llm: std::sync::Arc<dyn LlmClient>,
}

pub async fn register(
    State(state): State<std::sync::Arc<AppState>>,
    Json(payload): Json<CreateUser>,
) -> Result<Json<AuthResponse>, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(payload.password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string();

    let user: User = sqlx::query_as(
        r#"INSERT INTO users (email, password_hash)
           VALUES ($1, $2)
           RETURNING id, email, password_hash, created_at"#,
    )
    .bind(&payload.email)
    .bind(&password_hash)
    .fetch_one(&state.pool)
    .await?;

    let token = jwt::create_token(user.id, &state.jwt_secret)?;
    Ok(Json(AuthResponse {
        token,
        user_id: user.id,
    }))
}

pub async fn login(
    State(state): State<std::sync::Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, password_hash, created_at FROM users WHERE email = $1",
    )
    .bind(&payload.email)
    .fetch_optional(&state.pool)
    .await?;

    let user = user.ok_or_else(|| AppError::Auth("invalid email or password".into()))?;

    let parsed_hash =
        PasswordHash::new(&user.password_hash).map_err(|e| AppError::Internal(e.to_string()))?;

    Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Auth("invalid email or password".into()))?;

    let token = jwt::create_token(user.id, &state.jwt_secret)?;
    Ok(Json(AuthResponse {
        token,
        user_id: user.id,
    }))
}

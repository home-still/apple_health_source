use sqlx::postgres::PgPoolOptions;
use sqlx::{ConnectOptions, PgPool};
use std::str::FromStr;

pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let opts = sqlx::postgres::PgConnectOptions::from_str(database_url)?
        // Fail fast rather than hang forever — 60s is more than enough for any
        // sane sync query against this workload.
        .options([("statement_timeout", "60000")]);

    PgPoolOptions::new()
        .max_connections(20)
        .connect_with(opts)
        .await
}

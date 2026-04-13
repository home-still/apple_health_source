use std::env;

pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub api_host: String,
    pub api_port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            jwt_secret: env::var("JWT_SECRET")?,
            api_host: env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            api_port: env::var("API_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()
                .expect("API_PORT must be a valid u16"),
        })
    }
}

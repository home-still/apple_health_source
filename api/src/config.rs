use std::env;

pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub api_host: String,
    pub api_port: u16,
    pub ollama_api_key: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
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
            ollama_api_key: env::var("OLLAMA_API_KEY")?,
            ollama_base_url: env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "https://ollama.com/v1".into()),
            ollama_model: env::var("OLLAMA_MODEL")
                .unwrap_or_else(|_| "gpt-oss:120b-cloud".into()),
        })
    }
}

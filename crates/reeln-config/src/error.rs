use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config not found: {0}")]
    NotFound(String),

    #[error("invalid config: {0}")]
    Invalid(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

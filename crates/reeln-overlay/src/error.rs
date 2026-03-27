use thiserror::Error;

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("template error: {0}")]
    Template(String),

    #[error("render error: {0}")]
    Render(String),

    #[error("font error: {0}")]
    Font(String),

    #[error("image error: {0}")]
    Image(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

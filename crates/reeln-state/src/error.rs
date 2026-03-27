use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("game directory error: {0}")]
    Directory(String),

    #[error("state persistence error: {0}")]
    Persist(String),

    #[error("replay error: {0}")]
    Replay(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sport error: {0}")]
    Sport(#[from] reeln_sport::SportError),

    #[error("glob pattern error: {0}")]
    GlobPattern(#[from] glob::PatternError),
}

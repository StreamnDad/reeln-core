use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SportError {
    #[error("unknown sport: {0}")]
    UnknownSport(String),

    #[error("invalid segment number: {0}")]
    InvalidSegment(String),
}

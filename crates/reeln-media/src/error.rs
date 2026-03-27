use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("failed to open media file: {0}")]
    OpenFailed(String),

    #[error("no streams found in {0}")]
    NoStreams(String),

    #[error("codec error: {0}")]
    Codec(String),

    #[error("filter error: {0}")]
    Filter(String),

    #[error("concat error: {0}")]
    Concat(String),

    #[error("render error: {0}")]
    Render(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ffmpeg error: {0}")]
    Ffmpeg(String),
}

impl From<ffmpeg_next::Error> for MediaError {
    fn from(e: ffmpeg_next::Error) -> Self {
        MediaError::Ffmpeg(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_ffmpeg_error() {
        let ffmpeg_err = ffmpeg_next::Error::InvalidData;
        let media_err: MediaError = ffmpeg_err.into();
        match &media_err {
            MediaError::Ffmpeg(_) => {}
            other => panic!("expected Ffmpeg variant, got {other:?}"),
        }
        let display = media_err.to_string();
        assert!(display.contains("ffmpeg error"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let media_err: MediaError = io_err.into();
        match &media_err {
            MediaError::Io(_) => {}
            other => panic!("expected Io variant, got {other:?}"),
        }
        let display = media_err.to_string();
        assert!(display.contains("not found"));
    }

    #[test]
    fn test_error_variants_display() {
        let cases: Vec<MediaError> = vec![
            MediaError::OpenFailed("test".into()),
            MediaError::NoStreams("test".into()),
            MediaError::Codec("test".into()),
            MediaError::Filter("test".into()),
            MediaError::Concat("test".into()),
            MediaError::Render("test".into()),
            MediaError::Ffmpeg("test".into()),
        ];
        for err in &cases {
            let msg = err.to_string();
            assert!(msg.contains("test"), "missing 'test' in: {msg}");
        }
    }

    #[test]
    fn test_error_debug() {
        let err = MediaError::OpenFailed("debug test".into());
        let debug = format!("{err:?}");
        assert!(debug.contains("OpenFailed"));
    }
}

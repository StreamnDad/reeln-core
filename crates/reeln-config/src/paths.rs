use std::path::PathBuf;

/// Return the platform-appropriate config directory for reeln.
///
/// macOS: `~/Library/Application Support/reeln/`
/// Linux: `~/.config/reeln/`
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("reeln")
}

/// Return the platform-appropriate data directory for reeln.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("reeln")
}

/// Return the default config file path.
///
/// Without a profile: `<config_dir>/config.json`
/// With a profile: `<config_dir>/config.<profile>.json`
pub fn default_config_path(profile: Option<&str>) -> PathBuf {
    let dir = config_dir();
    match profile {
        Some(p) => dir.join(format!("config.{p}.json")),
        None => dir.join("config.json"),
    }
}

/// Resolve which config path to use.
///
/// If an explicit path is given, use it. Otherwise fall back to
/// `default_config_path(profile)`.
pub fn resolve_config_path(path: Option<&std::path::Path>, profile: Option<&str>) -> PathBuf {
    match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path(profile),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir_ends_with_reeln() {
        let dir = config_dir();
        assert!(dir.ends_with("reeln"));
    }

    #[test]
    fn test_data_dir_ends_with_reeln() {
        let dir = data_dir();
        assert!(dir.ends_with("reeln"));
    }

    #[test]
    fn test_default_config_path_no_profile() {
        let path = default_config_path(None);
        assert_eq!(path.file_name().unwrap(), "config.json");
    }

    #[test]
    fn test_default_config_path_with_profile() {
        let path = default_config_path(Some("dev"));
        assert_eq!(path.file_name().unwrap(), "config.dev.json");
    }

    #[test]
    fn test_resolve_config_path_explicit() {
        let explicit = PathBuf::from("/custom/path.json");
        let result = resolve_config_path(Some(explicit.as_path()), Some("ignored"));
        assert_eq!(result, explicit);
    }

    #[test]
    fn test_resolve_config_path_default_no_profile() {
        let result = resolve_config_path(None, None);
        assert_eq!(result.file_name().unwrap(), "config.json");
    }

    #[test]
    fn test_resolve_config_path_default_with_profile() {
        let result = resolve_config_path(None, Some("prod"));
        assert_eq!(result.file_name().unwrap(), "config.prod.json");
    }
}

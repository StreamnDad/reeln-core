use crate::OverlayError;

/// Resolve a percentage or pixel string (e.g. "50%" or "100") to pixels.
///
/// Supported formats:
/// - `"50%"` — percentage of `container_size`
/// - `"100"` or `"100.5"` — raw pixel value
/// - `"auto"` — returns 0.0 (caller handles auto sizing)
pub fn resolve_dimension(value: &str, container_size: u32) -> Result<f32, OverlayError> {
    if let Some(pct) = value.strip_suffix('%') {
        let pct: f32 = pct
            .trim()
            .parse()
            .map_err(|e| OverlayError::Template(format!("invalid percentage '{value}': {e}")))?;
        Ok(pct / 100.0 * container_size as f32)
    } else if value == "auto" {
        Ok(0.0) // auto sizing handled by caller
    } else {
        value
            .trim()
            .parse::<f32>()
            .map_err(|e| OverlayError::Template(format!("invalid dimension '{value}': {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentage() {
        let result = resolve_dimension("50%", 1920).unwrap();
        assert!((result - 960.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_percentage_100() {
        let result = resolve_dimension("100%", 1080).unwrap();
        assert!((result - 1080.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_percentage_zero() {
        let result = resolve_dimension("0%", 500).unwrap();
        assert!((result - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_percentage_fractional() {
        let result = resolve_dimension("33.3%", 900).unwrap();
        assert!((result - 299.7).abs() < 0.1);
    }

    #[test]
    fn test_percentage_with_whitespace() {
        let result = resolve_dimension(" 50 %", 200).unwrap();
        assert!((result - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pixels_integer() {
        let result = resolve_dimension("100", 1920).unwrap();
        assert!((result - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pixels_float() {
        let result = resolve_dimension("50.5", 1920).unwrap();
        assert!((result - 50.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pixels_zero() {
        let result = resolve_dimension("0", 1920).unwrap();
        assert!((result - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_auto() {
        let result = resolve_dimension("auto", 1920).unwrap();
        assert!((result - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_invalid_string() {
        assert!(resolve_dimension("abc", 1920).is_err());
    }

    #[test]
    fn test_invalid_percentage() {
        assert!(resolve_dimension("abc%", 1920).is_err());
    }

    #[test]
    fn test_container_size_zero() {
        let result = resolve_dimension("50%", 0).unwrap();
        assert!((result - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pixels_with_whitespace() {
        let result = resolve_dimension(" 200 ", 1920).unwrap();
        assert!((result - 200.0).abs() < f32::EPSILON);
    }
}

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::OverlayError;

/// A visual element within an overlay template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Element {
    Rect {
        #[serde(flatten)]
        bounds: Bounds,
        fill: Color,
        #[serde(default)]
        corner_radius: f32,
        #[serde(default)]
        border: Option<Border>,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },
    Text {
        content: String,
        #[serde(flatten)]
        position: Position,
        font: FontSpec,
        color: Color,
        #[serde(default)]
        outline: Option<OutlineSpec>,
        #[serde(default)]
        alignment: Alignment,
        #[serde(default)]
        max_width: Option<f32>,
        /// Conditional visibility: e.g. "{{has_assists}}".
        #[serde(default)]
        visible: Option<String>,
    },
    Image {
        source: ImageSource,
        #[serde(flatten)]
        position: Position,
        #[serde(flatten)]
        size: Size,
        #[serde(default)]
        fit: ImageFit,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },
    Gradient {
        #[serde(flatten)]
        bounds: Bounds,
        stops: Vec<GradientStop>,
        #[serde(default)]
        direction: GradientDirection,
        #[serde(default)]
        corner_radius: f32,
    },
}

fn default_opacity() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub x: String,
    pub y: String,
    pub w: String,
    pub h: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: String,
    pub y: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Size {
    pub w: String,
    pub h: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Color {
    Hex(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontSpec {
    pub family: String,
    pub size: f32,
    #[serde(default)]
    pub weight: Option<String>,
    /// Minimum font size for auto-shrink.
    #[serde(default)]
    pub auto_shrink: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineSpec {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageSource {
    File(PathBuf),
    Variable(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
    Fill,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradientStop {
    pub color: Color,
    pub position: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientDirection {
    Horizontal,
    #[default]
    Vertical,
    Angle(f32),
}

/// Parse a color string into (R, G, B, A) components.
///
/// Supported formats:
/// - `"#RRGGBB"` — hex with no alpha (alpha defaults to 255)
/// - `"#RRGGBBAA"` — hex with alpha
/// - `"rgb(r, g, b)"` — decimal components
/// - Named colors: white, black, red, green, blue, yellow, cyan, magenta, transparent
pub fn parse_color(s: &str) -> Result<(u8, u8, u8, u8), OverlayError> {
    let s = s.trim();

    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        return parse_rgb_color(inner);
    }

    parse_named_color(s)
}

fn parse_hex_color(hex: &str) -> Result<(u8, u8, u8, u8), OverlayError> {
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            Ok((r, g, b, 255))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            let a = u8::from_str_radix(&hex[6..8], 16)
                .map_err(|e| OverlayError::Template(format!("invalid hex color: {e}")))?;
            Ok((r, g, b, a))
        }
        _ => Err(OverlayError::Template(format!(
            "invalid hex color length: expected 6 or 8 hex digits, got {}",
            hex.len()
        ))),
    }
}

fn parse_rgb_color(inner: &str) -> Result<(u8, u8, u8, u8), OverlayError> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return Err(OverlayError::Template(format!(
            "rgb() requires 3 components, got {}",
            parts.len()
        )));
    }
    let r: u8 = parts[0]
        .trim()
        .parse()
        .map_err(|e| OverlayError::Template(format!("invalid rgb red: {e}")))?;
    let g: u8 = parts[1]
        .trim()
        .parse()
        .map_err(|e| OverlayError::Template(format!("invalid rgb green: {e}")))?;
    let b: u8 = parts[2]
        .trim()
        .parse()
        .map_err(|e| OverlayError::Template(format!("invalid rgb blue: {e}")))?;
    Ok((r, g, b, 255))
}

fn parse_named_color(name: &str) -> Result<(u8, u8, u8, u8), OverlayError> {
    match name.to_lowercase().as_str() {
        "white" => Ok((255, 255, 255, 255)),
        "black" => Ok((0, 0, 0, 255)),
        "red" => Ok((255, 0, 0, 255)),
        "green" => Ok((0, 128, 0, 255)),
        "blue" => Ok((0, 0, 255, 255)),
        "yellow" => Ok((255, 255, 0, 255)),
        "cyan" => Ok((0, 255, 255, 255)),
        "magenta" => Ok((255, 0, 255, 255)),
        "transparent" => Ok((0, 0, 0, 0)),
        _ => Err(OverlayError::Template(format!("unknown color: {name}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_color: hex ---

    #[test]
    fn test_parse_hex_6_digit() {
        assert_eq!(parse_color("#FF0000").unwrap(), (255, 0, 0, 255));
    }

    #[test]
    fn test_parse_hex_8_digit() {
        assert_eq!(parse_color("#FF000080").unwrap(), (255, 0, 0, 128));
    }

    #[test]
    fn test_parse_hex_lowercase() {
        assert_eq!(parse_color("#ff00ff").unwrap(), (255, 0, 255, 255));
    }

    #[test]
    fn test_parse_hex_with_alpha_zero() {
        assert_eq!(parse_color("#00000000").unwrap(), (0, 0, 0, 0));
    }

    #[test]
    fn test_parse_hex_invalid_length() {
        assert!(parse_color("#FFF").is_err());
    }

    #[test]
    fn test_parse_hex_invalid_chars() {
        assert!(parse_color("#GGGGGG").is_err());
    }

    // --- parse_color: rgb() ---

    #[test]
    fn test_parse_rgb() {
        assert_eq!(parse_color("rgb(255, 128, 0)").unwrap(), (255, 128, 0, 255));
    }

    #[test]
    fn test_parse_rgb_no_spaces() {
        assert_eq!(parse_color("rgb(0,0,0)").unwrap(), (0, 0, 0, 255));
    }

    #[test]
    fn test_parse_rgb_invalid_component_count() {
        assert!(parse_color("rgb(1,2)").is_err());
    }

    #[test]
    fn test_parse_rgb_overflow() {
        assert!(parse_color("rgb(999,0,0)").is_err());
    }

    #[test]
    fn test_parse_rgb_negative() {
        assert!(parse_color("rgb(-1,0,0)").is_err());
    }

    // --- parse_color: named ---

    #[test]
    fn test_parse_named_white() {
        assert_eq!(parse_color("white").unwrap(), (255, 255, 255, 255));
    }

    #[test]
    fn test_parse_named_black() {
        assert_eq!(parse_color("black").unwrap(), (0, 0, 0, 255));
    }

    #[test]
    fn test_parse_named_red() {
        assert_eq!(parse_color("red").unwrap(), (255, 0, 0, 255));
    }

    #[test]
    fn test_parse_named_green() {
        assert_eq!(parse_color("green").unwrap(), (0, 128, 0, 255));
    }

    #[test]
    fn test_parse_named_blue() {
        assert_eq!(parse_color("blue").unwrap(), (0, 0, 255, 255));
    }

    #[test]
    fn test_parse_named_yellow() {
        assert_eq!(parse_color("yellow").unwrap(), (255, 255, 0, 255));
    }

    #[test]
    fn test_parse_named_cyan() {
        assert_eq!(parse_color("cyan").unwrap(), (0, 255, 255, 255));
    }

    #[test]
    fn test_parse_named_magenta() {
        assert_eq!(parse_color("magenta").unwrap(), (255, 0, 255, 255));
    }

    #[test]
    fn test_parse_named_transparent() {
        assert_eq!(parse_color("transparent").unwrap(), (0, 0, 0, 0));
    }

    #[test]
    fn test_parse_named_case_insensitive() {
        assert_eq!(parse_color("WHITE").unwrap(), (255, 255, 255, 255));
        assert_eq!(parse_color("Red").unwrap(), (255, 0, 0, 255));
    }

    #[test]
    fn test_parse_named_unknown() {
        assert!(parse_color("chartreuse").is_err());
    }

    #[test]
    fn test_parse_color_whitespace() {
        assert_eq!(parse_color("  #FF0000  ").unwrap(), (255, 0, 0, 255));
    }

    // --- serde tests for types ---

    #[test]
    fn test_element_rect_deserialize() {
        let json = r##"{
            "type": "rect",
            "x": "0", "y": "0", "w": "100", "h": "50",
            "fill": "#FF0000"
        }"##;
        let elem: Element = serde_json::from_str(json).unwrap();
        match elem {
            Element::Rect {
                bounds,
                fill: _,
                corner_radius,
                border,
                opacity,
            } => {
                assert_eq!(bounds.x, "0");
                assert_eq!(bounds.w, "100");
                assert!((opacity - 1.0).abs() < f32::EPSILON);
                assert_eq!(corner_radius, 0.0);
                assert!(border.is_none());
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn test_element_text_deserialize() {
        let json = r##"{
            "type": "text",
            "content": "Hello",
            "x": "10", "y": "20",
            "font": { "family": "Arial", "size": 24.0 },
            "color": "#FFFFFF"
        }"##;
        let elem: Element = serde_json::from_str(json).unwrap();
        match elem {
            Element::Text {
                content,
                position,
                font,
                ..
            } => {
                assert_eq!(content, "Hello");
                assert_eq!(position.x, "10");
                assert_eq!(font.family, "Arial");
                assert!((font.size - 24.0).abs() < f32::EPSILON);
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_element_gradient_deserialize() {
        let json = r##"{
            "type": "gradient",
            "x": "0", "y": "0", "w": "100%", "h": "100%",
            "stops": [
                { "color": "#000000", "position": 0.0 },
                { "color": "#FFFFFF", "position": 1.0 }
            ]
        }"##;
        let elem: Element = serde_json::from_str(json).unwrap();
        match elem {
            Element::Gradient {
                stops, direction, ..
            } => {
                assert_eq!(stops.len(), 2);
                assert!(matches!(direction, GradientDirection::Vertical));
            }
            _ => panic!("expected Gradient"),
        }
    }

    #[test]
    fn test_alignment_default() {
        let a = Alignment::default();
        assert!(matches!(a, Alignment::Left));
    }

    #[test]
    fn test_image_fit_default() {
        let f = ImageFit::default();
        assert!(matches!(f, ImageFit::Contain));
    }

    #[test]
    fn test_gradient_direction_default() {
        let d = GradientDirection::default();
        assert!(matches!(d, GradientDirection::Vertical));
    }
}

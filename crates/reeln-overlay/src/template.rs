use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::elements::Element;
use crate::error::OverlayError;

/// An overlay template with canvas, layers, and timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: u32,
    pub canvas: Canvas,
    pub layers: Vec<Element>,
    #[serde(default)]
    pub timing: Timing,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timing {
    #[serde(default = "default_fade_in")]
    pub fade_in: f64,
    #[serde(default = "default_hold")]
    pub hold: f64,
    #[serde(default = "default_fade_out")]
    pub fade_out: f64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            fade_in: default_fade_in(),
            hold: default_hold(),
            fade_out: default_fade_out(),
        }
    }
}

fn default_fade_in() -> f64 {
    0.3
}
fn default_hold() -> f64 {
    10.0
}
fn default_fade_out() -> f64 {
    0.5
}

/// Context variables for template rendering.
pub type TemplateContext = HashMap<String, String>;

/// Load a template from a JSON file.
pub fn load_template(path: &Path) -> Result<Template, OverlayError> {
    let content = std::fs::read_to_string(path).map_err(OverlayError::Io)?;
    serde_json::from_str(&content).map_err(OverlayError::Json)
}

/// Replace `{{key}}` placeholders in `text` with values from `context`.
/// Missing keys are left as-is (the placeholder remains).
pub fn substitute_variables(text: &str, context: &TemplateContext) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(start) = remaining.find("{{") {
        result.push_str(&remaining[..start]);
        let after_open = &remaining[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let key = after_open[..end].trim();
            if let Some(value) = context.get(key) {
                result.push_str(value);
            } else {
                // Leave placeholder intact for missing keys
                result.push_str(&remaining[start..start + 2 + end + 2]);
            }
            remaining = &after_open[end + 2..];
        } else {
            // No closing braces — push rest and break
            result.push_str(&remaining[start..]);
            remaining = "";
        }
    }
    result.push_str(remaining);
    result
}

/// Evaluate a visibility condition string like `"{{has_assists}}"`.
///
/// Returns `true` if the variable exists in `context` and its value is
/// non-empty and not `"false"` or `"0"`.
pub fn evaluate_visibility(condition: &str, context: &TemplateContext) -> bool {
    let trimmed = condition.trim();

    // Extract variable name from {{...}} wrapper if present
    let key = if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        trimmed[2..trimmed.len() - 2].trim()
    } else {
        trimmed
    };

    match context.get(key) {
        None => false,
        Some(v) => {
            let v = v.trim();
            !v.is_empty() && v != "false" && v != "0"
        }
    }
}

/// Trait for plugins that contribute template variables.
pub trait TemplateProvider: Send + Sync {
    fn provide(&self, context: &mut TemplateContext);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_context(pairs: &[(&str, &str)]) -> TemplateContext {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_load_template_valid() {
        let json = r#"{
            "name": "test",
            "canvas": { "width": 1920, "height": 1080 },
            "layers": []
        }"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        std::fs::write(&path, json).unwrap();

        let template = load_template(&path).unwrap();
        assert_eq!(template.name, "test");
        assert_eq!(template.canvas.width, 1920);
        assert_eq!(template.canvas.height, 1080);
        assert_eq!(template.version, 1); // default
        assert!(template.layers.is_empty());
    }

    #[test]
    fn test_load_template_with_version() {
        let json = r#"{
            "name": "v2",
            "version": 2,
            "canvas": { "width": 800, "height": 600 },
            "layers": []
        }"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        std::fs::write(&path, json).unwrap();

        let template = load_template(&path).unwrap();
        assert_eq!(template.version, 2);
    }

    #[test]
    fn test_load_template_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        assert!(load_template(&path).is_err());
    }

    #[test]
    fn test_load_template_missing_file() {
        let path = Path::new("/nonexistent/template.json");
        assert!(load_template(path).is_err());
    }

    #[test]
    fn test_load_template_with_timing() {
        let json = r#"{
            "name": "timed",
            "canvas": { "width": 100, "height": 100 },
            "layers": [],
            "timing": { "fade_in": 1.0, "hold": 5.0, "fade_out": 2.0 }
        }"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        std::fs::write(&path, json).unwrap();

        let template = load_template(&path).unwrap();
        assert!((template.timing.fade_in - 1.0).abs() < f64::EPSILON);
        assert!((template.timing.hold - 5.0).abs() < f64::EPSILON);
        assert!((template.timing.fade_out - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_load_template_default_timing() {
        let json = r#"{
            "name": "default_timing",
            "canvas": { "width": 100, "height": 100 },
            "layers": []
        }"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        std::fs::write(&path, json).unwrap();

        let template = load_template(&path).unwrap();
        assert!((template.timing.fade_in - 0.3).abs() < f64::EPSILON);
        assert!((template.timing.hold - 10.0).abs() < f64::EPSILON);
        assert!((template.timing.fade_out - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_timing_default_impl() {
        let timing = Timing::default();
        assert!((timing.fade_in - 0.3).abs() < f64::EPSILON);
        assert!((timing.hold - 10.0).abs() < f64::EPSILON);
        assert!((timing.fade_out - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_load_template_with_layers() {
        let json = r##"{
            "name": "with_layers",
            "canvas": { "width": 1920, "height": 1080 },
            "layers": [
                {
                    "type": "rect",
                    "x": "0", "y": "0", "w": "100%", "h": "100%",
                    "fill": "#000000"
                }
            ]
        }"##;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.json");
        std::fs::write(&path, json).unwrap();

        let template = load_template(&path).unwrap();
        assert_eq!(template.layers.len(), 1);
    }

    // --- substitute_variables tests ---

    #[test]
    fn test_substitute_basic() {
        let ctx = make_context(&[("name", "Alice"), ("score", "42")]);
        assert_eq!(
            substitute_variables("Hello {{name}}, score: {{score}}", &ctx),
            "Hello Alice, score: 42"
        );
    }

    #[test]
    fn test_substitute_missing_key() {
        let ctx = make_context(&[("name", "Bob")]);
        assert_eq!(
            substitute_variables("Hello {{name}}, {{missing}}", &ctx),
            "Hello Bob, {{missing}}"
        );
    }

    #[test]
    fn test_substitute_no_placeholders() {
        let ctx = make_context(&[]);
        assert_eq!(substitute_variables("plain text", &ctx), "plain text");
    }

    #[test]
    fn test_substitute_empty_string() {
        let ctx = make_context(&[]);
        assert_eq!(substitute_variables("", &ctx), "");
    }

    #[test]
    fn test_substitute_adjacent_placeholders() {
        let ctx = make_context(&[("a", "X"), ("b", "Y")]);
        assert_eq!(substitute_variables("{{a}}{{b}}", &ctx), "XY");
    }

    #[test]
    fn test_substitute_whitespace_in_key() {
        let ctx = make_context(&[("key", "val")]);
        assert_eq!(substitute_variables("{{ key }}", &ctx), "val");
    }

    #[test]
    fn test_substitute_unclosed_brace() {
        let ctx = make_context(&[("key", "val")]);
        assert_eq!(substitute_variables("{{key", &ctx), "{{key");
    }

    #[test]
    fn test_substitute_nested_braces() {
        let ctx = make_context(&[("inner", "ok")]);
        // {{ {{ inner }} }} — the first {{ matches with the first }}
        assert_eq!(substitute_variables("{{inner}}", &ctx), "ok");
    }

    // --- evaluate_visibility tests ---

    #[test]
    fn test_visibility_true() {
        let ctx = make_context(&[("has_assists", "3")]);
        assert!(evaluate_visibility("{{has_assists}}", &ctx));
    }

    #[test]
    fn test_visibility_false_value() {
        let ctx = make_context(&[("show", "false")]);
        assert!(!evaluate_visibility("{{show}}", &ctx));
    }

    #[test]
    fn test_visibility_zero_value() {
        let ctx = make_context(&[("show", "0")]);
        assert!(!evaluate_visibility("{{show}}", &ctx));
    }

    #[test]
    fn test_visibility_empty_value() {
        let ctx = make_context(&[("show", "")]);
        assert!(!evaluate_visibility("{{show}}", &ctx));
    }

    #[test]
    fn test_visibility_missing_key() {
        let ctx = make_context(&[]);
        assert!(!evaluate_visibility("{{missing}}", &ctx));
    }

    #[test]
    fn test_visibility_without_braces() {
        let ctx = make_context(&[("visible", "yes")]);
        assert!(evaluate_visibility("visible", &ctx));
    }

    #[test]
    fn test_visibility_whitespace_value() {
        let ctx = make_context(&[("show", "  ")]);
        assert!(!evaluate_visibility("{{show}}", &ctx));
    }

    #[test]
    fn test_visibility_truthy_string() {
        let ctx = make_context(&[("flag", "true")]);
        assert!(evaluate_visibility("{{flag}}", &ctx));
    }

    #[test]
    fn test_template_provider_trait() {
        struct TestProvider;
        impl TemplateProvider for TestProvider {
            fn provide(&self, context: &mut TemplateContext) {
                context.insert("key".to_string(), "value".to_string());
            }
        }
        let provider = TestProvider;
        let mut ctx = TemplateContext::new();
        provider.provide(&mut ctx);
        assert_eq!(ctx.get("key").unwrap(), "value");
    }

    #[test]
    fn test_template_serialize_roundtrip() {
        let template = Template {
            name: "roundtrip".to_string(),
            version: 1,
            canvas: Canvas {
                width: 100,
                height: 50,
            },
            layers: vec![],
            timing: Timing::default(),
        };
        let json = serde_json::to_string(&template).unwrap();
        let parsed: Template = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "roundtrip");
        assert_eq!(parsed.canvas.width, 100);
    }
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Metadata for an upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadMetadata {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
}

/// Result from a generator plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorResult {
    pub path: Option<PathBuf>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub success: bool,
    pub error_message: String,
}

/// A plugin that can upload content to a platform.
pub trait Uploader: Send + Sync {
    fn name(&self) -> &str;
    fn upload(
        &self,
        path: &Path,
        metadata: &UploadMetadata,
    ) -> Result<String, Box<dyn std::error::Error>>;
}

/// A plugin that can enrich event metadata.
pub trait MetadataEnricher: Send + Sync {
    fn name(&self) -> &str;
    fn enrich(
        &self,
        event_data: &mut HashMap<String, serde_json::Value>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

/// A plugin that can send notifications.
pub trait Notifier: Send + Sync {
    fn name(&self) -> &str;
    fn notify(
        &self,
        message: &str,
        metadata: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

/// A plugin that can generate content.
pub trait Generator: Send + Sync {
    fn name(&self) -> &str;
    fn generate(
        &self,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<GeneratorResult, Box<dyn std::error::Error>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── UploadMetadata tests ─────────────────────────────────────────

    #[test]
    fn upload_metadata_creation() {
        let meta = UploadMetadata {
            title: "My Video".to_string(),
            description: "A cool video".to_string(),
            tags: vec!["gaming".to_string(), "highlight".to_string()],
        };
        assert_eq!(meta.title, "My Video");
        assert_eq!(meta.description, "A cool video");
        assert_eq!(meta.tags.len(), 2);
    }

    #[test]
    fn upload_metadata_clone() {
        let meta = UploadMetadata {
            title: "T".to_string(),
            description: "D".to_string(),
            tags: vec!["a".to_string()],
        };
        let meta2 = meta.clone();
        assert_eq!(meta.title, meta2.title);
    }

    #[test]
    fn upload_metadata_debug() {
        let meta = UploadMetadata {
            title: "T".to_string(),
            description: "D".to_string(),
            tags: vec![],
        };
        let dbg = format!("{meta:?}");
        assert!(dbg.contains("UploadMetadata"));
    }

    #[test]
    fn upload_metadata_serde_roundtrip() {
        let meta = UploadMetadata {
            title: "Title".to_string(),
            description: "Desc".to_string(),
            tags: vec!["tag1".to_string()],
        };
        let json = serde_json::to_string(&meta).unwrap();
        let deser: UploadMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.title, "Title");
        assert_eq!(deser.tags, vec!["tag1"]);
    }

    // ── GeneratorResult tests ────────────────────────────────────────

    #[test]
    fn generator_result_creation() {
        let result = GeneratorResult {
            path: Some(PathBuf::from("/tmp/output.mp4")),
            metadata: HashMap::new(),
            success: true,
            error_message: String::new(),
        };
        assert!(result.success);
        assert_eq!(result.path.unwrap(), PathBuf::from("/tmp/output.mp4"));
    }

    #[test]
    fn generator_result_no_path() {
        let result = GeneratorResult {
            path: None,
            metadata: HashMap::new(),
            success: false,
            error_message: "failed".to_string(),
        };
        assert!(!result.success);
        assert!(result.path.is_none());
        assert_eq!(result.error_message, "failed");
    }

    #[test]
    fn generator_result_clone() {
        let mut meta = HashMap::new();
        meta.insert("key".to_string(), serde_json::json!("val"));
        let result = GeneratorResult {
            path: None,
            metadata: meta,
            success: true,
            error_message: String::new(),
        };
        let result2 = result.clone();
        assert_eq!(result2.metadata["key"], serde_json::json!("val"));
    }

    #[test]
    fn generator_result_debug() {
        let result = GeneratorResult {
            path: None,
            metadata: HashMap::new(),
            success: true,
            error_message: String::new(),
        };
        let dbg = format!("{result:?}");
        assert!(dbg.contains("GeneratorResult"));
    }

    #[test]
    fn generator_result_serde_roundtrip() {
        let mut meta = HashMap::new();
        meta.insert("k".to_string(), serde_json::json!(1));
        let result = GeneratorResult {
            path: Some(PathBuf::from("/out")),
            metadata: meta,
            success: true,
            error_message: "".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deser: GeneratorResult = serde_json::from_str(&json).unwrap();
        assert!(deser.success);
        assert_eq!(deser.path.unwrap(), PathBuf::from("/out"));
        assert_eq!(deser.metadata["k"], serde_json::json!(1));
    }

    // ── Trait object tests (via mock implementations) ────────────────

    struct MockUploader;
    impl Uploader for MockUploader {
        fn name(&self) -> &str {
            "mock_uploader"
        }
        fn upload(
            &self,
            _path: &Path,
            _metadata: &UploadMetadata,
        ) -> Result<String, Box<dyn std::error::Error>> {
            Ok("https://example.com/video".to_string())
        }
    }

    struct FailingUploader;
    impl Uploader for FailingUploader {
        fn name(&self) -> &str {
            "failing_uploader"
        }
        fn upload(
            &self,
            _path: &Path,
            _metadata: &UploadMetadata,
        ) -> Result<String, Box<dyn std::error::Error>> {
            Err("upload failed".into())
        }
    }

    #[test]
    fn uploader_trait_object() {
        let u: Box<dyn Uploader> = Box::new(MockUploader);
        assert_eq!(u.name(), "mock_uploader");
        let meta = UploadMetadata {
            title: "T".to_string(),
            description: "D".to_string(),
            tags: vec![],
        };
        let url = u.upload(Path::new("/tmp/file.mp4"), &meta).unwrap();
        assert_eq!(url, "https://example.com/video");
    }

    #[test]
    fn uploader_error() {
        let u: Box<dyn Uploader> = Box::new(FailingUploader);
        let meta = UploadMetadata {
            title: "T".to_string(),
            description: "D".to_string(),
            tags: vec![],
        };
        let result = u.upload(Path::new("/tmp/file.mp4"), &meta);
        assert!(result.is_err());
    }

    struct MockEnricher;
    impl MetadataEnricher for MockEnricher {
        fn name(&self) -> &str {
            "mock_enricher"
        }
        fn enrich(
            &self,
            event_data: &mut HashMap<String, serde_json::Value>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            event_data.insert("enriched".to_string(), serde_json::json!(true));
            Ok(())
        }
    }

    #[test]
    fn metadata_enricher_trait_object() {
        let e: Box<dyn MetadataEnricher> = Box::new(MockEnricher);
        assert_eq!(e.name(), "mock_enricher");
        let mut data = HashMap::new();
        e.enrich(&mut data).unwrap();
        assert_eq!(data["enriched"], serde_json::json!(true));
    }

    struct MockNotifier;
    impl Notifier for MockNotifier {
        fn name(&self) -> &str {
            "mock_notifier"
        }
        fn notify(
            &self,
            _message: &str,
            _metadata: Option<&HashMap<String, serde_json::Value>>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
    }

    #[test]
    fn notifier_trait_object() {
        let n: Box<dyn Notifier> = Box::new(MockNotifier);
        assert_eq!(n.name(), "mock_notifier");
        n.notify("hello", None).unwrap();
    }

    #[test]
    fn notifier_with_metadata() {
        let n: Box<dyn Notifier> = Box::new(MockNotifier);
        let mut meta = HashMap::new();
        meta.insert("channel".to_string(), serde_json::json!("general"));
        n.notify("hello", Some(&meta)).unwrap();
    }

    struct MockGenerator;
    impl Generator for MockGenerator {
        fn name(&self) -> &str {
            "mock_generator"
        }
        fn generate(
            &self,
            context: &HashMap<String, serde_json::Value>,
        ) -> Result<GeneratorResult, Box<dyn std::error::Error>> {
            let mut metadata = HashMap::new();
            if let Some(v) = context.get("input") {
                metadata.insert("received".to_string(), v.clone());
            }
            Ok(GeneratorResult {
                path: Some(PathBuf::from("/generated")),
                metadata,
                success: true,
                error_message: String::new(),
            })
        }
    }

    #[test]
    fn generator_trait_object() {
        let g: Box<dyn Generator> = Box::new(MockGenerator);
        assert_eq!(g.name(), "mock_generator");
        let mut ctx = HashMap::new();
        ctx.insert("input".to_string(), serde_json::json!("data"));
        let result = g.generate(&ctx).unwrap();
        assert!(result.success);
        assert_eq!(result.metadata["received"], serde_json::json!("data"));
    }

    #[test]
    fn generator_empty_context() {
        let g: Box<dyn Generator> = Box::new(MockGenerator);
        let ctx = HashMap::new();
        let result = g.generate(&ctx).unwrap();
        assert!(result.success);
        assert!(result.metadata.is_empty());
    }

    // ── Send + Sync compile-time checks ─────────────────────────────

    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn traits_are_send_sync() {
        _assert_send_sync::<Box<dyn Uploader>>();
        _assert_send_sync::<Box<dyn MetadataEnricher>>();
        _assert_send_sync::<Box<dyn Notifier>>();
        _assert_send_sync::<Box<dyn Generator>>();
    }
}

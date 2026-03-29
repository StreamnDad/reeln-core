use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::hooks::{HookContext, HookHandler};

/// Describes a registered plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub entry_point: String,
    pub package: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
}

/// A field in a plugin's configuration schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    pub name: String,
    pub field_type: String,
    pub default: Option<serde_json::Value>,
    pub required: bool,
    pub description: String,
    pub secret: bool,
}

/// Schema describing a plugin's configurable fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfigSchema {
    pub fields: Vec<ConfigField>,
}

impl PluginConfigSchema {
    /// Returns a map of field name -> default value for fields that have defaults.
    #[must_use]
    pub fn defaults_dict(&self) -> HashMap<String, serde_json::Value> {
        self.fields
            .iter()
            .filter_map(|f| f.default.as_ref().map(|d| (f.name.clone(), d.clone())))
            .collect()
    }

    /// Returns names of required fields.
    #[must_use]
    pub fn required_fields(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name.as_str())
            .collect()
    }

    /// Find a field by name.
    #[must_use]
    pub fn field_by_name(&self, name: &str) -> Option<&ConfigField> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// Plugin registry — manages plugin discovery, activation, and hook dispatch.
///
/// Each plugin is stored alongside its handler. On `emit`, we iterate
/// plugins and dispatch to handlers whose subscribed hooks match.
pub struct PluginRegistry {
    plugins: Vec<(PluginInfo, Box<dyn HookHandler>)>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin with its info and hook handler.
    /// The handler is registered in the internal `HookRegistry` for each
    /// of its subscribed hooks.
    pub fn register_plugin(&mut self, info: PluginInfo, handler: Box<dyn HookHandler>) {
        let hooks = handler.subscribed_hooks();
        self.plugins.push((info, handler));

        // We need to rebuild the hook_registry since we can't split the
        // borrow. Instead, we dispatch manually in `emit`.
        // Just track which hooks have handlers.
        for hook in hooks {
            // Use a no-op registration to track that handlers exist.
            // Actually, let's skip the HookRegistry entirely and dispatch
            // directly from plugins vec. This is cleaner.
            let _ = hook;
        }
    }

    /// Get plugin info by name.
    #[must_use]
    pub fn get_plugin(&self, name: &str) -> Option<&PluginInfo> {
        self.plugins
            .iter()
            .find(|(info, _)| info.name == name)
            .map(|(info, _)| info)
    }

    /// List all registered plugins.
    #[must_use]
    pub fn list_plugins(&self) -> Vec<&PluginInfo> {
        self.plugins.iter().map(|(info, _)| info).collect()
    }

    /// Emit a hook context. Dispatches to all plugin handlers that subscribe
    /// to the context's hook. Panics are caught per-handler.
    pub fn emit(&self, context: &mut HookContext) {
        for (info, handler) in &self.plugins {
            if handler.subscribed_hooks().contains(&context.hook) {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler.on_hook(context);
                }));
                if let Err(e) = result {
                    let msg = if let Some(s) = e.downcast_ref::<&str>() {
                        (*s).to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    log::error!(
                        "Plugin '{}' handler panicked on {:?}: {}",
                        info.name,
                        context.hook,
                        msg
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::Hook;
    use std::sync::{Arc, Mutex};

    // ── PluginInfo tests ─────────────────────────────────────────────

    #[test]
    fn plugin_info_creation() {
        let info = PluginInfo {
            name: "test-plugin".to_string(),
            entry_point: "main".to_string(),
            package: "test-pkg".to_string(),
            capabilities: vec!["uploader".to_string()],
            enabled: true,
        };
        assert_eq!(info.name, "test-plugin");
        assert!(info.enabled);
    }

    #[test]
    fn plugin_info_clone() {
        let info = PluginInfo {
            name: "p".to_string(),
            entry_point: "e".to_string(),
            package: "pkg".to_string(),
            capabilities: vec![],
            enabled: false,
        };
        let info2 = info.clone();
        assert_eq!(info.name, info2.name);
        assert_eq!(info.enabled, info2.enabled);
    }

    #[test]
    fn plugin_info_debug() {
        let info = PluginInfo {
            name: "p".to_string(),
            entry_point: "e".to_string(),
            package: "pkg".to_string(),
            capabilities: vec![],
            enabled: true,
        };
        let dbg = format!("{info:?}");
        assert!(dbg.contains("PluginInfo"));
    }

    #[test]
    fn plugin_info_serde_roundtrip() {
        let info = PluginInfo {
            name: "test".to_string(),
            entry_point: "main".to_string(),
            package: "pkg".to_string(),
            capabilities: vec!["notifier".to_string()],
            enabled: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deser: PluginInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.name, "test");
        assert_eq!(deser.capabilities, vec!["notifier"]);
    }

    // ── ConfigField tests ────────────────────────────────────────────

    #[test]
    fn config_field_creation() {
        let field = ConfigField {
            name: "api_key".to_string(),
            field_type: "str".to_string(),
            default: None,
            required: true,
            description: "API key".to_string(),
            secret: true,
        };
        assert_eq!(field.name, "api_key");
        assert!(field.required);
        assert!(field.secret);
        assert!(field.default.is_none());
    }

    #[test]
    fn config_field_with_default() {
        let field = ConfigField {
            name: "timeout".to_string(),
            field_type: "int".to_string(),
            default: Some(serde_json::json!(30)),
            required: false,
            description: "Timeout in seconds".to_string(),
            secret: false,
        };
        assert_eq!(field.default, Some(serde_json::json!(30)));
    }

    #[test]
    fn config_field_clone() {
        let field = ConfigField {
            name: "f".to_string(),
            field_type: "bool".to_string(),
            default: Some(serde_json::json!(true)),
            required: false,
            description: "d".to_string(),
            secret: false,
        };
        let field2 = field.clone();
        assert_eq!(field.name, field2.name);
    }

    #[test]
    fn config_field_debug() {
        let field = ConfigField {
            name: "f".to_string(),
            field_type: "str".to_string(),
            default: None,
            required: true,
            description: "d".to_string(),
            secret: false,
        };
        let dbg = format!("{field:?}");
        assert!(dbg.contains("ConfigField"));
    }

    #[test]
    fn config_field_serde_roundtrip() {
        let field = ConfigField {
            name: "port".to_string(),
            field_type: "int".to_string(),
            default: Some(serde_json::json!(8080)),
            required: false,
            description: "Port number".to_string(),
            secret: false,
        };
        let json = serde_json::to_string(&field).unwrap();
        let deser: ConfigField = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.name, "port");
        assert_eq!(deser.default, Some(serde_json::json!(8080)));
    }

    // ── PluginConfigSchema tests ─────────────────────────────────────

    #[test]
    fn schema_default_is_empty() {
        let schema = PluginConfigSchema::default();
        assert!(schema.fields.is_empty());
        assert!(schema.defaults_dict().is_empty());
        assert!(schema.required_fields().is_empty());
    }

    #[test]
    fn schema_defaults_dict() {
        let schema = PluginConfigSchema {
            fields: vec![
                ConfigField {
                    name: "a".to_string(),
                    field_type: "str".to_string(),
                    default: Some(serde_json::json!("hello")),
                    required: false,
                    description: "".to_string(),
                    secret: false,
                },
                ConfigField {
                    name: "b".to_string(),
                    field_type: "int".to_string(),
                    default: None,
                    required: true,
                    description: "".to_string(),
                    secret: false,
                },
                ConfigField {
                    name: "c".to_string(),
                    field_type: "float".to_string(),
                    default: Some(serde_json::json!(1.23)),
                    required: false,
                    description: "".to_string(),
                    secret: false,
                },
            ],
        };
        let defaults = schema.defaults_dict();
        assert_eq!(defaults.len(), 2);
        assert_eq!(defaults["a"], serde_json::json!("hello"));
        assert_eq!(defaults["c"], serde_json::json!(1.23));
        assert!(!defaults.contains_key("b"));
    }

    #[test]
    fn schema_required_fields() {
        let schema = PluginConfigSchema {
            fields: vec![
                ConfigField {
                    name: "required_field".to_string(),
                    field_type: "str".to_string(),
                    default: None,
                    required: true,
                    description: "".to_string(),
                    secret: false,
                },
                ConfigField {
                    name: "optional_field".to_string(),
                    field_type: "str".to_string(),
                    default: Some(serde_json::json!("")),
                    required: false,
                    description: "".to_string(),
                    secret: false,
                },
            ],
        };
        let required = schema.required_fields();
        assert_eq!(required, vec!["required_field"]);
    }

    #[test]
    fn schema_field_by_name_found() {
        let schema = PluginConfigSchema {
            fields: vec![ConfigField {
                name: "api_key".to_string(),
                field_type: "str".to_string(),
                default: None,
                required: true,
                description: "The API key".to_string(),
                secret: true,
            }],
        };
        let field = schema.field_by_name("api_key").unwrap();
        assert_eq!(field.field_type, "str");
        assert!(field.secret);
    }

    #[test]
    fn schema_field_by_name_not_found() {
        let schema = PluginConfigSchema::default();
        assert!(schema.field_by_name("nonexistent").is_none());
    }

    #[test]
    fn schema_clone() {
        let schema = PluginConfigSchema {
            fields: vec![ConfigField {
                name: "x".to_string(),
                field_type: "bool".to_string(),
                default: None,
                required: false,
                description: "".to_string(),
                secret: false,
            }],
        };
        let schema2 = schema.clone();
        assert_eq!(schema2.fields.len(), 1);
    }

    #[test]
    fn schema_debug() {
        let schema = PluginConfigSchema::default();
        let dbg = format!("{schema:?}");
        assert!(dbg.contains("PluginConfigSchema"));
    }

    #[test]
    fn schema_serde_roundtrip() {
        let schema = PluginConfigSchema {
            fields: vec![ConfigField {
                name: "host".to_string(),
                field_type: "str".to_string(),
                default: Some(serde_json::json!("localhost")),
                required: false,
                description: "Hostname".to_string(),
                secret: false,
            }],
        };
        let json = serde_json::to_string(&schema).unwrap();
        let deser: PluginConfigSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.fields.len(), 1);
        assert_eq!(deser.fields[0].name, "host");
    }

    // ── PluginRegistry tests ─────────────────────────────────────────

    struct TestHandler {
        hooks: Vec<Hook>,
        call_log: Arc<Mutex<Vec<Hook>>>,
    }

    impl HookHandler for TestHandler {
        fn on_hook(&self, context: &mut HookContext) {
            self.call_log.lock().unwrap().push(context.hook);
        }
        fn subscribed_hooks(&self) -> Vec<Hook> {
            self.hooks.clone()
        }
    }

    struct PanickingPluginHandler;
    impl HookHandler for PanickingPluginHandler {
        fn on_hook(&self, _context: &mut HookContext) {
            panic!("plugin handler exploded");
        }
        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnError]
        }
    }

    struct PanickingStringPluginHandler;
    impl HookHandler for PanickingStringPluginHandler {
        fn on_hook(&self, _context: &mut HookContext) {
            std::panic::panic_any(String::from("owned string plugin panic"));
        }
        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnError]
        }
    }

    struct PanickingNonStringPluginHandler;
    impl HookHandler for PanickingNonStringPluginHandler {
        fn on_hook(&self, _context: &mut HookContext) {
            std::panic::panic_any(42_i32);
        }
        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnError]
        }
    }

    fn make_info(name: &str) -> PluginInfo {
        PluginInfo {
            name: name.to_string(),
            entry_point: "main".to_string(),
            package: "pkg".to_string(),
            capabilities: vec![],
            enabled: true,
        }
    }

    #[test]
    fn plugin_registry_new_is_empty() {
        let reg = PluginRegistry::new();
        assert!(reg.list_plugins().is_empty());
        assert!(reg.get_plugin("anything").is_none());
    }

    #[test]
    fn plugin_registry_default() {
        let reg = PluginRegistry::default();
        assert!(reg.list_plugins().is_empty());
    }

    #[test]
    fn plugin_registry_register_and_get() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = PluginRegistry::new();
        reg.register_plugin(
            make_info("my-plugin"),
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log),
            }),
        );
        assert!(reg.get_plugin("my-plugin").is_some());
        assert_eq!(reg.get_plugin("my-plugin").unwrap().name, "my-plugin");
        assert!(reg.get_plugin("other").is_none());
    }

    #[test]
    fn plugin_registry_list_plugins() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = PluginRegistry::new();
        reg.register_plugin(
            make_info("plugin-a"),
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log),
            }),
        );
        reg.register_plugin(
            make_info("plugin-b"),
            Box::new(TestHandler {
                hooks: vec![Hook::OnError],
                call_log: Arc::clone(&log),
            }),
        );
        let plugins = reg.list_plugins();
        assert_eq!(plugins.len(), 2);
        let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"plugin-a"));
        assert!(names.contains(&"plugin-b"));
    }

    #[test]
    fn plugin_registry_emit_dispatches() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = PluginRegistry::new();
        reg.register_plugin(
            make_info("p1"),
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit, Hook::OnError],
                call_log: Arc::clone(&log),
            }),
        );
        reg.register_plugin(
            make_info("p2"),
            Box::new(TestHandler {
                hooks: vec![Hook::OnError],
                call_log: Arc::clone(&log),
            }),
        );

        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 1);

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 3); // 1 + 2

        // Hook with no subscribers
        let mut ctx = HookContext::new(Hook::PreRender);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 3); // unchanged
    }

    #[test]
    fn plugin_registry_emit_catches_panic() {
        let mut reg = PluginRegistry::new();
        reg.register_plugin(make_info("bad-plugin"), Box::new(PanickingPluginHandler));

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx); // must not panic
    }

    #[test]
    fn plugin_registry_emit_catches_string_panic() {
        let mut reg = PluginRegistry::new();
        reg.register_plugin(
            make_info("string-panic"),
            Box::new(PanickingStringPluginHandler),
        );

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx); // must not panic
    }

    #[test]
    fn plugin_registry_emit_catches_non_string_panic() {
        let mut reg = PluginRegistry::new();
        reg.register_plugin(
            make_info("non-string-panic"),
            Box::new(PanickingNonStringPluginHandler),
        );

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx); // must not panic
    }

    #[test]
    fn plugin_registry_emit_panic_does_not_skip_others() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = PluginRegistry::new();
        reg.register_plugin(make_info("bad"), Box::new(PanickingPluginHandler));
        reg.register_plugin(
            make_info("good"),
            Box::new(TestHandler {
                hooks: vec![Hook::OnError],
                call_log: Arc::clone(&log),
            }),
        );

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 1);
    }
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Lifecycle hooks that plugins can subscribe to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hook {
    #[serde(rename = "pre_render")]
    PreRender,
    #[serde(rename = "post_render")]
    PostRender,
    #[serde(rename = "on_clip_available")]
    OnClipAvailable,
    #[serde(rename = "on_event_created")]
    OnEventCreated,
    #[serde(rename = "on_event_tagged")]
    OnEventTagged,
    #[serde(rename = "on_game_init")]
    OnGameInit,
    #[serde(rename = "on_game_ready")]
    OnGameReady,
    #[serde(rename = "on_game_finish")]
    OnGameFinish,
    #[serde(rename = "on_post_game_finish")]
    OnPostGameFinish,
    #[serde(rename = "on_highlights_merged")]
    OnHighlightsMerged,
    #[serde(rename = "on_segment_start")]
    OnSegmentStart,
    #[serde(rename = "on_segment_complete")]
    OnSegmentComplete,
    #[serde(rename = "on_frames_extracted")]
    OnFramesExtracted,
    #[serde(rename = "on_error")]
    OnError,
}

impl Hook {
    /// Returns the string representation used for serialization.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreRender => "pre_render",
            Self::PostRender => "post_render",
            Self::OnClipAvailable => "on_clip_available",
            Self::OnEventCreated => "on_event_created",
            Self::OnEventTagged => "on_event_tagged",
            Self::OnGameInit => "on_game_init",
            Self::OnGameReady => "on_game_ready",
            Self::OnGameFinish => "on_game_finish",
            Self::OnPostGameFinish => "on_post_game_finish",
            Self::OnHighlightsMerged => "on_highlights_merged",
            Self::OnSegmentStart => "on_segment_start",
            Self::OnSegmentComplete => "on_segment_complete",
            Self::OnFramesExtracted => "on_frames_extracted",
            Self::OnError => "on_error",
        }
    }

    /// Returns all hook variants.
    #[must_use]
    pub fn all() -> &'static [Hook] {
        &[
            Self::PreRender,
            Self::PostRender,
            Self::OnClipAvailable,
            Self::OnEventCreated,
            Self::OnEventTagged,
            Self::OnGameInit,
            Self::OnGameReady,
            Self::OnGameFinish,
            Self::OnPostGameFinish,
            Self::OnHighlightsMerged,
            Self::OnSegmentStart,
            Self::OnSegmentComplete,
            Self::OnFramesExtracted,
            Self::OnError,
        ]
    }
}

impl std::fmt::Display for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Context passed to hook handlers.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// The hook being emitted.
    pub hook: Hook,
    /// Hook-specific data.
    pub data: HashMap<String, serde_json::Value>,
    /// Shared mutable state between handlers.
    pub shared: HashMap<String, serde_json::Value>,
}

impl HookContext {
    /// Create a new `HookContext` with the given hook and empty maps.
    #[must_use]
    pub fn new(hook: Hook) -> Self {
        Self {
            hook,
            data: HashMap::new(),
            shared: HashMap::new(),
        }
    }

    /// Create a new `HookContext` with hook and data.
    #[must_use]
    pub fn with_data(hook: Hook, data: HashMap<String, serde_json::Value>) -> Self {
        Self {
            hook,
            data,
            shared: HashMap::new(),
        }
    }
}

/// Trait that plugins implement to handle hooks.
pub trait HookHandler: Send + Sync {
    /// Called when a subscribed hook is emitted. Context is mutable so
    /// handlers can write to `shared`.
    fn on_hook(&self, context: &mut HookContext);

    /// Returns the hooks this handler wants to receive.
    fn subscribed_hooks(&self) -> Vec<Hook>;
}

/// Registry of hook handlers with panic-safe dispatch.
#[derive(Default)]
pub struct HookRegistry {
    handlers: HashMap<Hook, Vec<Box<dyn HookHandler>>>,
}

impl HookRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for a specific hook.
    pub fn register(&mut self, hook: Hook, handler: Box<dyn HookHandler>) {
        self.handlers.entry(hook).or_default().push(handler);
    }

    /// Emit a hook context to all registered handlers. Panics in handlers
    /// are caught so they never crash core operations.
    pub fn emit(&self, context: &mut HookContext) {
        if let Some(handlers) = self.handlers.get(&context.hook) {
            for handler in handlers {
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
                    log::error!("Hook handler panicked on {:?}: {}", context.hook, msg);
                }
            }
        }
    }

    /// Returns `true` if at least one handler is registered for the given hook.
    #[must_use]
    pub fn has_handlers(&self, hook: Hook) -> bool {
        self.handlers.get(&hook).is_some_and(|h| !h.is_empty())
    }

    /// Remove all handlers.
    pub fn clear(&mut self) {
        self.handlers.clear();
    }

    /// Total number of handler registrations across all hooks.
    #[must_use]
    pub fn handler_count(&self) -> usize {
        self.handlers.values().map(Vec::len).sum()
    }
}

/// A registry wrapper that only allows registration for a declared set of hooks.
pub struct FilteredRegistry {
    backing: HookRegistry,
    allowed: std::collections::HashSet<Hook>,
}

impl FilteredRegistry {
    /// Create a new `FilteredRegistry` that only permits the given hooks.
    #[must_use]
    pub fn new(allowed_hooks: &[Hook]) -> Self {
        Self {
            backing: HookRegistry::new(),
            allowed: allowed_hooks.iter().copied().collect(),
        }
    }

    /// Register a handler. If the hook is not in the allowed set, the call
    /// is silently ignored (with a warning log).
    pub fn register(&mut self, hook: Hook, handler: Box<dyn HookHandler>) {
        if self.allowed.contains(&hook) {
            self.backing.register(hook, handler);
        } else {
            log::warn!(
                "FilteredRegistry: ignoring registration for undeclared hook {:?}",
                hook
            );
        }
    }

    /// Emit delegates to the backing registry.
    pub fn emit(&self, context: &mut HookContext) {
        self.backing.emit(context);
    }

    /// Check if the backing registry has handlers for the given hook.
    #[must_use]
    pub fn has_handlers(&self, hook: Hook) -> bool {
        self.backing.has_handlers(hook)
    }

    /// Total handler count in the backing registry.
    #[must_use]
    pub fn handler_count(&self) -> usize {
        self.backing.handler_count()
    }

    /// Clear all handlers in the backing registry.
    pub fn clear(&mut self) {
        self.backing.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // ── Hook enum tests ──────────────────────────────────────────────

    #[test]
    fn hook_as_str_returns_correct_values() {
        assert_eq!(Hook::PreRender.as_str(), "pre_render");
        assert_eq!(Hook::PostRender.as_str(), "post_render");
        assert_eq!(Hook::OnClipAvailable.as_str(), "on_clip_available");
        assert_eq!(Hook::OnEventCreated.as_str(), "on_event_created");
        assert_eq!(Hook::OnEventTagged.as_str(), "on_event_tagged");
        assert_eq!(Hook::OnGameInit.as_str(), "on_game_init");
        assert_eq!(Hook::OnGameReady.as_str(), "on_game_ready");
        assert_eq!(Hook::OnGameFinish.as_str(), "on_game_finish");
        assert_eq!(Hook::OnPostGameFinish.as_str(), "on_post_game_finish");
        assert_eq!(Hook::OnHighlightsMerged.as_str(), "on_highlights_merged");
        assert_eq!(Hook::OnSegmentStart.as_str(), "on_segment_start");
        assert_eq!(Hook::OnSegmentComplete.as_str(), "on_segment_complete");
        assert_eq!(Hook::OnFramesExtracted.as_str(), "on_frames_extracted");
        assert_eq!(Hook::OnError.as_str(), "on_error");
    }

    #[test]
    fn hook_display_matches_as_str() {
        for hook in Hook::all() {
            assert_eq!(format!("{hook}"), hook.as_str());
        }
    }

    #[test]
    fn hook_all_returns_14_variants() {
        assert_eq!(Hook::all().len(), 14);
    }

    #[test]
    fn hook_serde_roundtrip() {
        for hook in Hook::all() {
            let json = serde_json::to_string(hook).unwrap();
            let deserialized: Hook = serde_json::from_str(&json).unwrap();
            assert_eq!(*hook, deserialized);
        }
    }

    #[test]
    fn hook_serde_string_values() {
        let json = serde_json::to_string(&Hook::OnGameInit).unwrap();
        assert_eq!(json, "\"on_game_init\"");

        let json = serde_json::to_string(&Hook::PreRender).unwrap();
        assert_eq!(json, "\"pre_render\"");
    }

    #[test]
    fn hook_clone_and_copy() {
        let h = Hook::OnError;
        let h2 = h;
        #[allow(clippy::clone_on_copy)]
        let h3 = h.clone();
        assert_eq!(h, h2);
        assert_eq!(h, h3);
    }

    #[test]
    fn hook_debug() {
        let dbg = format!("{:?}", Hook::OnGameInit);
        assert_eq!(dbg, "OnGameInit");
    }

    #[test]
    fn hook_eq_and_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Hook::OnGameInit);
        set.insert(Hook::OnGameInit);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&Hook::OnGameInit));
        assert!(!set.contains(&Hook::OnError));
    }

    // ── HookContext tests ────────────────────────────────────────────

    #[test]
    fn hook_context_new_has_empty_maps() {
        let ctx = HookContext::new(Hook::OnGameInit);
        assert_eq!(ctx.hook, Hook::OnGameInit);
        assert!(ctx.data.is_empty());
        assert!(ctx.shared.is_empty());
    }

    #[test]
    fn hook_context_with_data() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_json::json!("value"));
        let ctx = HookContext::with_data(Hook::OnError, data);
        assert_eq!(ctx.hook, Hook::OnError);
        assert_eq!(ctx.data["key"], serde_json::json!("value"));
        assert!(ctx.shared.is_empty());
    }

    #[test]
    fn hook_context_clone() {
        let mut ctx = HookContext::new(Hook::PreRender);
        ctx.shared.insert("x".to_string(), serde_json::json!(42));
        let ctx2 = ctx.clone();
        assert_eq!(ctx2.shared["x"], serde_json::json!(42));
    }

    #[test]
    fn hook_context_debug() {
        let ctx = HookContext::new(Hook::PreRender);
        let dbg = format!("{ctx:?}");
        assert!(dbg.contains("PreRender"));
    }

    // ── Test handler helper ──────────────────────────────────────────

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

    struct SharedWriter {
        key: String,
        value: serde_json::Value,
    }

    impl HookHandler for SharedWriter {
        fn on_hook(&self, context: &mut HookContext) {
            context.shared.insert(self.key.clone(), self.value.clone());
        }

        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnGameInit]
        }
    }

    struct PanickingHandler;

    impl HookHandler for PanickingHandler {
        fn on_hook(&self, _context: &mut HookContext) {
            panic!("handler exploded");
        }

        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnError]
        }
    }

    struct PanickingStringHandler;

    impl HookHandler for PanickingStringHandler {
        fn on_hook(&self, _context: &mut HookContext) {
            std::panic::panic_any(String::from("owned string panic"));
        }

        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnError]
        }
    }

    struct PanickingNonStringHandler;

    impl HookHandler for PanickingNonStringHandler {
        fn on_hook(&self, _context: &mut HookContext) {
            std::panic::panic_any(42_i32);
        }

        fn subscribed_hooks(&self) -> Vec<Hook> {
            vec![Hook::OnError]
        }
    }

    // ── HookRegistry tests ──────────────────────────────────────────

    #[test]
    fn hook_registry_new_is_empty() {
        let reg = HookRegistry::new();
        assert_eq!(reg.handler_count(), 0);
        assert!(!reg.has_handlers(Hook::OnGameInit));
    }

    #[test]
    fn hook_registry_default_is_empty() {
        let reg = HookRegistry::default();
        assert_eq!(reg.handler_count(), 0);
    }

    #[test]
    fn hook_registry_register_and_emit() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = HookRegistry::new();
        reg.register(
            Hook::OnGameInit,
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log),
            }),
        );

        assert!(reg.has_handlers(Hook::OnGameInit));
        assert!(!reg.has_handlers(Hook::OnError));
        assert_eq!(reg.handler_count(), 1);

        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 1);
        assert_eq!(log.lock().unwrap()[0], Hook::OnGameInit);
    }

    #[test]
    fn hook_registry_emit_no_handlers() {
        let reg = HookRegistry::new();
        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx); // should not panic
    }

    #[test]
    fn hook_registry_multiple_handlers_same_hook() {
        let log1 = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::new(Mutex::new(Vec::new()));
        let mut reg = HookRegistry::new();
        reg.register(
            Hook::OnGameInit,
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log1),
            }),
        );
        reg.register(
            Hook::OnGameInit,
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log2),
            }),
        );
        assert_eq!(reg.handler_count(), 2);

        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(log1.lock().unwrap().len(), 1);
        assert_eq!(log2.lock().unwrap().len(), 1);
    }

    #[test]
    fn hook_registry_handler_writes_to_shared() {
        let mut reg = HookRegistry::new();
        reg.register(
            Hook::OnGameInit,
            Box::new(SharedWriter {
                key: "result".to_string(),
                value: serde_json::json!(true),
            }),
        );

        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(ctx.shared["result"], serde_json::json!(true));
    }

    #[test]
    fn hook_registry_catches_panic_str() {
        let mut reg = HookRegistry::new();
        reg.register(Hook::OnError, Box::new(PanickingHandler));

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx); // must not panic
    }

    #[test]
    fn hook_registry_catches_panic_string() {
        let mut reg = HookRegistry::new();
        reg.register(Hook::OnError, Box::new(PanickingStringHandler));

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx); // must not panic
    }

    #[test]
    fn hook_registry_catches_panic_non_string() {
        let mut reg = HookRegistry::new();
        reg.register(Hook::OnError, Box::new(PanickingNonStringHandler));

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx); // must not panic
    }

    #[test]
    fn hook_registry_panic_does_not_skip_subsequent_handlers() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = HookRegistry::new();
        reg.register(Hook::OnError, Box::new(PanickingHandler));
        reg.register(
            Hook::OnError,
            Box::new(TestHandler {
                hooks: vec![Hook::OnError],
                call_log: Arc::clone(&log),
            }),
        );

        let mut ctx = HookContext::new(Hook::OnError);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[test]
    fn hook_registry_clear() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = HookRegistry::new();
        reg.register(
            Hook::OnGameInit,
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log),
            }),
        );
        assert_eq!(reg.handler_count(), 1);
        reg.clear();
        assert_eq!(reg.handler_count(), 0);
        assert!(!reg.has_handlers(Hook::OnGameInit));
    }

    #[test]
    fn hook_registry_has_handlers_false_for_empty_vec() {
        let mut reg = HookRegistry::new();
        // Manually ensure an empty vec entry doesn't count
        reg.handlers.insert(Hook::OnGameInit, vec![]);
        assert!(!reg.has_handlers(Hook::OnGameInit));
    }

    // ── FilteredRegistry tests ──────────────────────────────────────

    #[test]
    fn filtered_registry_allows_declared_hooks() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = FilteredRegistry::new(&[Hook::OnGameInit]);
        reg.register(
            Hook::OnGameInit,
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log),
            }),
        );
        assert!(reg.has_handlers(Hook::OnGameInit));
        assert_eq!(reg.handler_count(), 1);

        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[test]
    fn filtered_registry_ignores_undeclared_hooks() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = FilteredRegistry::new(&[Hook::OnGameInit]);
        reg.register(
            Hook::OnError,
            Box::new(TestHandler {
                hooks: vec![Hook::OnError],
                call_log: Arc::clone(&log),
            }),
        );
        assert!(!reg.has_handlers(Hook::OnError));
        assert_eq!(reg.handler_count(), 0);
    }

    #[test]
    fn filtered_registry_clear() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut reg = FilteredRegistry::new(&[Hook::OnGameInit]);
        reg.register(
            Hook::OnGameInit,
            Box::new(TestHandler {
                hooks: vec![Hook::OnGameInit],
                call_log: Arc::clone(&log),
            }),
        );
        reg.clear();
        assert_eq!(reg.handler_count(), 0);
    }

    #[test]
    fn filtered_registry_emit_no_handlers() {
        let reg = FilteredRegistry::new(&[Hook::OnGameInit]);
        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx); // should not panic
    }
}

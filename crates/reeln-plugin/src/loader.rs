//! Dynamic plugin loading via `libloading`.
//!
//! Native Rust plugins expose a C ABI entry point:
//!
//! ```c
//! extern "C" ReelnPluginDescriptor reeln_plugin_init(void);
//! ```
//!
//! The returned descriptor provides the plugin name, version, subscribed hooks,
//! and a callback invoked when a hook fires.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;

use crate::hooks::{Hook, HookContext, HookHandler};
use crate::registry::PluginInfo;

/// Error type for plugin loading failures.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The shared library could not be loaded.
    #[error("failed to load library: {0}")]
    Library(String),

    /// The entry point symbol was not found.
    #[error("entry point not found: {0}")]
    Symbol(String),

    /// The plugin returned invalid data (e.g. null pointers).
    #[error("invalid plugin descriptor: {0}")]
    InvalidDescriptor(String),

    /// The library path does not exist.
    #[error("library not found: {0}")]
    NotFound(String),
}

/// C-compatible hook callback signature.
///
/// The callback receives:
/// - `hook_name`: null-terminated hook name string (e.g. "on_game_init")
/// - `data_json`: null-terminated JSON string with hook-specific data
/// - `shared_json`: null-terminated JSON string with shared mutable state
/// - `shared_out`: pointer to a buffer where the plugin writes updated shared JSON
/// - `shared_out_len`: size of the `shared_out` buffer
///
/// Returns the number of bytes written to `shared_out` (0 if no changes).
pub type HookCallbackFn = unsafe extern "C" fn(
    hook_name: *const c_char,
    data_json: *const c_char,
    shared_json: *const c_char,
    shared_out: *mut c_char,
    shared_out_len: usize,
) -> usize;

/// C ABI plugin descriptor returned by the entry point.
///
/// All string pointers must be valid, null-terminated, and remain valid for
/// the lifetime of the loaded library.
#[repr(C)]
pub struct PluginDescriptor {
    /// Plugin name (null-terminated UTF-8).
    pub name: *const c_char,
    /// Plugin version string (null-terminated UTF-8).
    pub version: *const c_char,
    /// Comma-separated list of subscribed hook names (null-terminated UTF-8).
    /// e.g. "on_game_init,on_game_finish"
    pub subscribed_hooks: *const c_char,
    /// Hook callback function pointer.
    pub on_hook: HookCallbackFn,
}

/// Type of the plugin entry point function.
pub type PluginInitFn = unsafe extern "C" fn() -> PluginDescriptor;

/// The default symbol name for the plugin entry point.
pub const ENTRY_POINT_SYMBOL: &[u8] = b"reeln_plugin_init\0";

/// A loaded native plugin. Holds the library handle to keep it alive.
pub struct LoadedPlugin {
    /// The loaded library handle. Must remain alive for callbacks to work.
    _library: libloading::Library,
    /// Plugin name extracted from the descriptor.
    name: String,
    /// Plugin version extracted from the descriptor.
    version: String,
    /// Hooks this plugin subscribes to.
    hooks: Vec<Hook>,
    /// The C callback function.
    callback: HookCallbackFn,
}

impl LoadedPlugin {
    /// Returns the plugin name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the plugin version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns a `PluginInfo` suitable for the registry.
    #[must_use]
    pub fn info(&self) -> PluginInfo {
        PluginInfo {
            name: self.name.clone(),
            entry_point: "native".to_string(),
            package: String::new(),
            capabilities: self
                .hooks
                .iter()
                .map(|h| format!("hook:{}", h.as_str()))
                .collect(),
            enabled: true,
        }
    }
}

impl std::fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("hooks", &self.hooks)
            .finish_non_exhaustive()
    }
}

impl HookHandler for LoadedPlugin {
    fn on_hook(&self, context: &mut HookContext) {
        let hook_name =
            CString::new(context.hook.as_str()).expect("hook name should not contain null bytes");

        let data_json = serde_json::to_string(&context.data).unwrap_or_default();
        let data_c = CString::new(data_json).unwrap_or_default();

        let shared_json = serde_json::to_string(&context.shared).unwrap_or_default();
        let shared_c = CString::new(shared_json).unwrap_or_default();

        // Allocate buffer for shared state output (64 KiB should be plenty).
        const SHARED_BUF_SIZE: usize = 65536;
        let mut shared_out = vec![0u8; SHARED_BUF_SIZE];

        let bytes_written = unsafe {
            (self.callback)(
                hook_name.as_ptr(),
                data_c.as_ptr(),
                shared_c.as_ptr(),
                shared_out.as_mut_ptr().cast::<c_char>(),
                SHARED_BUF_SIZE,
            )
        };

        // If the plugin wrote updated shared state, parse it back.
        if bytes_written > 0
            && bytes_written < SHARED_BUF_SIZE
            && let Ok(updated_str) = std::str::from_utf8(&shared_out[..bytes_written])
        {
            if let Ok(updated) = serde_json::from_str(updated_str) {
                context.shared = updated;
            } else {
                log::warn!(
                    "Plugin '{}' returned invalid JSON for shared state",
                    self.name
                );
            }
        }
    }

    fn subscribed_hooks(&self) -> Vec<Hook> {
        self.hooks.clone()
    }
}

/// Parse a comma-separated hook names string into a list of `Hook` values.
///
/// Unknown hook names are logged and skipped.
fn parse_hook_list(hooks_str: &str) -> Vec<Hook> {
    hooks_str
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|name| {
            // Try to deserialize from the JSON string representation.
            let json = format!("\"{name}\"");
            match serde_json::from_str::<Hook>(&json) {
                Ok(hook) => Some(hook),
                Err(_) => {
                    log::warn!("Unknown hook name in plugin descriptor: '{name}'");
                    None
                }
            }
        })
        .collect()
}

/// Load a native plugin from a shared library path.
///
/// The library must export a `reeln_plugin_init` symbol with the
/// `PluginInitFn` signature.
///
/// # Errors
///
/// Returns `LoadError` if the library cannot be loaded, the entry point
/// is missing, or the descriptor contains invalid data.
///
/// # Safety
///
/// This function loads and executes code from a shared library. The caller
/// must ensure the library is trusted.
pub fn load_plugin(path: &Path) -> Result<LoadedPlugin, LoadError> {
    if !path.exists() {
        return Err(LoadError::NotFound(path.display().to_string()));
    }

    // SAFETY: Loading a shared library executes its init functions.
    // The caller is responsible for ensuring the library is trusted.
    let library = unsafe { libloading::Library::new(path.as_os_str()) }
        .map_err(|e| LoadError::Library(e.to_string()))?;

    // Look up the entry point symbol.
    let init_fn: libloading::Symbol<PluginInitFn> =
        unsafe { library.get(ENTRY_POINT_SYMBOL) }.map_err(|e| LoadError::Symbol(e.to_string()))?;

    // Call the init function to get the descriptor.
    let descriptor = unsafe { init_fn() };

    // Validate and extract the name.
    if descriptor.name.is_null() {
        return Err(LoadError::InvalidDescriptor(
            "name pointer is null".to_string(),
        ));
    }
    let name = unsafe { CStr::from_ptr(descriptor.name) }
        .to_str()
        .map_err(|e| LoadError::InvalidDescriptor(format!("invalid name UTF-8: {e}")))?
        .to_string();

    // Validate and extract the version.
    if descriptor.version.is_null() {
        return Err(LoadError::InvalidDescriptor(
            "version pointer is null".to_string(),
        ));
    }
    let version = unsafe { CStr::from_ptr(descriptor.version) }
        .to_str()
        .map_err(|e| LoadError::InvalidDescriptor(format!("invalid version UTF-8: {e}")))?
        .to_string();

    // Parse subscribed hooks.
    let hooks = if descriptor.subscribed_hooks.is_null() {
        Vec::new()
    } else {
        let hooks_str = unsafe { CStr::from_ptr(descriptor.subscribed_hooks) }
            .to_str()
            .map_err(|e| LoadError::InvalidDescriptor(format!("invalid hooks UTF-8: {e}")))?;
        parse_hook_list(hooks_str)
    };

    Ok(LoadedPlugin {
        _library: library,
        name,
        version,
        hooks,
        callback: descriptor.on_hook,
    })
}

/// Scan a directory for plugin shared libraries and load them.
///
/// Looks for files matching the platform-specific pattern:
/// - macOS: `*.dylib`
/// - Linux: `*.so`
/// - Windows: `*.dll`
///
/// Returns a list of successfully loaded plugins along with any errors
/// encountered during loading.
pub fn discover_plugins(dir: &Path) -> (Vec<LoadedPlugin>, Vec<(std::path::PathBuf, LoadError)>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();

    let extension = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    };

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("Cannot read plugin directory {}: {e}", dir.display());
            return (plugins, errors);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(extension) {
            match load_plugin(&path) {
                Ok(plugin) => {
                    log::info!("Loaded plugin '{}' v{}", plugin.name(), plugin.version());
                    plugins.push(plugin);
                }
                Err(e) => {
                    log::warn!("Failed to load plugin {}: {e}", path.display());
                    errors.push((path, e));
                }
            }
        }
    }

    (plugins, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── LoadError tests ──────────────────────────────────────────────

    #[test]
    fn load_error_library_display() {
        let e = LoadError::Library("libfoo.so not found".to_string());
        assert!(e.to_string().contains("failed to load library"));
        assert!(e.to_string().contains("libfoo.so not found"));
    }

    #[test]
    fn load_error_symbol_display() {
        let e = LoadError::Symbol("reeln_plugin_init".to_string());
        assert!(e.to_string().contains("entry point not found"));
    }

    #[test]
    fn load_error_invalid_descriptor_display() {
        let e = LoadError::InvalidDescriptor("name is null".to_string());
        assert!(e.to_string().contains("invalid plugin descriptor"));
    }

    #[test]
    fn load_error_not_found_display() {
        let e = LoadError::NotFound("/foo/bar.so".to_string());
        assert!(e.to_string().contains("library not found"));
        assert!(e.to_string().contains("/foo/bar.so"));
    }

    #[test]
    fn load_error_debug() {
        let e = LoadError::Library("test".to_string());
        let dbg = format!("{e:?}");
        assert!(dbg.contains("Library"));
    }

    // ── parse_hook_list tests ────────────────────────────────────────

    #[test]
    fn parse_hook_list_single() {
        let hooks = parse_hook_list("on_game_init");
        assert_eq!(hooks, vec![Hook::OnGameInit]);
    }

    #[test]
    fn parse_hook_list_multiple() {
        let hooks = parse_hook_list("on_game_init,on_game_finish,on_error");
        assert_eq!(
            hooks,
            vec![Hook::OnGameInit, Hook::OnGameFinish, Hook::OnError]
        );
    }

    #[test]
    fn parse_hook_list_with_whitespace() {
        let hooks = parse_hook_list("on_game_init , pre_render , post_render");
        assert_eq!(
            hooks,
            vec![Hook::OnGameInit, Hook::PreRender, Hook::PostRender]
        );
    }

    #[test]
    fn parse_hook_list_empty() {
        let hooks = parse_hook_list("");
        assert!(hooks.is_empty());
    }

    #[test]
    fn parse_hook_list_unknown_hooks_skipped() {
        let hooks = parse_hook_list("on_game_init,totally_fake,on_error");
        assert_eq!(hooks, vec![Hook::OnGameInit, Hook::OnError]);
    }

    #[test]
    fn parse_hook_list_all_hooks() {
        let all_names: Vec<&str> = Hook::all().iter().map(|h| h.as_str()).collect();
        let hooks_str = all_names.join(",");
        let hooks = parse_hook_list(&hooks_str);
        assert_eq!(hooks.len(), 14);
    }

    #[test]
    fn parse_hook_list_trailing_comma() {
        let hooks = parse_hook_list("on_game_init,");
        assert_eq!(hooks, vec![Hook::OnGameInit]);
    }

    #[test]
    fn parse_hook_list_leading_comma() {
        let hooks = parse_hook_list(",on_game_init");
        assert_eq!(hooks, vec![Hook::OnGameInit]);
    }

    #[test]
    fn parse_hook_list_double_comma() {
        let hooks = parse_hook_list("on_game_init,,on_error");
        assert_eq!(hooks, vec![Hook::OnGameInit, Hook::OnError]);
    }

    // ── load_plugin error path tests ─────────────────────────────────

    #[test]
    fn load_plugin_not_found() {
        let result = load_plugin(Path::new("/nonexistent/plugin.so"));
        assert!(result.is_err());
        match result.unwrap_err() {
            LoadError::NotFound(msg) => assert!(msg.contains("nonexistent")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn load_plugin_invalid_library() {
        // Create a temp file that is not a valid shared library.
        let dir = tempfile::tempdir().unwrap();
        let fake_lib = dir.path().join("fake.so");
        std::fs::write(&fake_lib, b"not a real library").unwrap();

        let result = load_plugin(&fake_lib);
        assert!(result.is_err());
        match result.unwrap_err() {
            LoadError::Library(_) => {}
            other => panic!("expected Library error, got {other:?}"),
        }
    }

    // ── discover_plugins tests ───────────────────────────────────────

    #[test]
    fn discover_plugins_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (plugins, errors) = discover_plugins(dir.path());
        assert!(plugins.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn discover_plugins_nonexistent_dir() {
        let (plugins, errors) = discover_plugins(Path::new("/nonexistent/plugins"));
        assert!(plugins.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn discover_plugins_skips_non_library_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not a plugin").unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        let (plugins, errors) = discover_plugins(dir.path());
        assert!(plugins.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn discover_plugins_reports_invalid_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else if cfg!(target_os = "windows") {
            "dll"
        } else {
            "so"
        };
        let fake = dir.path().join(format!("bad.{ext}"));
        std::fs::write(&fake, b"not a library").unwrap();

        let (plugins, errors) = discover_plugins(dir.path());
        assert!(plugins.is_empty());
        assert_eq!(errors.len(), 1);
    }

    // ── PluginDescriptor / C ABI type tests ──────────────────────────

    #[test]
    fn entry_point_symbol_is_null_terminated() {
        assert!(ENTRY_POINT_SYMBOL.ends_with(b"\0"));
        let s = std::str::from_utf8(&ENTRY_POINT_SYMBOL[..ENTRY_POINT_SYMBOL.len() - 1]).unwrap();
        assert_eq!(s, "reeln_plugin_init");
    }

    // ── LoadedPlugin tests (using mock descriptor) ───────────────────

    // We can't easily create a real LoadedPlugin without a real .so file,
    // but we can test the HookHandler implementation by constructing one
    // with a mock callback.

    /// A mock callback that writes `{"called": true}` to shared_out.
    unsafe extern "C" fn mock_callback(
        _hook_name: *const c_char,
        _data_json: *const c_char,
        _shared_json: *const c_char,
        shared_out: *mut c_char,
        shared_out_len: usize,
    ) -> usize {
        let response = b"{\"called\":true}";
        if shared_out_len >= response.len() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    response.as_ptr().cast::<c_char>(),
                    shared_out,
                    response.len(),
                );
            }
            response.len()
        } else {
            0
        }
    }

    /// A mock callback that returns 0 (no shared state changes).
    unsafe extern "C" fn noop_callback(
        _hook_name: *const c_char,
        _data_json: *const c_char,
        _shared_json: *const c_char,
        _shared_out: *mut c_char,
        _shared_out_len: usize,
    ) -> usize {
        0
    }

    /// A mock callback that writes invalid JSON to shared_out.
    unsafe extern "C" fn invalid_json_callback(
        _hook_name: *const c_char,
        _data_json: *const c_char,
        _shared_json: *const c_char,
        shared_out: *mut c_char,
        shared_out_len: usize,
    ) -> usize {
        let response = b"not json";
        if shared_out_len >= response.len() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    response.as_ptr().cast::<c_char>(),
                    shared_out,
                    response.len(),
                );
            }
            response.len()
        } else {
            0
        }
    }

    /// Helper to create a mock LoadedPlugin without a real library.
    ///
    /// # Safety
    /// This creates a LoadedPlugin that does NOT hold a real library handle.
    /// Only use in tests — the _library field is loaded from the current
    /// executable which is always valid.
    fn mock_plugin(
        name: &str,
        version: &str,
        hooks: Vec<Hook>,
        callback: HookCallbackFn,
    ) -> LoadedPlugin {
        // Load the current executable as a "library" — this is safe and keeps
        // the library handle valid for the test lifetime.
        let exe = std::env::current_exe().unwrap();
        let library = unsafe { libloading::Library::new(exe.as_os_str()) }.unwrap();
        LoadedPlugin {
            _library: library,
            name: name.to_string(),
            version: version.to_string(),
            hooks,
            callback,
        }
    }

    #[test]
    fn loaded_plugin_name_and_version() {
        let plugin = mock_plugin(
            "test-plugin",
            "1.2.3",
            vec![Hook::OnGameInit],
            noop_callback,
        );
        assert_eq!(plugin.name(), "test-plugin");
        assert_eq!(plugin.version(), "1.2.3");
    }

    #[test]
    fn loaded_plugin_info() {
        let plugin = mock_plugin(
            "test-plugin",
            "1.0.0",
            vec![Hook::OnGameInit, Hook::OnError],
            noop_callback,
        );
        let info = plugin.info();
        assert_eq!(info.name, "test-plugin");
        assert_eq!(info.entry_point, "native");
        assert!(info.enabled);
        assert_eq!(
            info.capabilities,
            vec!["hook:on_game_init", "hook:on_error"]
        );
    }

    #[test]
    fn loaded_plugin_debug() {
        let plugin = mock_plugin("test", "0.1.0", vec![], noop_callback);
        let dbg = format!("{plugin:?}");
        assert!(dbg.contains("LoadedPlugin"));
        assert!(dbg.contains("test"));
    }

    #[test]
    fn loaded_plugin_subscribed_hooks() {
        let hooks = vec![Hook::OnGameInit, Hook::OnGameFinish, Hook::OnError];
        let plugin = mock_plugin("multi", "1.0.0", hooks.clone(), noop_callback);
        assert_eq!(plugin.subscribed_hooks(), hooks);
    }

    #[test]
    fn loaded_plugin_on_hook_noop() {
        let plugin = mock_plugin("noop", "1.0.0", vec![Hook::OnGameInit], noop_callback);
        let mut ctx = HookContext::new(Hook::OnGameInit);
        plugin.on_hook(&mut ctx);
        // Shared should remain empty since callback returns 0.
        assert!(ctx.shared.is_empty());
    }

    #[test]
    fn loaded_plugin_on_hook_writes_shared() {
        let plugin = mock_plugin("writer", "1.0.0", vec![Hook::OnGameInit], mock_callback);
        let mut ctx = HookContext::new(Hook::OnGameInit);
        plugin.on_hook(&mut ctx);
        assert_eq!(ctx.shared.get("called"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn loaded_plugin_on_hook_passes_data() {
        // The mock callback doesn't inspect data, but this tests
        // that we correctly serialize and pass it without errors.
        let plugin = mock_plugin("data-test", "1.0.0", vec![Hook::OnGameInit], noop_callback);
        let mut data = HashMap::new();
        data.insert("game_id".to_string(), serde_json::json!("abc-123"));
        data.insert("score".to_string(), serde_json::json!(42));
        let mut ctx = HookContext::with_data(Hook::OnGameInit, data);
        plugin.on_hook(&mut ctx);
    }

    #[test]
    fn loaded_plugin_on_hook_invalid_json_from_plugin() {
        let plugin = mock_plugin(
            "bad-json",
            "1.0.0",
            vec![Hook::OnGameInit],
            invalid_json_callback,
        );
        let mut ctx = HookContext::new(Hook::OnGameInit);
        ctx.shared
            .insert("original".to_string(), serde_json::json!(true));
        plugin.on_hook(&mut ctx);
        // Shared should remain unchanged because the plugin returned invalid JSON.
        assert_eq!(ctx.shared.get("original"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn loaded_plugin_on_hook_preserves_shared_on_noop() {
        let plugin = mock_plugin("noop2", "1.0.0", vec![Hook::OnGameInit], noop_callback);
        let mut ctx = HookContext::new(Hook::OnGameInit);
        ctx.shared
            .insert("pre_existing".to_string(), serde_json::json!("keep me"));
        plugin.on_hook(&mut ctx);
        assert_eq!(
            ctx.shared.get("pre_existing"),
            Some(&serde_json::json!("keep me"))
        );
    }

    #[test]
    fn loaded_plugin_hook_handler_trait_object() {
        // Verify LoadedPlugin can be used as a trait object.
        let plugin = mock_plugin("trait-obj", "1.0.0", vec![Hook::OnGameInit], noop_callback);
        let handler: Box<dyn HookHandler> = Box::new(plugin);
        assert_eq!(handler.subscribed_hooks(), vec![Hook::OnGameInit]);
    }

    #[test]
    fn loaded_plugin_registered_in_plugin_registry() {
        use crate::registry::PluginRegistry;

        let plugin = mock_plugin("reg-test", "1.0.0", vec![Hook::OnGameInit], mock_callback);
        let info = plugin.info();

        let mut registry = PluginRegistry::new();
        registry.register_plugin(info, Box::new(plugin));

        let found = registry.get_plugin("reg-test");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "reg-test");
    }

    #[test]
    fn loaded_plugin_emitted_via_plugin_registry() {
        use crate::registry::PluginRegistry;

        let plugin = mock_plugin("emit-test", "1.0.0", vec![Hook::OnGameInit], mock_callback);
        let info = plugin.info();

        let mut registry = PluginRegistry::new();
        registry.register_plugin(info, Box::new(plugin));

        let mut ctx = HookContext::new(Hook::OnGameInit);
        registry.emit(&mut ctx);
        assert_eq!(ctx.shared.get("called"), Some(&serde_json::json!(true)));
    }

    // ── Integration with HookRegistry ────────────────────────────────

    #[test]
    fn loaded_plugin_in_hook_registry() {
        use crate::hooks::HookRegistry;

        let plugin = mock_plugin("hook-reg", "1.0.0", vec![Hook::OnGameInit], mock_callback);
        let mut reg = HookRegistry::new();
        reg.register(Hook::OnGameInit, Box::new(plugin));

        assert!(reg.has_handlers(Hook::OnGameInit));
        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(ctx.shared.get("called"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn loaded_plugin_in_filtered_registry() {
        use crate::hooks::FilteredRegistry;

        let plugin = mock_plugin("filtered", "1.0.0", vec![Hook::OnGameInit], mock_callback);
        let mut reg = FilteredRegistry::new(&[Hook::OnGameInit]);
        reg.register(Hook::OnGameInit, Box::new(plugin));

        assert!(reg.has_handlers(Hook::OnGameInit));
        let mut ctx = HookContext::new(Hook::OnGameInit);
        reg.emit(&mut ctx);
        assert_eq!(ctx.shared.get("called"), Some(&serde_json::json!(true)));
    }

    // ── Multiple plugins in registry ─────────────────────────────────

    /// A mock callback that writes `{"plugin": "alpha"}` to shared_out.
    unsafe extern "C" fn alpha_callback(
        _hook_name: *const c_char,
        _data_json: *const c_char,
        _shared_json: *const c_char,
        shared_out: *mut c_char,
        shared_out_len: usize,
    ) -> usize {
        let response = b"{\"plugin\":\"alpha\"}";
        if shared_out_len >= response.len() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    response.as_ptr().cast::<c_char>(),
                    shared_out,
                    response.len(),
                );
            }
            response.len()
        } else {
            0
        }
    }

    #[test]
    fn multiple_loaded_plugins_in_registry() {
        use crate::registry::PluginRegistry;

        let p1 = mock_plugin("alpha", "1.0.0", vec![Hook::OnGameInit], alpha_callback);
        let p2 = mock_plugin("beta", "2.0.0", vec![Hook::OnGameFinish], noop_callback);
        let i1 = p1.info();
        let i2 = p2.info();

        let mut registry = PluginRegistry::new();
        registry.register_plugin(i1, Box::new(p1));
        registry.register_plugin(i2, Box::new(p2));

        assert_eq!(registry.list_plugins().len(), 2);
    }
}

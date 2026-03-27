pub mod capabilities;
pub mod hooks;
pub mod loader;
pub mod registry;

pub use capabilities::{
    Generator, GeneratorResult, MetadataEnricher, Notifier, UploadMetadata, Uploader,
};
pub use hooks::{FilteredRegistry, Hook, HookContext, HookHandler, HookRegistry};
pub use loader::{LoadError, LoadedPlugin, PluginDescriptor, discover_plugins, load_plugin};
pub use registry::{ConfigField, PluginConfigSchema, PluginInfo, PluginRegistry};

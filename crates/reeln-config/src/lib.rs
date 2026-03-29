pub mod error;
pub mod loader;
pub mod model;
pub mod paths;

pub use error::ConfigError;
pub use loader::{
    apply_env_overrides, deep_merge, default_config, load_config, save_config, validate_config,
};
pub use model::{
    AppConfig, BrandingConfig, EventTypeEntry, IterationConfig, OrchestrationConfig, PathConfig,
    PluginsConfig, RenderProfile, SpeedSegment, VideoConfig,
};
pub use paths::{config_dir, data_dir, default_config_path, resolve_config_path};

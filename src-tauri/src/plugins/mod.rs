pub mod manifest;
pub mod extractor;
pub mod runtime;
pub mod format;

pub use format::{FormatPluginRegistry, ImageFormatPlugin, ReloadResult};
pub use manifest::{ExtractorConfig, InstalledPluginInfo, PluginActionCommand, PluginActionDefinition, PluginManifest};
pub use runtime::{
    compare_natural, discover_plugins, get_plugin_search_dirs, normalize_archive_inner_path,
    split_archive_virtual_path, ArchiveEntryMeta, ArchivePlugin, BuiltinZipArchivePlugin,
    DynamicFormatPlugin, ExternalCliArchivePlugin,
};

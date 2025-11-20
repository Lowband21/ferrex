pub mod background;
pub mod file_watcher;
pub mod folder_monitor;
pub mod fs;
pub mod incremental;
pub mod orchestrator;
pub mod tmdb_folder_generator;

pub use background::BackgroundScanner;
pub use file_watcher::FileWatcher;
pub use folder_monitor::{FolderMonitor, FolderMonitorConfig};
pub use fs::{FileSystem, InMemoryFs, RealFs};
pub use incremental::IncrementalScanner;
pub use orchestrator::{ScanOptions, ScanOrchestrator};
pub use tmdb_folder_generator::{
    DefaultNamingStrategy, GeneratedNode, NamingStrategy, StructurePlan, TmdbFolderGenerator,
    apply_plan_to_inmemory_fs,
};

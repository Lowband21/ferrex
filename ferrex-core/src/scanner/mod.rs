pub mod background;
pub mod file_watcher;
pub mod folder_monitor;
pub mod fs;
pub mod tmdb_folder_generator;
pub mod incremental;
pub mod orchestrator;

pub use background::BackgroundScanner;
pub use file_watcher::FileWatcher;
pub use folder_monitor::{FolderMonitor, FolderMonitorConfig};
pub use fs::{FileSystem, InMemoryFs, RealFs};
pub use tmdb_folder_generator::{apply_plan_to_inmemory_fs, DefaultNamingStrategy, GeneratedNode, NamingStrategy, StructurePlan, TmdbFolderGenerator};
pub use incremental::IncrementalScanner;
pub use orchestrator::{ScanOptions, ScanOrchestrator};

pub mod file_watcher;
pub mod fs;
pub mod tmdb_folder_generator;

pub use file_watcher::FileWatcher;
pub use fs::{FileSystem, InMemoryFs, RealFs};
pub use tmdb_folder_generator::{
    DefaultNamingStrategy, GeneratedNode, NamingStrategy, StructurePlan, TmdbFolderGenerator,
    apply_plan_to_inmemory_fs,
};

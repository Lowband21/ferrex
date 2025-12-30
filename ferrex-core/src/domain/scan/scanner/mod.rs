pub mod fs;
pub mod settings;
pub mod tmdb_folder_generator;

// Filesystem watchers now live under `ferrex_core::infra::scan` alongside the orchestrator-facing
// event bus. The `scanner` namespace is scoped to filesystem enumeration helpers and test fixtures.
pub use fs::{FileSystem, InMemoryFs, RealFs};
pub use tmdb_folder_generator::{
    DefaultNamingStrategy, GeneratedNode, NamingStrategy, StructurePlan,
    TmdbFolderGenerator, apply_plan_to_inmemory_fs,
};

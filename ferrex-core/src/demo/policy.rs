use crate::types::library::LibraryType;
use std::path::PathBuf;

/// Runtime policy toggles that relax validation while demo mode is active.
#[derive(Debug, Clone, Copy, Default)]
pub struct DemoPolicy {
    pub allow_zero_length_files: bool,
    pub skip_metadata_probe: bool,
}

/// Metadata tracked for each demo library the runtime registers.
#[derive(Debug, Clone)]
pub struct DemoRuntimeMetadata {
    pub name: String,
    pub library_type: LibraryType,
    pub root: PathBuf,
}

#![allow(clippy::module_inception)]

//! Demo-mode utilities for generating fake media libraries and relaxing
//! validation rules when operating on synthetic data. This module is only
//! compiled when the `demo` feature flag is enabled so production builds incur
//! zero overhead.

use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::{
    error::{MediaError, Result},
    types::{
        ids::LibraryID,
        library::{Library, LibraryType},
    },
};

pub mod config;
#[cfg(feature = "scan-runtime")]
pub mod generator;
pub mod policy;

pub use config::{DemoLibraryOptions, DemoSeedOptions};
#[cfg(feature = "scan-runtime")]
pub use generator::{
    DemoLibraryPlan, DemoSeedPlan, apply_plan, generate_plan,
    prepare_plan_roots,
};
pub use policy::{DemoPolicy, DemoRuntimeMetadata};

#[cfg(all(test, feature = "demo"))]
mod tests;

static DEMO_CONTEXT: OnceCell<DemoContext> = OnceCell::new();

#[derive(Debug)]
pub struct DemoContext {
    root: PathBuf,
    policy: DemoPolicy,
    libraries: Mutex<HashMap<LibraryID, DemoRuntimeMetadata>>,
}

impl DemoContext {
    pub fn new(root: PathBuf, policy: DemoPolicy) -> Self {
        Self {
            root,
            policy,
            libraries: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn policy(&self) -> &DemoPolicy {
        &self.policy
    }

    pub fn register_library(&self, id: LibraryID, meta: DemoRuntimeMetadata) {
        if let Ok(mut guard) = self.libraries.lock() {
            guard.insert(id, meta);
        }
    }

    pub fn libraries(&self) -> Vec<(LibraryID, DemoRuntimeMetadata)> {
        self.libraries
            .lock()
            .map(|map| {
                map.iter().map(|(id, meta)| (*id, meta.clone())).collect()
            })
            .unwrap_or_default()
    }
}

/// Initialise the global demo context. Only succeeds once.
pub fn init_demo_context(root: PathBuf, policy: DemoPolicy) -> Result<()> {
    DEMO_CONTEXT
        .set(DemoContext::new(root, policy))
        .map_err(|_| {
            MediaError::Internal("demo context already initialised".into())
        })
}

/// Fetch the global demo context if demo mode is active.
pub fn context() -> Option<&'static DemoContext> {
    DEMO_CONTEXT.get()
}

/// Convenience accessor for the demo policy.
pub fn policy() -> Option<&'static DemoPolicy> {
    context().map(|ctx| ctx.policy())
}

/// Helper to register a demo library with the global context.
pub fn register_demo_library(library: &Library) {
    if let Some(ctx) = context() {
        ctx.register_library(
            library.id,
            DemoRuntimeMetadata {
                name: library.name.clone(),
                library_type: library.library_type,
                root: library
                    .paths
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ctx.root().clone()),
            },
        );
    }
}

/// Clear all registered demo libraries (used when regenerating plans).
pub fn clear_registered_libraries() {
    if let Some(ctx) = context()
        && let Ok(mut guard) = ctx.libraries.lock()
    {
        guard.clear();
    }
}

/// Check if the provided library ID belongs to the demo runtime.
pub fn is_demo_library(id: &LibraryID) -> bool {
    context()
        .and_then(|ctx| ctx.libraries.lock().ok())
        .map(|map| map.contains_key(id))
        .unwrap_or(false)
}

/// Returns whether zero-length files are permitted for the given library.
/// When the demo feature is disabled this always returns `false`.
pub fn allow_zero_length_for(id: &LibraryID) -> bool {
    let ctx = match context() {
        Some(ctx) => ctx,
        None => return false,
    };

    if !ctx.policy().allow_zero_length_files {
        return false;
    }

    ctx.libraries
        .lock()
        .ok()
        .map(|map| map.contains_key(id))
        .unwrap_or(false)
}

/// Create a placeholder library record for the provided plan.
#[cfg(feature = "scan-runtime")]
pub fn library_from_plan(plan: &DemoLibraryPlan) -> Library {
    use chrono::Utc;
    use uuid::Uuid;

    Library {
        id: LibraryID(Uuid::now_v7()),
        name: plan.name.clone(),
        library_type: plan.library_type,
        paths: vec![plan.root_path.clone()],
        scan_interval_minutes: 24 * 60,
        last_scan: None,
        enabled: true,
        auto_scan: true,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        media: None,
    }
}

/// Basic descriptor used to report demo libraries at runtime.
#[derive(Debug, Clone)]
pub struct DemoLibraryDescriptor {
    pub id: LibraryID,
    pub name: String,
    pub library_type: LibraryType,
    pub root: PathBuf,
}

// Centralized service abstractions and compatibility utilities
// RUS-136 Phase 0: Compatibility layer

use std::sync::Arc;

pub mod api;
pub mod auth;
pub mod metadata;
pub mod settings;
pub mod streaming;
pub mod user_management;

/// A reference to either a concrete service instance or a trait object.
/// This allows domains to work with a unified handle while we migrate
/// from concrete types to trait-based repository_ports.
#[derive(Clone, Debug)]
pub enum ServiceRef<T, Trait>
where
    Trait: ?Sized + Send + Sync + 'static,
{
    Concrete(Arc<T>),
    Trait(Arc<Trait>),
}

impl<T, Trait> ServiceRef<T, Trait>
where
    Trait: ?Sized + Send + Sync + 'static,
{
    pub fn from_concrete(concrete: Arc<T>) -> Self {
        ServiceRef::Concrete(concrete)
    }
    pub fn from_trait(tr: Arc<Trait>) -> Self {
        ServiceRef::Trait(tr)
    }
}

/// Global internal toggle for pilot migrations.
/// Intentionally not exposed via user-config; will be removed post-migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatToggles {
    pub prefer_trait_services: bool,
}

impl Default for CompatToggles {
    fn default() -> Self {
        Self {
            prefer_trait_services: true,
        }
    }
}

/// Simple builder to construct and wire services for domains during app startup.
#[derive(Clone, Debug)]
pub struct ServiceBuilder {
    toggles: CompatToggles,
}

impl Default for ServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceBuilder {
    pub fn new() -> Self {
        Self {
            toggles: CompatToggles::default(),
        }
    }
    pub fn with_toggles(mut self, toggles: CompatToggles) -> Self {
        self.toggles = toggles;
        self
    }
    pub fn toggles(&self) -> CompatToggles {
        self.toggles
    }
}

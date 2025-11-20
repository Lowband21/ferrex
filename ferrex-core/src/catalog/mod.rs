//! Catalog bounded context facade.
//!
//! Re-export catalog-centric domain concepts, ports, and helpers so
//! downstream crates can migrate away from flattened crate-root exports.

#[cfg(feature = "database")]
pub mod ports {
    pub use crate::database::ports::images::ImageRepository;
    pub use crate::database::ports::library::LibraryRepository;
    pub use crate::database::ports::media_references::MediaReferencesRepository;
    pub use crate::database::ports::query::QueryRepository;
}

pub mod types {
    pub use crate::types::library::*;
}

pub mod query {
    pub use crate::query::complexity_guard::{ComplexityConfig, QueryComplexityGuard};
    pub use crate::query::prelude::*;
}

#[cfg(feature = "database")]
pub mod indices {
    pub use crate::indices::*;
}

pub use crate::extras_parser as extras;
#[cfg(feature = "database")]
pub use crate::image::MediaImageKind;
#[cfg(feature = "database")]
pub use crate::image::records;

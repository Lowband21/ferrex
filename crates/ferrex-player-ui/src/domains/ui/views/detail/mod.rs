//! Shared adaptive detail-view primitives.
//!
//! The detail module is split into three layers:
//!
//! - [`model`] defines repository-free render data for movie, series, season,
//!   and episode detail routes.
//! - [`layout`] resolves a pure viewport/scale/interface-mode plan using the
//!   centralized size provider and scaled layout values, including the 10-foot
//!   layout composition.
//! - [`components`] renders reusable Iced hero, metadata, action, section,
//!   relationship, empty-state, and backdrop controls from the model and plan.

pub mod components;
pub mod layout;
pub mod model;

pub use components::{
    view_action_cluster, view_backdrop_controls, view_cast_section,
    view_detail_hero, view_empty_state, view_fact_panel, view_hero_art,
    view_metadata_pills, view_overview_section,
    view_registered_relationship_rail, view_relationship_rail, view_section,
    view_sections, view_technical_section,
};
pub use layout::{
    DetailActionClusterLayout, DetailArtAspect, DetailArtLayout, DetailAxis,
    DetailBackdropLayout, DetailComposition, DetailInterfaceMode,
    DetailLayoutInput, DetailLayoutPlan, DetailRailLayout,
    DetailSectionGridLayout, solve_detail_layout,
    solve_detail_layout_from_runtime,
};
pub use model::{
    DetailAction, DetailActionMenuItem, DetailActionRole, DetailArtwork,
    DetailBackdropControl, DetailCastMember, DetailCastSection,
    DetailContentKind, DetailEmptyState, DetailFact, DetailFactPanel,
    DetailMetadataPill, DetailNotice, DetailOverviewSection, DetailPageModel,
    DetailRailItem, DetailRelationshipRail, DetailSection, DetailTechnicalItem,
    DetailTechnicalSection, DetailTone,
};

//! Shared adaptive detail-view primitives.
//!
//! The detail module is split into three layers:
//!
//! - [`model`] defines repository-free render data for movie, series, season,
//!   and episode detail routes.
//! - [`layout`] resolves a pure viewport/scale/interface-mode plan using the
//!   centralized size provider and scaled layout values, including the 10-foot
//!   layout composition.
//! - [`typography`] maps global font/spacing tokens to semantic detail text
//!   roles, readable measures, overflow budgets, and composition-specific
//!   alignment strategies.
//! - [`components`] renders reusable Iced hero, metadata, action, section,
//!   relationship, empty-state, and backdrop controls from the model and plan.

pub mod components;
pub mod layout;
pub mod model;
pub mod typography;

pub use components::{
    DetailActionSurfaceMode, DetailForegroundSurface,
    DetailForegroundSurfaceTokens, DetailRegisteredRailAdapter,
    DetailStageSectionRenderState, detail_action_surface_mode,
    detail_foreground_surface_tokens, detail_stage_section_render_state,
    view_action_cluster, view_backdrop_controls, view_cast_band,
    view_cast_section, view_control_shelf, view_detail_hero, view_detail_stage,
    view_detail_stage_with_registered_rails, view_empty_stage,
    view_empty_state, view_fact_panel, view_fact_ribbon, view_hero_art,
    view_metadata_pills, view_metadata_ribbons, view_notice_slab,
    view_overview_section, view_projection_shelf,
    view_registered_relationship_rail, view_registered_relationship_rail_deck,
    view_relationship_rail, view_relationship_rail_deck, view_section,
    view_sections, view_stage_hero, view_stage_section, view_stage_sections,
    view_technical_ribbon, view_technical_section,
};
pub use layout::{
    DetailActionClusterLayout, DetailArtAspect, DetailArtLayout, DetailAxis,
    DetailBackdropLayout, DetailComposition, DetailControlShelf,
    DetailForegroundLayout, DetailForegroundRect, DetailForegroundStage,
    DetailHeroArtAnchor, DetailInterfaceMode, DetailLayoutInput,
    DetailLayoutPlan, DetailRailDeckLayout, DetailRailLayout,
    DetailReadableCopyLobe, DetailSafeGutters, DetailSectionBandLayout,
    DetailSectionGridLayout, DetailSurfaceIntensityTokens,
    DetailTheaterPlateLayout, DetailTheaterPlateRect, solve_detail_layout,
    solve_detail_layout_from_runtime,
};
pub use model::{
    DetailAction, DetailActionMenuItem, DetailActionRole, DetailArtwork,
    DetailBackdropControl, DetailCastMember, DetailCastSection,
    DetailContentKind, DetailEmptyState, DetailFact, DetailFactPanel,
    DetailMetadataImportance, DetailMetadataKind, DetailMetadataPill,
    DetailNotice, DetailOverviewSection, DetailPageModel, DetailRailItem,
    DetailRelationshipRail, DetailSection, DetailTechnicalItem,
    DetailTechnicalSection, DetailTone, prioritize_metadata_items,
};
pub use typography::{
    DetailCaptionBudgets, DetailColorIntent, DetailFactLayoutMode,
    DetailTextAlignment, DetailTextMetrics, DetailTextOverflow, DetailTextRole,
    DetailTextStyle, DetailTypography, DetailTypographyInput,
};

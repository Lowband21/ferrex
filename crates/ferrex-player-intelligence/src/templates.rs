//! Smart-shelf composer templates.

use ferrex_player_api::api_types::{
    IntelligenceMediaKind, clamp_smart_shelf_item_count,
};
use serde_json::{Value, json};

/// Prompt template used by smart-shelf composer state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfTemplate {
    /// Stable template id.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Short display description.
    pub description: Option<String>,
    /// Prompt inserted into the composer.
    pub prompt: String,
    /// Media kinds requested by the template.
    pub media_kinds: Vec<IntelligenceMediaKind>,
    /// Requested item count.
    pub item_count: u16,
    /// Structured constraints to send with the start request.
    pub constraints: Value,
}

impl SmartShelfTemplate {
    /// Build a template with default movie/series media kinds.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            prompt: prompt.into(),
            media_kinds: vec![
                IntelligenceMediaKind::Movie,
                IntelligenceMediaKind::Series,
            ],
            item_count: clamp_smart_shelf_item_count(8),
            constraints: Value::Null,
        }
    }

    /// Add a display description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Override the requested media kinds.
    pub fn with_media_kinds(
        mut self,
        media_kinds: Vec<IntelligenceMediaKind>,
    ) -> Self {
        self.media_kinds = media_kinds;
        self
    }

    /// Override the requested item count.
    pub fn with_item_count(mut self, item_count: u16) -> Self {
        self.item_count = clamp_smart_shelf_item_count(item_count);
        self
    }

    /// Attach structured API constraints.
    pub fn with_constraints(mut self, constraints: Value) -> Self {
        self.constraints = constraints;
        self
    }
}

/// Built-in templates that require no server state and can be safely shown by UI shells.
pub fn built_in_templates() -> Vec<SmartShelfTemplate> {
    vec![
        SmartShelfTemplate::new(
            "rainy-night",
            "Rainy night",
            "Build a cozy rainy-night shelf with atmospheric movies and series from my library.",
        )
        .with_description("Cozy, moody, rewatch-friendly picks")
        .with_constraints(json!({
            "mood": "cozy_rainy_night",
            "avoid_duplicates": true
        })),
        SmartShelfTemplate::new(
            "hidden-gems",
            "Hidden gems",
            "Find under-watched gems in my library that deserve a spot on a smart shelf.",
        )
        .with_description("Lower-profile titles grounded in library metadata")
        .with_constraints(json!({
            "novelty": "under_watched",
            "avoid_duplicates": true
        })),
        SmartShelfTemplate::new(
            "quick-comfort",
            "Quick comfort",
            "Create a comfort-watch shelf with shorter movies or easy series starts.",
        )
        .with_description("Lower-friction choices for short viewing windows")
        .with_item_count(6)
        .with_constraints(json!({
            "pace": "low_friction",
            "runtime_preference": "shorter"
        })),
    ]
}

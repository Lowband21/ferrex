use crate::{
    domains::ui::{
        messages::UiMessage, views::virtual_carousel::types::CarouselKey,
    },
    infra::shader_widgets::poster::{PosterFace, animation::AnimationBehavior},
};

use ferrex_model::ImageSize;
use iced::Color;
use lucide_icons::Icon;
use uuid::Uuid;

/// Top-level render model for a detail surface.
///
/// This model is intentionally normalized and repository-free: route code is
/// responsible for reading movies, series, seasons, episodes, and playback
/// state, then handing rendering components this plain data shape.
#[derive(Debug, Clone)]
pub struct DetailPageModel {
    pub content_kind: DetailContentKind,
    pub title: String,
    pub subtitle: Option<String>,
    pub eyebrow: Option<String>,
    pub hero_art: DetailArtwork,
    pub metadata: Vec<DetailMetadataPill>,
    pub actions: Vec<DetailAction>,
    pub sections: Vec<DetailSection>,
    pub backdrop_controls: Vec<DetailBackdropControl>,
    pub empty_state: Option<DetailEmptyState>,
}

impl DetailPageModel {
    pub fn new(
        content_kind: DetailContentKind,
        title: impl Into<String>,
    ) -> Self {
        Self {
            content_kind,
            title: title.into(),
            subtitle: None,
            eyebrow: None,
            hero_art: DetailArtwork::None {
                label: "No artwork".to_string(),
            },
            metadata: Vec::new(),
            actions: Vec::new(),
            sections: Vec::new(),
            backdrop_controls: Vec::new(),
            empty_state: None,
        }
    }

    pub fn with_eyebrow(mut self, eyebrow: impl Into<String>) -> Self {
        self.eyebrow = Some(eyebrow.into());
        self
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn with_hero_art(mut self, hero_art: DetailArtwork) -> Self {
        self.hero_art = hero_art;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.empty_state.is_some()
            || (self.metadata.is_empty()
                && self.actions.is_empty()
                && self.sections.is_empty())
    }
}

/// Media type represented by a detail render model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailContentKind {
    Movie,
    Series,
    Season,
    Episode,
}

/// Artwork used by the hero, rails, cast cards, and empty states.
#[derive(Debug, Clone)]
pub enum DetailArtwork {
    Poster {
        media_uuid: Uuid,
        image_id: Option<Uuid>,
        alt: String,
        placeholder: Icon,
        request_size: ImageSize,
        theme_color: Option<Color>,
        animation: Option<AnimationBehavior>,
        face: Option<PosterFace>,
        rotation_y: Option<f32>,
    },
    Still {
        media_uuid: Uuid,
        image_id: Option<Uuid>,
        alt: String,
    },
    Profile {
        media_uuid: Uuid,
        image_id: Option<Uuid>,
        alt: String,
    },
    None {
        label: String,
    },
}

impl DetailArtwork {
    pub fn poster(
        media_uuid: Uuid,
        image_id: Option<Uuid>,
        alt: impl Into<String>,
    ) -> Self {
        Self::Poster {
            media_uuid,
            image_id,
            alt: alt.into(),
            placeholder: Icon::Film,
            request_size: ImageSize::poster_large(),
            theme_color: None,
            animation: None,
            face: None,
            rotation_y: None,
        }
    }

    pub fn tv_poster(
        media_uuid: Uuid,
        image_id: Option<Uuid>,
        alt: impl Into<String>,
    ) -> Self {
        Self::Poster {
            media_uuid,
            image_id,
            alt: alt.into(),
            placeholder: Icon::Tv,
            request_size: ImageSize::poster_large(),
            theme_color: None,
            animation: None,
            face: None,
            rotation_y: None,
        }
    }

    pub fn still(
        media_uuid: Uuid,
        image_id: Option<Uuid>,
        alt: impl Into<String>,
    ) -> Self {
        Self::Still {
            media_uuid,
            image_id,
            alt: alt.into(),
        }
    }

    pub fn with_request_size(mut self, request_size: ImageSize) -> Self {
        if let Self::Poster {
            request_size: current,
            ..
        } = &mut self
        {
            *current = request_size;
        }
        self
    }

    pub fn with_theme_color(mut self, theme_color: Color) -> Self {
        if let Self::Poster {
            theme_color: current,
            ..
        } = &mut self
        {
            *current = Some(theme_color);
        }
        self
    }

    pub fn with_animation(mut self, animation: AnimationBehavior) -> Self {
        if let Self::Poster {
            animation: current, ..
        } = &mut self
        {
            *current = Some(animation);
        }
        self
    }

    pub fn with_face(mut self, face: PosterFace) -> Self {
        if let Self::Poster { face: current, .. } = &mut self {
            *current = Some(face);
        }
        self
    }

    pub fn with_rotation_y(mut self, rotation_y: f32) -> Self {
        if let Self::Poster {
            rotation_y: current,
            ..
        } = &mut self
        {
            *current = Some(rotation_y);
        }
        self
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Poster { alt, .. }
            | Self::Still { alt, .. }
            | Self::Profile { alt, .. } => alt,
            Self::None { label } => label,
        }
    }
}

/// A compact metadata value displayed near the title.
///
/// Neutral descriptive metadata is rendered inline. Playback state and
/// audience metadata can still render as accent chips.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetailMetadataPill {
    pub label: String,
    pub tone: DetailTone,
    pub kind: DetailMetadataKind,
    pub importance: DetailMetadataImportance,
}

impl DetailMetadataPill {
    pub fn neutral(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: DetailTone::Neutral,
            kind: DetailMetadataKind::Descriptive,
            importance: DetailMetadataImportance::Secondary,
        }
    }

    pub fn playback_state(label: impl Into<String>, tone: DetailTone) -> Self {
        Self {
            label: label.into(),
            tone,
            kind: DetailMetadataKind::PlaybackState,
            importance: DetailMetadataImportance::Primary,
        }
    }

    pub fn rating(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: DetailTone::Warning,
            kind: DetailMetadataKind::AudienceRating,
            importance: DetailMetadataImportance::Tertiary,
        }
    }

    pub fn with_importance(
        mut self,
        importance: DetailMetadataImportance,
    ) -> Self {
        self.importance = importance;
        self
    }

    pub fn renders_as_chip(&self) -> bool {
        self.tone != DetailTone::Neutral
            || !matches!(self.kind, DetailMetadataKind::Descriptive)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailMetadataKind {
    Descriptive,
    PlaybackState,
    AudienceRating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailMetadataImportance {
    Primary,
    Secondary,
    Tertiary,
}

impl DetailMetadataImportance {
    fn rank(self) -> u8 {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
            Self::Tertiary => 2,
        }
    }
}

impl DetailMetadataKind {
    fn rank(self) -> u8 {
        match self {
            Self::Descriptive => 0,
            Self::PlaybackState => 1,
            Self::AudienceRating => 2,
        }
    }
}

pub fn prioritize_metadata_items(items: &mut [DetailMetadataPill]) {
    items.sort_by_key(|item| (item.importance.rank(), item.kind.rank()));
}

/// Semantic tone shared by pills, facts, and notices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailTone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Muted,
}

/// Action button data. An action with no `on_press` is rendered disabled.
#[derive(Debug, Clone)]
pub struct DetailAction {
    pub id: String,
    pub label: String,
    pub subtitle: Option<String>,
    pub icon: Option<Icon>,
    pub role: DetailActionRole,
    pub on_press: Option<UiMessage>,
    pub menu_items: Vec<DetailActionMenuItem>,
}

impl DetailAction {
    pub fn primary(
        id: impl Into<String>,
        label: impl Into<String>,
        on_press: UiMessage,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            icon: Some(Icon::Play),
            role: DetailActionRole::Primary,
            on_press: Some(on_press),
            menu_items: Vec::new(),
        }
    }

    pub fn secondary(
        id: impl Into<String>,
        label: impl Into<String>,
        on_press: UiMessage,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            icon: None,
            role: DetailActionRole::Secondary,
            on_press: Some(on_press),
            menu_items: Vec::new(),
        }
    }

    pub fn disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            icon: None,
            role: DetailActionRole::Secondary,
            on_press: None,
            menu_items: Vec::new(),
        }
    }

    pub fn menu(
        id: impl Into<String>,
        label: impl Into<String>,
        menu_items: Vec<DetailActionMenuItem>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            icon: Some(Icon::Ellipsis),
            role: DetailActionRole::Secondary,
            on_press: None,
            menu_items,
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

#[derive(Debug, Clone)]
pub struct DetailActionMenuItem {
    pub label: String,
    pub on_press: UiMessage,
}

impl DetailActionMenuItem {
    pub fn new(label: impl Into<String>, on_press: UiMessage) -> Self {
        Self {
            label: label.into(),
            on_press,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailActionRole {
    Primary,
    Secondary,
    Destructive,
    Back,
    Toggle,
}

/// A content section below or beside the hero.
#[derive(Debug, Clone)]
pub enum DetailSection {
    Overview(DetailOverviewSection),
    Facts(DetailFactPanel),
    Cast(DetailCastSection),
    Technical(DetailTechnicalSection),
    RelationshipRail(DetailRelationshipRail),
    Empty(DetailEmptyState),
    Notice(DetailNotice),
}

impl DetailSection {
    pub fn title(&self) -> &str {
        match self {
            Self::Overview(section) => &section.title,
            Self::Facts(section) => &section.title,
            Self::Cast(section) => &section.title,
            Self::Technical(section) => &section.title,
            Self::RelationshipRail(section) => &section.title,
            Self::Empty(section) => &section.title,
            Self::Notice(section) => &section.title,
        }
    }
}

/// Semantic family for a detail media rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailRailKind {
    Cast,
    Seasons,
    Episodes,
    EpisodeSiblings,
    Related,
}

/// Renderer-facing card shape used by the variant-aware rail metrics solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailRailCardVariant {
    Profile,
    Poster,
    StillWide,
}

/// Cache/fetch image family requested by a detail rail item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailRailImageRequestKind {
    Poster,
    Still,
    Profile,
    None,
}

impl DetailRailImageRequestKind {
    pub fn from_artwork(artwork: &DetailArtwork) -> Self {
        match artwork {
            DetailArtwork::Poster { .. } => Self::Poster,
            DetailArtwork::Still { .. } => Self::Still,
            DetailArtwork::Profile { .. } => Self::Profile,
            DetailArtwork::None { .. } => Self::None,
        }
    }
}

/// Rail-level activation semantics for remote and pointer surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailRailActivationPolicy {
    Disabled,
    ActivateItem,
    Navigate,
    Play,
}

impl DetailRailActivationPolicy {
    pub fn allows_item_activation(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// Repository-free, normalized detail media rail.
#[derive(Debug, Clone)]
pub struct DetailMediaRail {
    pub stable_key: String,
    pub kind: DetailRailKind,
    pub card_variant: DetailRailCardVariant,
    pub carousel_key: Option<CarouselKey>,
    pub title: String,
    pub items: Vec<DetailMediaRailItem>,
    pub empty_message: Option<String>,
    pub activation_policy: DetailRailActivationPolicy,
}

impl DetailMediaRail {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Repository-free detail rail item with stable identity and display metadata.
#[derive(Debug, Clone)]
pub struct DetailMediaRailItem {
    pub stable_id: String,
    pub image_request_kind: DetailRailImageRequestKind,
    pub title: String,
    pub meta: Option<String>,
    pub badge: Option<String>,
    pub progress: Option<f32>,
    pub artwork: DetailArtwork,
    pub on_press: Option<UiMessage>,
}

impl DetailMediaRailItem {
    pub fn new(
        stable_id: impl Into<String>,
        title: impl Into<String>,
        artwork: DetailArtwork,
    ) -> Self {
        let image_request_kind =
            DetailRailImageRequestKind::from_artwork(&artwork);
        Self {
            stable_id: stable_id.into(),
            image_request_kind,
            title: title.into(),
            meta: None,
            badge: None,
            progress: None,
            artwork,
            on_press: None,
        }
    }

    pub fn with_meta(mut self, meta: impl Into<String>) -> Self {
        let meta = meta.into();
        self.meta = (!meta.trim().is_empty()).then_some(meta);
        self
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        let badge = badge.into();
        self.badge = (!badge.trim().is_empty()).then_some(badge);
        self
    }

    pub fn with_progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    pub fn with_activation(mut self, on_press: UiMessage) -> Self {
        self.on_press = Some(on_press);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetailOverviewSection {
    pub title: String,
    pub body: String,
}

impl DetailOverviewSection {
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            title: "Overview".to_string(),
            body: body.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetailFactPanel {
    pub title: String,
    pub facts: Vec<DetailFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetailFact {
    pub label: String,
    pub value: String,
    pub tone: DetailTone,
}

impl DetailFact {
    pub fn neutral(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone: DetailTone::Neutral,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailCastSection {
    pub title: String,
    pub members: Vec<DetailCastMember>,
    pub empty_message: Option<String>,
}

impl DetailCastSection {
    pub fn to_media_rail(&self) -> DetailMediaRail {
        DetailMediaRail {
            stable_key: "cast".to_string(),
            kind: DetailRailKind::Cast,
            card_variant: DetailRailCardVariant::Profile,
            carousel_key: None,
            title: self.title.clone(),
            items: self
                .members
                .iter()
                .map(DetailCastMember::to_media_rail_item)
                .collect(),
            empty_message: self.empty_message.clone(),
            activation_policy: DetailRailActivationPolicy::Disabled,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailCastMember {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub artwork: DetailArtwork,
}

impl DetailCastMember {
    pub fn to_media_rail_item(&self) -> DetailMediaRailItem {
        let mut item = DetailMediaRailItem::new(
            self.id.clone(),
            self.name.clone(),
            self.artwork.clone(),
        );
        if let Some(role) = &self.role {
            item = item.with_meta(role.clone());
        }
        item
    }
}

#[derive(Debug, Clone)]
pub struct DetailTechnicalSection {
    pub title: String,
    pub items: Vec<DetailTechnicalItem>,
    pub empty_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DetailTechnicalItem {
    pub label: String,
    pub value: String,
    pub icon: Option<Icon>,
    pub tone: DetailTone,
}

#[derive(Debug, Clone)]
pub struct DetailRelationshipRail {
    pub id: String,
    pub kind: DetailRailKind,
    pub card_variant: DetailRailCardVariant,
    pub activation_policy: DetailRailActivationPolicy,
    pub carousel_key: Option<CarouselKey>,
    pub title: String,
    pub items: Vec<DetailRailItem>,
    pub empty_message: Option<String>,
}

impl DetailRelationshipRail {
    pub fn to_media_rail(&self) -> DetailMediaRail {
        DetailMediaRail {
            stable_key: self.id.clone(),
            kind: self.kind,
            card_variant: self.card_variant,
            carousel_key: self.carousel_key.clone(),
            title: self.title.clone(),
            items: self
                .items
                .iter()
                .map(DetailRailItem::to_media_rail_item)
                .collect(),
            empty_message: self.empty_message.clone(),
            activation_policy: self.activation_policy,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailRailItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork: DetailArtwork,
    pub on_press: Option<UiMessage>,
}

impl DetailRailItem {
    pub fn to_media_rail_item(&self) -> DetailMediaRailItem {
        let mut item = DetailMediaRailItem::new(
            self.id.clone(),
            self.title.clone(),
            self.artwork.clone(),
        );
        if let Some(subtitle) = &self.subtitle {
            item = item.with_meta(subtitle.clone());
        }
        if let Some(on_press) = &self.on_press {
            item = item.with_activation(on_press.clone());
        }
        item
    }
}

#[derive(Debug, Clone)]
pub struct DetailEmptyState {
    pub title: String,
    pub message: String,
    pub icon: Option<Icon>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetailNotice {
    pub title: String,
    pub message: String,
    pub tone: DetailTone,
}

#[derive(Debug, Clone)]
pub struct DetailBackdropControl {
    pub label: String,
    pub on_press: UiMessage,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    #[test]
    fn detail_page_model_empty_state_tracks_renderable_content() {
        let mut model =
            DetailPageModel::new(DetailContentKind::Series, "Signal Grove");
        assert!(model.is_empty());

        model.metadata.push(DetailMetadataPill::neutral("2024"));
        assert!(!model.is_empty());

        model.empty_state = Some(DetailEmptyState {
            title: "No rows".to_string(),
            message: "Refresh the library and retry.".to_string(),
            icon: None,
        });
        assert!(model.is_empty());
    }

    #[test]
    fn detail_artwork_builders_preserve_route_specific_image_requests() {
        let poster =
            DetailArtwork::poster(uuid(1), Some(uuid(2)), "Movie poster")
                .with_request_size(ImageSize::Poster(
                    ferrex_model::PosterSize::W342,
                ))
                .with_face(PosterFace::Back)
                .with_rotation_y(std::f32::consts::PI)
                .with_animation(AnimationBehavior::flip_then_fade());
        let still =
            DetailArtwork::still(uuid(3), Some(uuid(4)), "Episode still");

        match poster {
            DetailArtwork::Poster {
                media_uuid,
                image_id,
                request_size,
                face,
                rotation_y,
                animation,
                ..
            } => {
                assert_eq!(media_uuid, uuid(1));
                assert_eq!(image_id, Some(uuid(2)));
                assert_eq!(
                    request_size,
                    ImageSize::Poster(ferrex_model::PosterSize::W342)
                );
                assert_eq!(face, Some(PosterFace::Back));
                assert_eq!(rotation_y, Some(std::f32::consts::PI));
                assert!(animation.is_some());
            }
            other => panic!("expected poster artwork, got {other:?}"),
        }

        assert_eq!(still.label(), "Episode still");
    }

    #[test]
    fn metadata_semantics_keep_descriptive_values_inline() {
        let year = DetailMetadataPill::neutral("2024");
        let progress = DetailMetadataPill::playback_state(
            "52% watched",
            DetailTone::Accent,
        );
        let rating = DetailMetadataPill::rating("★ 8.4");

        assert!(!year.renders_as_chip());
        assert!(progress.renders_as_chip());
        assert!(rating.renders_as_chip());
        assert_eq!(year.kind, DetailMetadataKind::Descriptive);
        assert_eq!(progress.importance, DetailMetadataImportance::Primary);
    }

    #[test]
    fn metadata_prioritization_preserves_editorial_importance() {
        let mut metadata = vec![
            DetailMetadataPill::rating("★ 8.4"),
            DetailMetadataPill::neutral("PG-13"),
            DetailMetadataPill::neutral("2024")
                .with_importance(DetailMetadataImportance::Primary),
            DetailMetadataPill::playback_state("Watched", DetailTone::Success),
        ];

        prioritize_metadata_items(&mut metadata);

        assert_eq!(metadata[0].label, "2024");
        assert_eq!(metadata[1].label, "Watched");
        assert_eq!(metadata[2].label, "PG-13");
        assert_eq!(metadata[3].label, "★ 8.4");
    }

    #[test]
    fn cast_section_adapts_to_unified_profile_media_rail() {
        let section = DetailCastSection {
            title: "Cast".to_string(),
            empty_message: Some("No cast".to_string()),
            members: vec![DetailCastMember {
                id: "person-42".to_string(),
                name: "Ada".to_string(),
                role: Some("Captain".to_string()),
                artwork: DetailArtwork::Profile {
                    media_uuid: uuid(42),
                    image_id: Some(uuid(43)),
                    alt: "Ada profile".to_string(),
                },
            }],
        };

        let rail = section.to_media_rail();

        assert_eq!(rail.stable_key, "cast");
        assert_eq!(rail.kind, DetailRailKind::Cast);
        assert_eq!(rail.card_variant, DetailRailCardVariant::Profile);
        assert_eq!(
            rail.activation_policy,
            DetailRailActivationPolicy::Disabled
        );
        assert_eq!(rail.empty_message.as_deref(), Some("No cast"));
        assert_eq!(rail.items[0].stable_id, "person-42");
        assert_eq!(rail.items[0].meta.as_deref(), Some("Captain"));
        assert_eq!(
            rail.items[0].image_request_kind,
            DetailRailImageRequestKind::Profile
        );
    }

    #[test]
    fn relationship_rail_adapts_to_unified_media_rail_identity_and_policy() {
        let rail = DetailRelationshipRail {
            id: "SeasonEpisodes:season-1".to_string(),
            kind: DetailRailKind::Episodes,
            card_variant: DetailRailCardVariant::StillWide,
            activation_policy: DetailRailActivationPolicy::Play,
            carousel_key: None,
            title: "Episodes".to_string(),
            empty_message: None,
            items: vec![DetailRailItem {
                id: "episode-1".to_string(),
                title: "S01E01".to_string(),
                subtitle: Some("Pilot".to_string()),
                artwork: DetailArtwork::still(
                    uuid(1),
                    Some(uuid(2)),
                    "Pilot still",
                ),
                on_press: None,
            }],
        }
        .to_media_rail();

        assert_eq!(rail.stable_key, "SeasonEpisodes:season-1");
        assert_eq!(rail.kind, DetailRailKind::Episodes);
        assert_eq!(rail.card_variant, DetailRailCardVariant::StillWide);
        assert!(rail.activation_policy.allows_item_activation());
        assert_eq!(rail.items[0].stable_id, "episode-1");
        assert_eq!(rail.items[0].meta.as_deref(), Some("Pilot"));
        assert_eq!(
            rail.items[0].image_request_kind,
            DetailRailImageRequestKind::Still
        );
    }
}

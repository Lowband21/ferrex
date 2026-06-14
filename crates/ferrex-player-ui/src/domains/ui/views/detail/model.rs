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
    pub backdrop: Option<DetailBackdrop>,
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
            backdrop: None,
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

    pub fn with_backdrop(mut self, backdrop: DetailBackdrop) -> Self {
        self.backdrop = Some(backdrop);
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
    /// 10-foot detail content uses the same model but typically pairs with the
    /// TenFoot layout composition and focus-aware route messages.
    TenFoot,
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
    Backdrop {
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
            | Self::Backdrop { alt, .. }
            | Self::Profile { alt, .. } => alt,
            Self::None { label } => label,
        }
    }
}

/// Optional page backdrop metadata and controls target.
#[derive(Debug, Clone)]
pub struct DetailBackdrop {
    pub artwork: DetailArtwork,
    pub scrim: DetailBackdropScrim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailBackdropScrim {
    None,
    Light,
    Heavy,
}

/// A compact metadata value displayed near the title.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetailMetadataPill {
    pub label: String,
    pub tone: DetailTone,
}

impl DetailMetadataPill {
    pub fn neutral(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: DetailTone::Neutral,
        }
    }
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

#[derive(Debug, Clone)]
pub struct DetailCastMember {
    pub name: String,
    pub role: Option<String>,
    pub artwork: DetailArtwork,
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
    pub carousel_key: Option<CarouselKey>,
    pub title: String,
    pub items: Vec<DetailRailItem>,
    pub empty_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DetailRailItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork: DetailArtwork,
    pub on_press: Option<UiMessage>,
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
}

use crate::{
    domains::ui::{
        background_ui::BackgroundMessage,
        messages::UiMessage,
        playback_ui::PlaybackMessage,
        theme,
        views::{
            detail::{
                DetailAction, DetailActionMenuItem, DetailArtwork,
                DetailBackdropControl, DetailCastMember, DetailCastSection,
                DetailContentKind, DetailFact, DetailFactPanel,
                DetailLayoutPlan, DetailMetadataPill, DetailOverviewSection,
                DetailPageModel, DetailSection, DetailTechnicalItem,
                DetailTechnicalSection, DetailTone,
                solve_detail_layout_from_runtime, view_backdrop_controls,
                view_detail_hero, view_sections,
            },
            grid::macros::parse_hex_color,
        },
    },
    infra::{
        constants::layout::header,
        shader_widgets::poster::{
            PosterFace, PosterInstanceKey, animation::AnimationBehavior,
        },
    },
    state::State,
};

use ferrex_contracts::prelude::MovieLike;
use ferrex_core::{traits::id::MediaIDLike, types::ids::MovieID};
use ferrex_model::{
    EnhancedMovieDetails, ImageSize, media::ArchivedMovieReference,
};
use iced::{
    Element, Length,
    widget::{Column, Space, Stack, column, container, text},
};
use lucide_icons::Icon;
use rkyv::{deserialize, option::ArchivedOption, rancor::Error};
use uuid::Uuid;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_movie_detail<'a>(
    state: &'a State,
    movie_id: MovieID,
) -> Element<'a, UiMessage> {
    let movie_uuid = movie_id.to_uuid();
    let Some(yoke_arc) =
        state.domains.ui.state.movie_yoke_cache.peek(&movie_uuid)
    else {
        return media_error_view(
            state,
            "Media Not Found",
            "Repository error: yoke not loaded.",
        );
    };

    let movie = *yoke_arc.get();
    let movie_details =
        match deserialize::<EnhancedMovieDetails, Error>(&movie.details) {
            Ok(details) => details,
            Err(error) => {
                log::warn!(
                    "[MovieDetail] Failed to decode movie details for {}: {:?}",
                    movie_uuid,
                    error
                );
                return media_error_view(
                    state,
                    "Media Details Unavailable",
                    "Repository error: movie details could not be decoded.",
                );
            }
        };

    let theme_color = deserialize::<Option<String>, Error>(&movie.theme_color)
        .ok()
        .flatten()
        .and_then(|hex| parse_hex_color(&hex).ok());
    let (face, rotation_y) = poster_menu_face(state, movie_uuid);

    let model = build_movie_detail_model(
        state,
        movie_id,
        movie,
        &movie_details,
        theme_color,
        face,
        rotation_y,
    );
    let plan = movie_detail_layout_plan(state);
    view_movie_detail_model(state, &model, &plan)
}

fn view_movie_detail_model(
    state: &State,
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
) -> Element<'static, UiMessage> {
    let sizes = &state.domains.ui.state.size_provider;
    let window_width = state.window_size.width;
    let window_height = state.window_size.height;
    let content_offset = state
        .domains
        .ui
        .state
        .background_shader_state
        .calculate_content_offset_height(window_width, window_height)
        .max(0.0);

    let mut body = Column::new()
        .spacing(plan.section_grid.gap)
        .padding([plan.page_padding_y, plan.page_padding_x])
        .width(Length::Fill)
        .max_width(plan.content_width);

    body = body.push(view_detail_hero(model, plan, sizes));
    if !model.sections.is_empty() {
        body = body.push(view_sections(&model.sections, plan, sizes));
    }

    let content = column![
        Space::new().height(Length::Fixed(content_offset)),
        container(body)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
    ]
    .width(Length::Fill);

    let content_container = container(content).width(Length::Fill);
    let mut layered = Stack::new().push(content_container);

    if !model.backdrop_controls.is_empty() {
        let backdrop_dims = state
            .domains
            .ui
            .state
            .background_shader_state
            .calculate_backdrop_dimensions(window_width, window_height);
        let controls =
            view_backdrop_controls(&model.backdrop_controls, plan, sizes);
        let button_container = container(controls)
            .padding([0.0, plan.page_padding_x])
            .width(Length::Fill)
            .height(Length::Fixed(
                backdrop_dims
                    .button_height
                    .max(plan.backdrop.control_height),
            ))
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom);
        layered = layered.push(button_container);
    }

    layered.into()
}

fn movie_detail_layout_plan(state: &State) -> DetailLayoutPlan {
    solve_detail_layout_from_runtime(
        state.window_size.width,
        state.window_size.height,
        state
            .domains
            .ui
            .state
            .view
            .header_height()
            .unwrap_or(header::HEIGHT),
        state.interface_mode,
        &state.domains.ui.state.size_provider,
        &state.domains.ui.state.scaled_layout,
    )
}

fn build_movie_detail_model(
    state: &State,
    movie_id: MovieID,
    movie: &ArchivedMovieReference,
    movie_details: &EnhancedMovieDetails,
    theme_color: Option<iced::Color>,
    face: PosterFace,
    rotation_y: Option<f32>,
) -> DetailPageModel {
    let media_id = movie_id.to_media_id();
    let media_uuid = media_id.to_uuid();
    let title = movie.title.to_string();
    let poster_quality = state.domains.settings.display.detail_poster_quality;

    let mut hero_art = DetailArtwork::poster(
        media_uuid,
        movie_details.primary_poster_iid,
        format!("{title} poster"),
    )
    .with_request_size(ImageSize::Poster(poster_quality))
    .with_animation(AnimationBehavior::flip_then_fade())
    .with_face(face);

    if let Some(color) = theme_color {
        hero_art = hero_art.with_theme_color(color);
    }
    if let Some(rotation_y) = rotation_y {
        hero_art = hero_art.with_rotation_y(rotation_y);
    }

    let mut model =
        DetailPageModel::new(DetailContentKind::Movie, title.clone())
            .with_hero_art(hero_art);

    let directors = movie_details
        .crew
        .iter()
        .filter(|crew| crew.job == "Director")
        .map(|director| director.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if !directors.is_empty() {
        model =
            model.with_eyebrow(format!("Directed by {}", directors.join(", ")));
    }

    let genres = movie_details
        .genres
        .iter()
        .map(|genre| genre.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if !genres.is_empty() {
        model = model.with_subtitle(genres.clone());
    }

    model.metadata =
        movie_metadata_pills(state, media_id, movie, movie_details);
    model.actions = movie_actions(media_id);
    model.backdrop_controls.push(DetailBackdropControl {
        label: backdrop_aspect_label(state),
        on_press: BackgroundMessage::ToggleBackdropAspectMode.into(),
    });

    if let Some(overview) = movie_details
        .overview
        .as_deref()
        .map(str::trim)
        .filter(|overview| !overview.is_empty())
    {
        model
            .sections
            .push(DetailSection::Overview(DetailOverviewSection {
                title: "Synopsis".to_string(),
                body: overview.to_string(),
            }));
    }

    if let Some(facts) = movie_fact_panel(movie_details, &genres) {
        model.sections.push(DetailSection::Facts(facts));
    }

    if let Some(technical) = movie_technical_section(movie) {
        model.sections.push(DetailSection::Technical(technical));
    }

    let cast = movie_cast_members(movie_details);
    if !cast.is_empty() {
        model.sections.push(DetailSection::Cast(DetailCastSection {
            title: "Cast".to_string(),
            members: cast,
            empty_message: None,
        }));
    }

    model
}

fn movie_metadata_pills(
    state: &State,
    media_id: ferrex_model::media_id::MediaID,
    movie: &ArchivedMovieReference,
    movie_details: &EnhancedMovieDetails,
) -> Vec<DetailMetadataPill> {
    let mut metadata = Vec::new();

    if let Some(year) = movie
        .release_year()
        .map(str::trim)
        .filter(|year| !year.is_empty())
    {
        metadata.push(DetailMetadataPill::neutral(year.to_string()));
    }

    if let Some(runtime) = movie_details
        .runtime
        .map(format_runtime_minutes)
        .or_else(|| file_duration_label(movie))
    {
        metadata.push(DetailMetadataPill::neutral(runtime));
    }

    if let Some(content_rating) = movie_details
        .content_rating
        .as_deref()
        .map(str::trim)
        .filter(|rating| !rating.is_empty())
    {
        metadata.push(DetailMetadataPill::neutral(content_rating.to_string()));
    }

    if let Some(progress) =
        state.domains.media.state.get_media_progress(&media_id)
    {
        if state.domains.media.state.is_watched(&media_id) {
            metadata.push(DetailMetadataPill {
                label: "✓ Watched".to_string(),
                tone: DetailTone::Success,
            });
        } else {
            metadata.push(DetailMetadataPill {
                label: format!("{}% watched", (progress * 100.0) as u32),
                tone: DetailTone::Accent,
            });
        }
    }

    if let Some(rating) = movie_details.vote_average {
        let mut label = format!("★ {:.1}", rating);
        if let Some(votes) = movie_details.vote_count {
            label.push_str(&format!(" ({} votes)", votes));
        }
        metadata.push(DetailMetadataPill {
            label,
            tone: DetailTone::Warning,
        });
    }

    metadata
}

fn movie_actions(
    media_id: ferrex_model::media_id::MediaID,
) -> Vec<DetailAction> {
    vec![
        DetailAction::primary(
            "play",
            "Play",
            PlaybackMessage::PlayMediaWithId(media_id).into(),
        ),
        DetailAction::menu(
            "more",
            "More",
            vec![DetailActionMenuItem::new(
                "Play in MPV",
                PlaybackMessage::PlayMediaWithIdInMpv(media_id).into(),
            )],
        )
        .with_icon(Icon::Ellipsis),
    ]
}

fn movie_fact_panel(
    movie_details: &EnhancedMovieDetails,
    genres: &str,
) -> Option<DetailFactPanel> {
    let mut facts = Vec::new();

    if let Some(release_date) = movie_details
        .release_date
        .as_deref()
        .map(str::trim)
        .filter(|date| !date.is_empty())
    {
        facts.push(DetailFact::neutral("Release Date", release_date));
    }

    if !genres.is_empty() {
        facts.push(DetailFact::neutral("Genres", genres.to_string()));
    }

    let production = movie_details
        .production_companies
        .iter()
        .map(|company| company.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if !production.is_empty() {
        facts.push(DetailFact::neutral("Production", production));
    }

    if let Some(status) = movie_details
        .status
        .as_deref()
        .map(str::trim)
        .filter(|status| !status.is_empty())
    {
        facts.push(DetailFact::neutral("Status", status));
    }

    if facts.is_empty() {
        None
    } else {
        Some(DetailFactPanel {
            title: "Details".to_string(),
            facts,
        })
    }
}

fn movie_technical_section(
    movie: &ArchivedMovieReference,
) -> Option<DetailTechnicalSection> {
    let ArchivedOption::Some(metadata) = &movie.file.media_file_metadata else {
        return None;
    };

    let mut items = Vec::new();

    if let ArchivedOption::Some(width) = metadata.width
        && let ArchivedOption::Some(height) = metadata.height
    {
        items.push(technical_item(
            "Resolution",
            format!("{}×{}", width, height),
            None,
            DetailTone::Neutral,
        ));
    }

    if let ArchivedOption::Some(codec) = &metadata.video_codec {
        push_nonempty_technical_item(
            &mut items,
            "Video",
            codec.to_string(),
            Some(Icon::Film),
            DetailTone::Neutral,
        );
    }

    if let ArchivedOption::Some(codec) = &metadata.audio_codec {
        push_nonempty_technical_item(
            &mut items,
            "Audio",
            codec.to_string(),
            Some(Icon::Volume2),
            DetailTone::Neutral,
        );
    }

    if let ArchivedOption::Some(bitrate) = metadata.bitrate {
        let mbps = bitrate.to_native() as f64 / 1_000_000.0;
        items.push(technical_item(
            "Bitrate",
            format!("{:.1} Mbps", mbps),
            None,
            DetailTone::Neutral,
        ));
    }

    if let ArchivedOption::Some(framerate) = metadata.framerate {
        items.push(technical_item(
            "Frame Rate",
            format!("{:.2} fps", framerate),
            None,
            DetailTone::Neutral,
        ));
    }

    let size_gb =
        movie.file.size.to_native() as f64 / (1024.0 * 1024.0 * 1024.0);
    items.push(technical_item(
        "Size",
        format!("{:.2} GB", size_gb),
        None,
        DetailTone::Neutral,
    ));

    let mut hdr_label = None;
    if let ArchivedOption::Some(transfer) = &metadata.color_transfer {
        let transfer = transfer.to_string().to_ascii_lowercase();
        if transfer.contains("2084") {
            hdr_label = Some("HDR10".to_string());
        } else if transfer.contains("hlg") {
            hdr_label = Some("HLG".to_string());
        }
    }

    if let Some(mut hdr) = hdr_label {
        if let ArchivedOption::Some(bit_depth) = metadata.bit_depth {
            hdr.push_str(&format!(" {}bit", bit_depth));
        }
        items.push(technical_item("HDR", hdr, None, DetailTone::Accent));
    } else if let ArchivedOption::Some(bit_depth) = metadata.bit_depth {
        items.push(technical_item(
            "Bit Depth",
            format!("{}bit", bit_depth),
            None,
            DetailTone::Neutral,
        ));
    }

    if items.is_empty() {
        None
    } else {
        Some(DetailTechnicalSection {
            title: "Technical Details".to_string(),
            items,
            empty_message: None,
        })
    }
}

fn movie_cast_members(
    movie_details: &EnhancedMovieDetails,
) -> Vec<DetailCastMember> {
    movie_details
        .cast
        .iter()
        .filter(|actor| actor.image_id.is_some())
        .chain(
            movie_details
                .cast
                .iter()
                .filter(|actor| actor.image_id.is_none()),
        )
        .map(|actor| {
            let name = if actor.name.trim().is_empty() {
                "Unknown Cast Member".to_string()
            } else {
                actor.name.clone()
            };
            let role = if actor.character.trim().is_empty() {
                None
            } else {
                Some(actor.character.clone())
            };
            let media_uuid = actor
                .image_id
                .or(actor.person_id)
                .unwrap_or_else(|| Uuid::from_u128(actor.id as u128));
            let artwork = DetailArtwork::Profile {
                media_uuid,
                image_id: actor.image_id,
                alt: format!("{} profile", name),
            };

            DetailCastMember {
                name,
                role,
                artwork,
            }
        })
        .collect()
}

fn push_nonempty_technical_item(
    items: &mut Vec<DetailTechnicalItem>,
    label: &str,
    value: String,
    icon: Option<Icon>,
    tone: DetailTone,
) {
    let value = value.trim();
    if !value.is_empty() {
        items.push(technical_item(label, value.to_string(), icon, tone));
    }
}

fn technical_item(
    label: impl Into<String>,
    value: impl Into<String>,
    icon: Option<Icon>,
    tone: DetailTone,
) -> DetailTechnicalItem {
    DetailTechnicalItem {
        label: label.into(),
        value: value.into(),
        icon,
        tone,
    }
}

fn file_duration_label(movie: &ArchivedMovieReference) -> Option<String> {
    let ArchivedOption::Some(metadata) = &movie.file.media_file_metadata else {
        return None;
    };
    let ArchivedOption::Some(duration) = metadata.duration else {
        return None;
    };
    let seconds = duration.to_native();
    if seconds <= 0.0 {
        return None;
    }
    let minutes = (seconds / 60.0).round().max(1.0) as u32;
    Some(format_runtime_minutes(minutes))
}

fn format_runtime_minutes(runtime: u32) -> String {
    let hours = runtime / 60;
    let minutes = runtime % 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

fn backdrop_aspect_label(state: &State) -> String {
    match state
        .domains
        .ui
        .state
        .background_shader_state
        .backdrop_aspect_mode
    {
        crate::domains::ui::types::BackdropAspectMode::Auto => "Auto",
        crate::domains::ui::types::BackdropAspectMode::Force21x9 => "21:9",
    }
    .to_string()
}

fn poster_menu_face(
    state: &State,
    poster_id: Uuid,
) -> (PosterFace, Option<f32>) {
    let instance_key = PosterInstanceKey::standalone(poster_id);
    if let Some(menu_state) =
        state.domains.ui.state.poster_menu_states.get(&instance_key)
    {
        (menu_state.face_from_angle(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open.as_ref()
        == Some(&instance_key)
    {
        (PosterFace::Back, Some(std::f32::consts::PI))
    } else {
        (PosterFace::Front, None)
    }
}

fn media_error_view<'a>(
    state: &'a State,
    title: &str,
    message: &str,
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    container(
        column![
            text(title.to_string())
                .size(fonts.title)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().height(10),
            text(message.to_string())
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        ]
        .spacing(10)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

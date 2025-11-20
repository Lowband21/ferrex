use crate::common::ui_utils::icon_text;
use crate::domains::metadata::image_types;
use crate::domains::ui::messages::Message;
use crate::infrastructure::api_types;
use crate::infrastructure::api_types::{
    MediaDetailsOption, SeriesReference, TmdbDetails, WatchProgress,
};
use crate::{domains::ui::theme, media_card, state_refactored::State};

use ferrex_core::CastMember;
use iced::Font;
use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Element, Length,
};
use lucide_icons::Icon;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn movie_reference_card_with_state<'a>(
    movie: &'a crate::infrastructure::api_types::MovieReference,
    is_hovered: bool,
    is_visible: bool,
    watch_progress: Option<WatchProgress>,
) -> Element<'a, Message> {
    use crate::infrastructure::api_types::{MediaDetailsOption, TmdbDetails};

    let title = movie.title.as_str();
    let movie_id = movie.id.as_uuid();

    // Extract year and other info from details if available
    let info = match &movie.details {
        MediaDetailsOption::Details(TmdbDetails::Movie(details)) => {
            if let Some(release_date) = &details.release_date {
                if let Some(year) = release_date.split('-').next() {
                    year.to_string()
                } else {
                    "Unknown Year".to_string()
                }
            } else {
                "Unknown Year".to_string()
            }
        }
        _ => String::new(), // Empty string to reserve space without visual noise
    };

    // Determine priority based on visibility and hover state
    let priority = if is_hovered || is_visible {
        image_types::Priority::Visible
    } else {
        image_types::Priority::Preload
    };

    // Create the image element with progress indicator
    use crate::domains::ui::widgets::image_for;
    use iced::widget::{button, column, container, mouse_area, text};
    use iced::Length;

    let media_id = ferrex_core::api_types::MediaId::Movie(movie.id.clone());

    // Create image with watch progress and scroll tier
    let mut img = image_for(media_id.clone())
        .size(image_types::ImageSize::Poster)
        .rounded(4.0)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .animation(crate::domains::ui::widgets::AnimationType::enhanced_flip())
        .placeholder(lucide_icons::Icon::Film)
        .priority(priority)
        .is_hovered(is_hovered)
        .on_play(Message::PlayMediaWithId(
            api_types::to_legacy_media_file(&movie.file),
            ferrex_core::api_types::MediaId::Movie(movie.id.clone()),
        ))
        .on_click(Message::ViewMovieDetails(movie.clone()));

    // Add theme color if available
    if let Some(theme_color_str) = &movie.theme_color {
        if let Ok(color) = crate::domains::ui::views::macros::parse_hex_color(theme_color_str) {
            img = img.theme_color(color);
        }
    }

    if let Some(progress) = watch_progress {
        img = img.progress(progress.as_percentage());
    }
    //log::debug!("Movie {:?} watch progress: {:?} -> {}", movie.id, watch_progress, progress);

    // Use theme color for progress indicator if available
    if let Some(theme_color_str) = &movie.theme_color {
        if let Ok(color) = crate::domains::ui::views::macros::parse_hex_color(theme_color_str) {
            img = img.progress_color(color);
        }
    }

    // Create the full card manually to match media_card! structure
    let image_element: Element<'_, Message> = img.into();

    // Wrap with hover detection
    let image_with_hover = mouse_area(image_element)
        .on_enter(Message::MediaHovered(movie_id))
        .on_exit(Message::MediaUnhovered(movie_id));

    // Create poster button
    let poster_element = button(image_with_hover)
        .on_press(Message::ViewMovieDetails(movie.clone()))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    // Create text content
    let text_content = column![
        text(title).size(14),
        text(info)
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(2);

    // Final card layout - no need for Stack since shader handles progress
    column![
        poster_element,
        container(text_content)
            .padding(5)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(60.0))
    ]
    .spacing(5)
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn series_reference_card_with_state<'a>(
    series: &'a SeriesReference,
    is_hovered: bool,
    is_visible: bool,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
) -> Element<'a, Message> {
    let title = series.title.as_str();
    let series_id = series.id.as_uuid();

    // Extract info from details if available
    let info = match &series.details {
        MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
            format!("{} seasons", details.number_of_seasons.unwrap_or(0))
        }
        _ => String::new(), // Empty string to reserve space without visual noise
    };

    // Determine priority based on visibility and hover state
    let priority = if is_hovered || is_visible {
        image_types::Priority::Visible
    } else {
        image_types::Priority::Preload
    };

    use crate::domains::ui::widgets::image_for;
    use iced::widget::{button, column, container, mouse_area, text};
    use iced::Length;

    let media_id = ferrex_core::api_types::MediaId::Series(series.id.clone());

    // Create image with scroll tier
    let mut img = image_for(media_id.clone())
        .size(image_types::ImageSize::Poster)
        .rounded(4.0)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(300.0))
        .animation(crate::domains::ui::widgets::AnimationType::enhanced_flip())
        .placeholder(lucide_icons::Icon::Tv)
        .priority(priority)
        .is_hovered(is_hovered)
        .on_play(Message::PlaySeriesNextEpisode(series.id.clone()))
        .on_click(Message::ViewTvShow(series.id.clone()));

    // Add theme color if available
    if let Some(theme_color_str) = &series.theme_color {
        if let Ok(color) = crate::domains::ui::views::macros::parse_hex_color(theme_color_str) {
            img = img.theme_color(color);
        }
    }

    // Create the full card manually to match media_card! structure
    let image_element: Element<'_, Message> = img.into();

    // Wrap with hover detection
    let image_with_hover = mouse_area(image_element)
        .on_enter(Message::MediaHovered(series_id))
        .on_exit(Message::MediaUnhovered(series_id));

    // Create poster button
    let poster_element = button(image_with_hover)
        .on_press(Message::ViewTvShow(series.id.clone()))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    // Create text content
    let text_content = column![
        text(title).size(14),
        text(info)
            .size(12)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(2);

    // Final card layout
    column![
        poster_element,
        container(text_content)
            .padding(5)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(60.0))
    ]
    .spacing(5)
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn season_reference_card_with_state<'a>(
    season: crate::infrastructure::api_types::SeasonReference,
    is_hovered: bool,
    state: Option<&'a crate::state_refactored::State>,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
) -> Element<'a, Message> {
    use crate::infrastructure::api_types::{MediaDetailsOption, TmdbDetails};

    let season_id = season.id.as_uuid();

    // Extract season name from details if available
    let season_name = match &season.details {
        MediaDetailsOption::Details(TmdbDetails::Season(details)) => {
            if details.name.is_empty() {
                if season.season_number.value() == 0 {
                    "Specials"
                } else {
                    "Season"
                }
            } else {
                details.name.as_str()
            }
        }
        _ => {
            if season.season_number.value() == 0 {
                "Specials"
            } else {
                "Season"
            }
        }
    };

    // Get episode count from state if available, otherwise show loading
    let episode_count = state
        .map(|s| s.domains.media.state.get_season_episode_count(&season_id))
        .unwrap_or(0);

    let subtitle = if episode_count > 0 {
        if season.season_number.value() == 0 {
            format!("{} episodes", episode_count)
        } else {
            format!(
                "Season {} • {} episodes",
                season.season_number.value(),
                episode_count
            )
        }
    } else {
        // No episode count available yet
        if season.season_number.value() == 0 {
            "Specials".to_string()
        } else {
            format!("Season {}", season.season_number.value())
        }
    };

    // Use the media_card! macro for consistent card creation
    media_card! {
        type: Season,
        data: season,
        {
            id: ferrex_core::api_types::MediaId::Season(season.id.clone()),
            title: season_name,
            subtitle: &subtitle,
            image: {
                key: season_id,
                type: Poster,
                fallback: "📺",
            },
            size: Medium,
            on_click: Message::ViewSeason(season.series_id.clone(), season.id.clone()),
            on_play: Message::ViewSeason(season.series_id.clone(), season.id.clone()),
            hover_icon: lucide_icons::Icon::List,
            is_hovered: is_hovered,
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn episode_reference_card_with_state<'a>(
    episode: &'a crate::infrastructure::api_types::EpisodeReference,
    is_hovered: bool,
    state: Option<&'a State>,
    _watch_status: Option<WatchProgress>, // Number of remaining episodes equal to integer from watch_status, individual episode watch progress can be passed with the decimal part
) -> Element<'a, Message> {
    use crate::domains::metadata::image_types::{ImageSize, Priority};
    use crate::domains::ui::views::macros::truncate_text;
    use crate::domains::ui::widgets::image_for;
    use crate::infrastructure::api_types::{MediaDetailsOption, TmdbDetails};
    use iced::{
        widget::{button, column, container, mouse_area, text, Stack},
        Length,
    };

    let episode_id = episode.id.as_uuid();

    // Extract episode name from details if available
    let (episode_name, has_details) = match &episode.details {
        MediaDetailsOption::Details(TmdbDetails::Episode(details)) => (details.name.as_str(), true),
        _ => ("", false),
    };

    // Format season and episode numbers
    let season_episode = format!(
        "S{:02}E{:02}",
        episode.season_number.value(),
        episode.episode_number.value()
    );

    // Episode cards are typically wider (thumbnail format)
    let width = 240.0;
    let height = 135.0; // 16:9 aspect ratio
    let radius = 4.0;

    // Get watch progress for this episode
    let watch_progress = state.and_then(|s| {
        s.domains
            .media
            .state
            .get_media_progress(&ferrex_core::api_types::MediaId::Episode(
                episode.id.clone(),
            ))
    });

    // Create image element
    let mut image_element = image_for(ferrex_core::api_types::MediaId::Episode(episode.id.clone()))
        .size(ImageSize::Thumbnail)
        .rounded(radius)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .placeholder(lucide_icons::Icon::Play)
        .priority(if is_hovered {
            Priority::Visible
        } else {
            Priority::Preload
        });

    // Add watch progress - default to unwatched (0.0) if no watch state
    let progress = watch_progress.unwrap_or(0.0);
    image_element = image_element.progress(progress);

    // Create the poster button
    let poster_button = button(image_element)
        .on_press(Message::PlayMediaWithId(
            crate::infrastructure::api_types::to_legacy_media_file(&episode.file),
            ferrex_core::api_types::MediaId::Episode(episode.id.clone()),
        ))
        .padding(0)
        .style(theme::Button::MediaCard.style());

    // Add hover overlay if needed
    let poster_with_overlay: Element<'_, Message> = if is_hovered {
        use crate::hover_overlay;
        let overlay = hover_overlay!(
            width,
            height,
            radius,
            {
                center: (lucide_icons::Icon::Play, Message::PlayMediaWithId(
                    crate::infrastructure::api_types::to_legacy_media_file(&episode.file),
                    ferrex_core::api_types::MediaId::Episode(episode.id.clone())
                )),
                top_left: (lucide_icons::Icon::Circle, Message::NoOp),
                bottom_left: (lucide_icons::Icon::Pencil, Message::NoOp),
                bottom_right: (lucide_icons::Icon::EllipsisVertical, Message::NoOp),
            }
        );

        Stack::new().push(poster_button).push(overlay).into()
    } else {
        poster_button.into()
    };

    // Truncate episode name if too long
    let truncated_name = if episode_name.is_empty() {
        String::new()
    } else {
        truncate_text(episode_name, 30)
    };

    // Create text content with three lines
    let text_content = column![
        // Line 1: Episode title (always visible)
        text(format!("Episode {}", episode.episode_number.value()))
            .size(14)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        // Line 2: Episode name (with fade animation)
        container(text(truncated_name).size(12).color(if has_details {
            theme::MediaServerTheme::TEXT_SECONDARY
        } else {
            // Start with transparent color for fade-in effect
            iced::Color::from_rgba(
                theme::MediaServerTheme::TEXT_SECONDARY.r,
                theme::MediaServerTheme::TEXT_SECONDARY.g,
                theme::MediaServerTheme::TEXT_SECONDARY.b,
                0.0,
            )
        }))
        .height(Length::Fixed(16.0)), // Reserve space even when empty
        // Line 3: Season/Episode format
        text(season_episode)
            .size(11)
            .color(theme::MediaServerTheme::TEXT_DIMMED)
    ]
    .spacing(2);

    // Complete card
    let card_content = column![
        poster_with_overlay,
        container(text_content)
            .padding(5)
            .width(Length::Fixed(width))
            .height(Length::Fixed(65.0)) // Slightly taller for 3 lines
            .clip(true)
    ]
    .spacing(5);

    // Wrap in mouse area
    mouse_area(
        container(card_content)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height + 70.0)),
    )
    .on_enter(Message::MediaHovered(episode_id))
    .on_exit(Message::MediaUnhovered(episode_id))
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn create_cast_scrollable<'a>(cast: &'a [CastMember]) -> Element<'a, Message> {
    if cast.is_empty() {
        return Space::new(0, 0).into();
    }

    let mut content = column![].spacing(10);

    // Add "Cast" header
    content = content.push(container(text("Cast").size(24)).padding([0, 10]));

    // Create a horizontal scrollable row for cast
    let mut cast_row = row![].spacing(15);

    for actor in cast.iter().take(15) {
        let cast_card = create_cast_card(actor);
        cast_row = cast_row.push(cast_card);
    }

    // Wrap in scrollable container with corrected height
    let cast_scroll = scrollable(container(cast_row).padding([5, 10]))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default().scroller_width(4),
        ))
        .height(Length::Fixed(250.0)); // Increased from 220px to accommodate text

    content.push(cast_scroll).into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn create_cast_card<'a>(actor: &'a CastMember) -> Element<'a, Message> {
    let card_width = 120.0;
    let image_height = 180.0;

    let mut card_content = column![]
        .spacing(5)
        .width(Length::Fixed(card_width))
        .align_x(iced::Alignment::Center);

    // Create a deterministic PersonID from the TMDB person ID
    // This matches the UUID generation in the scanner
    let person_uuid = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("person-{}", actor.id).as_bytes(),
    );
    let person_id = ferrex_core::media::PersonID::new(person_uuid.to_string()).unwrap();

    // Use image_for widget with rounded_image_shader
    let profile_image =
        crate::domains::ui::widgets::image_for(ferrex_core::api_types::MediaId::Person(person_id))
            .size(crate::domains::metadata::image_types::ImageSize::Profile)
            .width(Length::Fixed(card_width))
            .height(Length::Fixed(image_height))
            .rounded(8.0)
            .placeholder(Icon::User);

    card_content = card_content.push(profile_image);

    // Actor name
    card_content = card_content.push(
        text(&actor.name)
            .size(12)
            .color(theme::MediaServerTheme::TEXT_PRIMARY)
            .width(Length::Fixed(card_width))
            .center(),
    );

    // Character name
    card_content = card_content.push(
        text(&actor.character)
            .size(10)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .width(Length::Fixed(card_width))
            .center(),
    );

    card_content.into()
}

/// Create the backdrop aspect ratio toggle button
pub fn create_backdrop_aspect_button<'a>(state: &'a State) -> Element<'a, Message> {
    let aspect_button_text = match state
        .domains
        .ui
        .state
        .background_shader_state
        .backdrop_aspect_mode
    {
        crate::domains::ui::types::BackdropAspectMode::Auto => "Auto",
        crate::domains::ui::types::BackdropAspectMode::Force21x9 => "21:9",
    };

    button(text(aspect_button_text).size(14))
        .on_press(Message::ToggleBackdropAspectMode)
        .style(theme::Button::BackdropControl.style())
        .padding([4, 8])
        .into()
}

/// Create an action button row with play button and optional additional buttons
pub fn create_action_button_row<'a>(
    play_message: Message,
    additional_buttons: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    // Play button with DetailAction style
    let play_button = button(
        row![icon_text(Icon::Play), text("Play").size(16)]
            .spacing(8)
            .align_y(iced::Alignment::Center),
    )
    .on_press(play_message)
    .padding([10, 20])
    .style(theme::Button::DetailAction.style());

    // More options button (3-dot menu) with HeaderIcon style
    let more_button = button(icon_text(Icon::Ellipsis))
        .on_press(Message::NoOp) // TODO: Implement menu
        .padding([10, 20])
        .style(theme::Button::HeaderIcon.style());

    // Build button row starting with play and menu buttons
    let mut button_row = row![play_button, more_button];

    // Add any additional buttons
    for button in additional_buttons {
        button_row = button_row.push(button);
    }

    button_row
        .spacing(0) // No spacing so buttons connect
        .align_y(iced::Alignment::Center)
        .into()
}

/// Create technical details cards for media file metadata
pub fn create_technical_details<'a>(
    metadata: &'a crate::infrastructure::api_types::MediaFileMetadata,
) -> Element<'a, Message> {
    let mut tech_row = row![Space::with_width(20)].spacing(8);

    // Resolution
    if let Some(width) = metadata.width {
        if let Some(height) = metadata.height {
            let resolution_card = container(
                text(format!("{}×{}", width, height))
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            )
            .padding(10)
            .style(theme::Container::TechDetail.style());

            tech_row = tech_row.push(resolution_card);
        }
    }

    // Video codec
    if let Some(codec) = &metadata.video_codec {
        let video_card = container(
            row![
                icon_text(Icon::Film).size(14),
                Space::with_width(5),
                text(codec)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY)
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(10)
        .style(theme::Container::TechDetail.style());

        tech_row = tech_row.push(video_card);
    }

    // Audio codec
    if let Some(codec) = &metadata.audio_codec {
        let audio_card = container(
            row![
                icon_text(Icon::Volume2).size(14),
                Space::with_width(5),
                text(codec)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY)
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(10)
        .style(theme::Container::TechDetail.style());

        tech_row = tech_row.push(audio_card);
    }

    // Bitrate
    if let Some(bitrate) = metadata.bitrate {
        let mbps = bitrate as f64 / 1_000_000.0;
        let bitrate_card = container(
            text(format!("{:.1} Mbps", mbps))
                .size(14)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .padding(10)
        .style(theme::Container::TechDetail.style());

        tech_row = tech_row.push(bitrate_card);
    }

    // Frame rate
    if let Some(framerate) = metadata.framerate {
        let fps_card = container(
            text(format!("{:.0} fps", framerate))
                .size(14)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .padding(10)
        .style(theme::Container::TechDetail.style());

        tech_row = tech_row.push(fps_card);
    }

    // Bit depth
    if let Some(bit_depth) = metadata.bit_depth {
        let depth_card = container(
            text(format!("{}-bit", bit_depth))
                .size(14)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .padding(10)
        .style(theme::Container::TechDetail.style());

        tech_row = tech_row.push(depth_card);
    }

    // Wrap in horizontal scrollable
    let tech_details = scrollable(tech_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default().scroller_width(4).margin(2),
        ))
        .style(theme::Scrollable::style());

    container(
        column![
            text("Technical Details").size(20),
            Space::with_height(10),
            tech_details
        ]
        .spacing(5),
    )
    .width(Length::Fill)
    .into()
}

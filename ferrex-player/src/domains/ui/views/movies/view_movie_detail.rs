use crate::common::ui_utils::{Icon, icon_text};
use crate::domains::ui::views::grid::macros::parse_hex_color;
use crate::domains::ui::widgets::AnimationType;
use crate::infrastructure::api_types::{MediaDetailsOption, TmdbDetails};
use crate::{
    domains::ui::components, domains::ui::messages::Message,
    domains::ui::theme, domains::ui::widgets::image_for::image_for,
    state_refactored::State,
};

use ferrex_core::{
    traits::id::MediaIDLike,
    types::{
        details::{ArchivedMediaDetailsOption, ArchivedTmdbDetails},
        ids::MovieID,
        image_request::Priority,
        util_types::{ImageSize, ImageType},
    },
};
use iced::{
    Element, Length,
    widget::{Space, Stack, column, container, row, scrollable, text},
};
use rkyv::deserialize;
use rkyv::option::ArchivedOption;
use rkyv::rancor::Error;

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
) -> Element<'a, Message> {
    // Borrow from UI yoke cache to satisfy lifetime 'a
    let movie_uuid = movie_id.to_uuid();
    match state.domains.ui.state.movie_yoke_cache.peek(&movie_uuid) {
        Some(yoke_arc) => {
            let media_id = movie_id.to_media_id();

            let movie = *yoke_arc.get();
            let theme_color =
                deserialize::<Option<String>, Error>(&movie.theme_color)
                    .unwrap();

            // Create the main content with proper spacing from top
            let mut content = column![].spacing(20);

            // Add dynamic spacing at the top based on backdrop dimensions
            let window_width = state.window_size.width;
            let window_height = state.window_size.height;
            let content_offset = state
                .domains
                .ui
                .state
                .background_shader_state
                .calculate_content_offset_height(window_width, window_height);
            content = content
                .push(Space::new().height(Length::Fixed(content_offset)));

            let mut poster_element = image_for(media_id.to_uuid())
                .size(ImageSize::Full)
                .image_type(ImageType::Movie)
                .width(Length::Fixed(300.0))
                .height(Length::Fixed(450.0))
                .priority(Priority::Visible)
                .animation(AnimationType::flip());

            if let Some(hex) = theme_color
                && let Ok(color) = parse_hex_color(&hex)
            {
                poster_element = poster_element.theme_color(color);
            }

            // Details column
            let mut details = column![]
                .spacing(5)
                .padding(20)
                .width(Length::Fill)
                .height(Length::Shrink);

            // Title
            details = details.push(
                text(movie.title.to_string())
                    .size(32)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            );

            let movie_details =
                deserialize::<MediaDetailsOption, Error>(&movie.details)
                    .unwrap();

            // Extract movie details for easier access
            let movie_details_opt = if let MediaDetailsOption::Details(
                TmdbDetails::Movie(movie_details),
            ) = movie_details
            {
                log::debug!(
                    "[MovieDetail] Movie '{}' has full TMDB details with overview",
                    movie.title
                );
                Some(movie_details)
            } else {
                match &movie.details {
                    ArchivedMediaDetailsOption::Endpoint(endpoint) => {
                        log::warn!(
                            "[MovieDetail] Movie '{}' only has Endpoint: {}, NOT Details!",
                            movie.title,
                            endpoint
                        );
                    }
                    _ => {
                        log::warn!(
                            "[MovieDetail] Movie '{}' has unexpected details variant",
                            movie.title
                        );
                    }
                }
                None
            };

            // Director info
            if let Some(ref movie_details) = movie_details_opt {
                let directors: Vec<&str> = movie_details
                    .crew
                    .iter()
                    .filter(|c| c.job == "Director")
                    .map(|d| d.name.as_str())
                    .collect();

                if !directors.is_empty() {
                    details = details.push(
                        text(format!("Directed by {}", directors.join(", ")))
                            .size(10)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    );
                }
            }

            // Basic info (year, duration, rating)
            let mut info_parts = vec![];

            // Year
            if let Some(ref movie_details) = movie_details_opt
                && let Some(release_date) = &movie_details.release_date
                && let Some(year) = release_date.split('-').next()
            {
                info_parts.push(year.to_string());
            }

            // Duration - prefer TMDB runtime over file metadata
            if let Some(ref movie_details) = movie_details_opt {
                if let Some(runtime) = movie_details.runtime {
                    let hours = runtime / 60;
                    let minutes = runtime % 60;
                    if hours > 0 {
                        info_parts.push(format!("{}h {}m", hours, minutes));
                    } else {
                        info_parts.push(format!("{}m", minutes));
                    }
                }
            } else if let ArchivedOption::Some(metadata) =
                &movie.file.media_file_metadata
                && let ArchivedOption::Some(duration) = metadata.duration
            {
                // TODO: Is to_native necessary here?
                let hours = (duration.to_native() / 3600.0) as u32;
                let minutes = ((duration.to_native() % 3600.0) / 60.0) as u32;
                if hours > 0 {
                    info_parts.push(format!("{}h {}m", hours, minutes));
                } else {
                    info_parts.push(format!("{}m", minutes));
                }
            }

            // Watch status - add to info_parts
            if let Some(progress) =
                state.domains.media.state.get_media_progress(&media_id)
            {
                if state.domains.media.state.is_watched(&media_id) {
                    info_parts.push("✓ Watched".to_string());
                } else {
                    let percentage = (progress * 100.0) as u32;
                    info_parts.push(format!("{}% watched", percentage));
                }
            }

            // Content rating - TODO: Need to find the right field
            // info_parts.push("PG-13".to_string()); // Placeholder

            if !info_parts.is_empty() {
                details = details.push(
                    text(info_parts.join(" • "))
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                );
            }

            // Genres
            if let Some(ref movie_details) = movie_details_opt {
                if !movie_details.genres.is_empty() {
                    let genre_text = movie_details
                        .genres
                        .iter()
                        .map(|genre| genre.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    details = details.push(
                        text(genre_text)
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    );
                }

                // Rating and votes
                if let Some(rating) = movie_details.vote_average {
                    let mut rating_row = row![
                        text("★")
                            .size(16)
                            .color(theme::MediaServerTheme::WARNING),
                        Space::new().width(5),
                        text(format!("{:.1}", rating))
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY)
                    ]
                    .spacing(3)
                    .align_y(iced::Alignment::Center);

                    if let Some(votes) = movie_details.vote_count {
                        rating_row = rating_row.push(
                            text(format!(" ({} votes)", votes))
                                .size(12)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        );
                    }

                    details = details.push(rating_row);
                }
            }

            let button_row =
                crate::domains::ui::components::create_action_button_row(
                    Message::PlayMediaWithId(media_id),
                    vec![], // No additional buttons yet
                );

            details = details.push(Space::new().height(10));
            details = details.push(button_row);

            // Metadata sections
            if let Some(ref movie_details) = movie_details_opt {
                // Synopsis
                if let Some(desc) = &movie_details.overview {
                    details = details.push(Space::new().height(20));
                    details = details.push(text("Synopsis").size(20));
                    details = details.push(
                        container(text(desc.to_string()).size(14))
                            .padding(10)
                            .width(Length::Fill),
                    );

                    // Production companies below synopsis
                    if !movie_details.production_companies.is_empty() {
                        let companies = movie_details
                            .production_companies
                            .iter()
                            .map(|company| company.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        details = details.push(Space::new().height(15));
                        details = details.push(row![
                            text("Production: ")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                            text(companies)
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY)
                        ]);
                    }
                }
            }

            // Add the poster and details to content
            content = content.push(
                //row![poster_element, details]
                row![poster_element, details]
                    .spacing(10)
                    .height(Length::Shrink)
                    .align_y(iced::alignment::Vertical::Top),
            );

            // Technical details section - displayed as cards below the poster
            if let ArchivedOption::Some(metadata) =
                &movie.file.media_file_metadata
            {
                let mut tech_row = row![Space::new().width(20)].spacing(8); // Start padding and tighter spacing

                // Resolution
                if let ArchivedOption::Some(width) = metadata.width
                    && let ArchivedOption::Some(height) = metadata.height
                {
                    let resolution_card = container(
                        text(format!("{}×{}", width, height))
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .padding(10)
                    .style(theme::Container::TechDetail.style());

                    tech_row = tech_row.push(resolution_card);
                }

                // Video codec
                if let ArchivedOption::Some(codec) = &metadata.video_codec {
                    let video_card = container(
                        row![
                            icon_text(Icon::Film).size(14),
                            Space::new().width(5),
                            text(codec.to_string())
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
                if let ArchivedOption::Some(codec) = &metadata.audio_codec {
                    let audio_card = container(
                        row![
                            icon_text(Icon::Volume2).size(14),
                            Space::new().width(5),
                            text(codec.to_string())
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
                if let ArchivedOption::Some(bitrate) = metadata.bitrate {
                    let mbps = bitrate.to_native() as f64 / 1_000_000.0;
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
                if let ArchivedOption::Some(framerate) = metadata.framerate {
                    let fps_card = container(
                        text(format!("{:.2} fps", framerate))
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .padding(10)
                    .style(theme::Container::TechDetail.style());

                    tech_row = tech_row.push(fps_card);
                }

                // File size
                let size_gb = movie.file.size.to_native() as f64
                    / (1024.0 * 1024.0 * 1024.0);
                let size_card = container(
                    text(format!("{:.2} GB", size_gb))
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                )
                .padding(10)
                .style(theme::Container::TechDetail.style());

                tech_row = tech_row.push(size_card);

                // HDR info - enhanced with bit depth
                let mut hdr_info = None;
                if let ArchivedOption::Some(transfer) = &metadata.color_transfer
                {
                    if transfer.contains("2084") {
                        hdr_info = Some("HDR10");
                    } else if transfer.contains("hlg") {
                        hdr_info = Some("HLG");
                    }
                }

                if let Some(hdr_type) = hdr_info {
                    let mut hdr_text = hdr_type.to_string();
                    if let ArchivedOption::Some(bit_depth) = metadata.bit_depth
                    {
                        hdr_text.push_str(&format!(" {}bit", bit_depth));
                    }

                    let hdr_card = container(
                        text(hdr_text)
                            .size(14)
                            .color(theme::MediaServerTheme::ACCENT_BLUE),
                    )
                    .padding(10)
                    .style(theme::Container::TechDetail.style());

                    tech_row = tech_row.push(hdr_card);
                } else if let ArchivedOption::Some(bit_depth) =
                    metadata.bit_depth
                {
                    // Show bit depth even if not HDR
                    let bit_card = container(
                        text(format!("{}bit", bit_depth))
                            .size(14)
                            .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    )
                    .padding(10)
                    .style(theme::Container::TechDetail.style());

                    tech_row = tech_row.push(bit_card);
                }

                // Add the tech info row to content
                // Wrap in a scrollable container for narrow screens
                let tech_details = scrollable(tech_row)
                    .direction(scrollable::Direction::Horizontal(
                        scrollable::Scrollbar::default()
                            .scroller_width(4)
                            .margin(2),
                    ))
                    .style(theme::Scrollable::style());

                content = content.push(tech_details);
            }

            // Cast section - now in a full-width container at the bottom
            if let ArchivedMediaDetailsOption::Details(
                ArchivedTmdbDetails::Movie(ref movie_details),
            ) = movie.details
            {
                let cast_section =
                    components::create_cast_scrollable(&movie_details.cast);
                content = content.push(cast_section);
            }

            // Create the main content container
            let content_container = container(content).width(Length::Fill);

            // Calculate backdrop dimensions
            let window_width = state.window_size.width;
            let window_height = state.window_size.height;
            let display_aspect = state
                .domains
                .ui
                .state
                .background_shader_state
                .calculate_display_aspect(window_width, window_height);
            let backdrop_height = window_width / display_aspect;
            //let header_offset = constants::layout::header::HEIGHT;

            // Create aspect ratio toggle button
            let aspect_button =
                components::create_backdrop_aspect_button(state);

            // Position the button at bottom-right of backdrop with small margin
            let button_container = container(aspect_button)
                .padding([0, 20])
                .width(Length::Fill)
                .height(Length::Fixed(backdrop_height - 22.5))
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom);

            // Layer the button over the content using Stack
            Stack::new()
                .push(content_container)
                .push(button_container)
                .into()
        }
        _ => container(
            column![
                text("Media Not Found")
                    .size(24)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::new().height(10),
                text("Repository error: yoke not loaded.")
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SUBDUED),
            ]
            .spacing(10)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
    }
}

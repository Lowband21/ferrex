use crate::{
    components::{default_episode_thumbnail, default_season_poster},
    image_cache::{ImageCache, ImageState},
    models::{EpisodeSummary, SeasonSummary},
    theme,
    widgets::rounded_image,
    Message,
};
use iced::advanced::Widget;
use iced::{
    widget::{button, column, container, image, text, Stack},
    Color, Element, Length,
};
use lucide_icons::Icon;

// Helper functions for icons
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

/// Create a season card with URL-based image loading
pub fn season_card_with_cache<'a>(
    season: &'a SeasonSummary,
    show_name: &'a str,
    image_cache: &'a ImageCache,
    server_url: &'a str,
) -> Element<'a, Message> {
    // Create poster element
    let poster_element: Element<Message> = if let Some(poster_url) = &season.poster_url {
        // Convert relative paths to full URLs
        let full_url = if poster_url.starts_with("/") {
            format!("{}{}", server_url, poster_url)
        } else {
            poster_url.clone()
        };
        match image_cache.get(&full_url) {
            Some(ImageState::Loaded(handle)) => rounded_image(handle.clone())
                .size(200.0, 300.0)
                .radius(8.0)
                .build(),
            Some(ImageState::Loading) => container(
                column![text("⏳").size(32), text("Loading...").size(12)]
                    .align_x(iced::Alignment::Center)
                    .spacing(5),
            )
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(300.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(theme::Container::Card.style())
            .into(),
            _ => default_season_poster(),
        }
    } else {
        default_season_poster()
    };

    let title = season.name.clone().unwrap_or_else(|| {
        if season.number == 0 {
            "Specials".to_string()
        } else {
            format!("Season {}", season.number)
        }
    });

    let info = format!("{} Episodes", season.episode_count);

    // For seasons, clicking play button or card navigates to season details
    // (No direct play action since a season contains multiple episodes)
    let content = column![
        button(poster_element)
            .on_press(Message::ViewSeason(show_name.to_string(), season.number))
            .padding(0)
            .style(theme::Button::MediaCard.style()),
        // Text container with fixed dimensions
        container(
            column![
                text(title)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(info)
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY)
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(60.0))
        .clip(true)
    ]
    .spacing(5);

    // Fixed size container
    container(
        container(content)
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(370.0)),
    )
    .width(Length::Fixed(200.0))
    .height(Length::Fixed(370.0))
    .into()
}

/// Create an episode card with thumbnail loading
pub fn episode_card_with_cache<'a>(
    episode: &'a EpisodeSummary,
    show_name: &'a str,
    season_num: u32,
    image_cache: &'a ImageCache,
) -> Element<'a, Message> {
    // Episodes use server thumbnails
    let thumbnail_key = format!("thumbnail:{}", episode.id);
    let thumbnail_element: Element<Message> = match image_cache.get(&thumbnail_key) {
        Some(ImageState::Loaded(handle)) => rounded_image(handle.clone())
            .size(250.0, 140.0)
            .radius(8.0)
            .build(),
        Some(ImageState::Loading) => container(
            column![text("⏳").size(24), text("Loading...").size(11)]
                .align_x(iced::Alignment::Center)
                .spacing(3),
        )
        .width(Length::Fixed(250.0))
        .height(Length::Fixed(140.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Card.style())
        .into(),
        _ => default_episode_thumbnail(episode.number),
    };

    let title = episode
        .title
        .clone()
        .unwrap_or_else(|| format!("Episode {}", episode.number));

    let mut info_parts = vec![format!("E{:02}", episode.number)];

    // Add duration if available
    if let Some(dur) = episode.duration {
        info_parts.push(format!("{} min", (dur / 60.0) as u32));
    }

    // Add air date if available
    if let Some(air_date) = &episode.air_date {
        // Parse and format the date nicely
        if let Some(year) = air_date.split('-').next() {
            info_parts.push(year.to_string());
        }
    }

    let info = info_parts.join(" • ");

    // Create a MediaFile for navigation
    let media_file = crate::media_library::MediaFile {
        id: episode.id.clone(),
        filename: format!("{} S{:02}E{:02}", show_name, season_num, episode.number),
        path: String::new(), // Will be populated when playing
        size: 0,
        created_at: String::new(),
        metadata: None,
        library_id: None, // Episode from details view, no library association
    };

    // Add play button overlay
    let play_button = container(
        button(
            text(icon_char(Icon::Play))
                .font(lucide_font())
                .size(24)
                .color(Color::WHITE),
        )
        .on_press(Message::PlayMedia(media_file.clone()))
        .padding(12)
        .style(theme::Button::PlayOverlay.style()),
    )
    .width(Length::Fill)
    .height(Length::Fixed(140.0)) // Match thumbnail height
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center);

    // Add refresh metadata button overlay (top-right corner)
    let refresh_button = container(
        button(
            text(icon_char(Icon::RefreshCw))
                .font(lucide_font())
                .size(16)
                .color(Color::WHITE),
        )
        .on_press(Message::RefreshEpisodeMetadata(episode.id.clone()))
        .padding(8)
        .style(theme::Button::PlayOverlay.style()),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Right)
    .align_y(iced::alignment::Vertical::Top)
    .padding(8);

    // Stack thumbnail with play button and refresh button
    let thumbnail_with_play = Stack::new()
        .push(
            button(thumbnail_element)
                .on_press(Message::ViewEpisode(media_file.clone()))
                .padding(0)
                .style(theme::Button::MediaCard.style()),
        )
        .push(play_button)
        .push(refresh_button);

    let content = column![
        thumbnail_with_play,
        // Text container with more info
        container(
            column![
                text(title)
                    .size(14)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(info)
                    .size(12)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                // Add truncated description if available
                if let Some(desc) = &episode.description {
                    let truncated = if desc.len() > 80 {
                        format!("{}...", &desc[..80])
                    } else {
                        desc.clone()
                    };
                    text(truncated)
                        .size(11)
                        .color(theme::MediaServerTheme::TEXT_DIMMED)
                } else {
                    text("")
                }
            ]
            .spacing(2)
        )
        .padding(5)
        .width(Length::Fixed(250.0))
        .height(Length::Fixed(80.0)) // Increased height for description
        .clip(true)
    ]
    .spacing(5);

    // Episode cards are wider than poster cards
    container(
        container(content)
            .width(Length::Fixed(250.0))
            .height(Length::Fixed(230.0)), // 140 (thumbnail) + 5 (spacing) + 80 (text) + 5 (padding)
    )
    .width(Length::Fixed(250.0))
    .height(Length::Fixed(230.0))
    .into()
}

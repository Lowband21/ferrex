use crate::{
    common::ui_utils::icon_text,
    domains::ui::{
        background_ui::BackgroundMessage, messages::UiMessage, theme,
        widgets::image_for,
    },
    infra::{
        constants::{layout::carousel::ITEM_SPACING, poster::CORNER_RADIUS},
        shader_widgets::poster::animation::{
            AnimationBehavior, PosterAnimationType,
        },
    },
    state::State,
};

use ferrex_core::player_prelude::{ArchivedCastMember, ImageSize};

use ferrex_model::MediaType;
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use iced_aw::menu::{Item, Menu, MenuBar};

use lucide_icons::Icon;

use rkyv::{deserialize, option::ArchivedOption, rancor::Error};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn create_cast_scrollable(
    cast: &[ArchivedCastMember],
) -> Element<'static, UiMessage> {
    if cast.is_empty() {
        return Space::new().into();
    }

    let mut content = column![].spacing(10);

    // Add "Cast" header
    content = content.push(container(text("Cast").size(24)).padding([0, 10]));

    // Create a horizontal scrollable row for cast
    let mut cast_row = row![].spacing(ITEM_SPACING);

    // Display order: actors with profile images first (preserving original
    // importance/order), followed by actors without images. This ensures
    // placeholders appear at the end of the carousel.
    for actor in cast
        .iter()
        .filter(|a| matches!(a.profile_media_id, ArchivedOption::Some(_)))
    {
        let cast_card = create_cast_card(actor);
        cast_row = cast_row.push(cast_card);
    }

    for actor in cast
        .iter()
        .filter(|a| matches!(a.profile_media_id, ArchivedOption::None))
    {
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
fn create_cast_card(actor: &ArchivedCastMember) -> Element<'static, UiMessage> {
    let card_width = 120.0;
    let image_height = 180.0;

    let mut card_content = column![]
        .spacing(5)
        .width(Length::Fixed(card_width))
        .align_x(iced::Alignment::Center);

    let profile_uuid = match &actor.profile_media_id {
        ArchivedOption::Some(uuid) => Some(*uuid),
        ArchivedOption::None => None,
    };

    let profile_index = match &actor.profile_image_index {
        ArchivedOption::Some(index) => index.to_native(),
        ArchivedOption::None => 0,
    };

    let profile_image: Element<'static, UiMessage> =
        if let Some(uuid) = profile_uuid {
            image_for(uuid)
                .size(ImageSize::profile())
                .image_type(MediaType::Person)
                .animation_behavior(AnimationBehavior::constant(
                    PosterAnimationType::flip(),
                ))
                .width(Length::Fixed(card_width))
                .height(Length::Fixed(image_height))
                .radius(CORNER_RADIUS)
                .image_index(profile_index)
                .placeholder(Icon::User)
                .into()
        } else {
            container(icon_text(Icon::User))
                .width(Length::Fixed(card_width))
                .height(Length::Fixed(image_height))
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        };

    card_content = card_content.push(profile_image);

    // Actor name
    card_content = card_content.push(
        text(deserialize::<String, Error>(&actor.name).unwrap())
            .size(12)
            .color(theme::MediaServerTheme::TEXT_PRIMARY)
            .width(Length::Fixed(card_width))
            .center(),
    );

    // Character name
    card_content = card_content.push(
        text(deserialize::<String, Error>(&actor.character).unwrap())
            .size(10)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .width(Length::Fixed(card_width))
            .center(),
    );

    card_content.into()
}

/// Create the backdrop aspect ratio toggle button
pub fn create_backdrop_aspect_button<'a>(
    state: &'a State,
) -> Element<'a, UiMessage> {
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
        .on_press(BackgroundMessage::ToggleBackdropAspectMode.into())
        .style(theme::Button::BackdropControl.style())
        .padding([4, 8])
        .into()
}

/// Create an action button row with play button and optional additional buttons
pub fn create_action_button_row<'a>(
    play_message: UiMessage,
    play_in_mpv_message: Option<UiMessage>,
    additional_buttons: Vec<Element<'a, UiMessage>>,
) -> Element<'a, UiMessage> {
    // Play button with DetailAction style
    let play_button = button(
        row![icon_text(Icon::Play), text("Play").size(16)]
            .spacing(8)
            .align_y(iced::Alignment::Center),
    )
    .on_press(play_message)
    .padding([10, 20])
    .style(theme::Button::DetailAction.style());

    // More options menu (3-dot) using iced_aw::Menu
    let more_button = button(icon_text(Icon::Ellipsis))
        .padding([10, 20])
        .style(theme::Button::HeaderIcon.style());

    // Build menu items
    let mut items: Vec<Item<'_, UiMessage, iced::Theme, iced::Renderer>> =
        Vec::new();

    if let Some(mpv_msg) = play_in_mpv_message.clone() {
        let play_in_mpv_button = button(text("Play in MPV").size(14))
            .on_press(mpv_msg)
            .style(theme::Button::HeaderMenuSecondary.style());
        items.push(Item::new(play_in_mpv_button));
    }

    let menu = Menu::new(items).max_width(220.0).spacing(0.0).offset(0.0);
    let menu_bar = MenuBar::new(vec![Item::with_menu(more_button, menu)])
        .spacing(0.0)
        .height(Length::Shrink)
        .close_on_item_click(true);

    // Build button row starting with play and menu
    let mut button_row = row![play_button, menu_bar];

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
    metadata: &'a crate::infra::api_types::MediaFileMetadata,
) -> Element<'a, UiMessage> {
    let mut tech_row = row![Space::new().width(20)].spacing(8);

    // Resolution
    if let Some(width) = metadata.width
        && let Some(height) = metadata.height
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
    if let Some(codec) = &metadata.video_codec {
        let video_card = container(
            row![
                icon_text(Icon::Film).size(14),
                Space::new().width(5),
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
                Space::new().width(5),
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
            Space::new().height(10),
            tech_details
        ]
        .spacing(5),
    )
    .width(Length::Fill)
    .into()
}

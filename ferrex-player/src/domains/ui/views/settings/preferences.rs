//! User preferences settings
//!
//! Allows users to customize their viewing experience

use crate::domains::ui::{
    messages::UiMessage, settings_ui::SettingsUiMessage, theme,
};
use crate::state::State;
use ferrex_core::player_prelude::{PlaybackQuality, ResumeBehavior, UserScale};
use iced::widget::{
    Space, button, column, container, pick_list, row, slider, text, toggler,
};
use iced::{Element, Length};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_user_preferences<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    let content = column![
        text("Preferences")
            .size(fonts.title)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().height(20),
        // Playback section
        text("Playback")
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().height(10),
        row![
            text("Auto-play next episode")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            toggler(true) // TODO: Connect to preferences
                .on_toggle(|_| UiMessage::NoOp),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(10),
        row![
            text("Preferred quality")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            pick_list(
                vec![
                    PlaybackQuality::Auto,
                    PlaybackQuality::Original,
                    PlaybackQuality::High4K,
                    PlaybackQuality::High1080p,
                    PlaybackQuality::Medium720p,
                    PlaybackQuality::Low480p,
                ],
                Some(PlaybackQuality::Auto),
                |_| UiMessage::NoOp, // TODO: Implement quality change
            )
            .width(Length::Fixed(150.0)),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(10),
        row![
            text("Resume behavior")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            pick_list(
                vec![
                    ResumeBehavior::Always,
                    ResumeBehavior::Ask,
                    ResumeBehavior::Never,
                ],
                Some(ResumeBehavior::Ask),
                |_| UiMessage::NoOp, // TODO: Implement resume behavior change
            )
            .width(Length::Fixed(150.0)),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(20),
        // Subtitles section
        text("Subtitles")
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().height(10),
        row![
            text("Show subtitles by default")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            toggler(false) // TODO: Connect to preferences
                .on_toggle(|_| UiMessage::NoOp),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(10),
        column![
            text("Subtitle size")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            row![
                text("Small").size(fonts.caption),
                slider(0.5..=2.0, 1.0, |_| UiMessage::NoOp)
                    .width(Length::Fixed(200.0)),
                text("Large").size(fonts.caption),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(5),
        Space::new().height(20),
        // UI section
        text("User Interface")
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().height(10),
        row![
            text("Library grid size")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            pick_list(
                vec![UserScale::Small, UserScale::Medium, UserScale::Large],
                Some(state.domains.settings.preferences.user_scale.clone()),
                |size| SettingsUiMessage::SetUserScale(size).into(),
            )
            .width(Length::Fixed(150.0)),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(10),
        row![
            text("Show poster titles on hover")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            toggler(false) // TODO: Connect to preferences
                .on_toggle(|_| UiMessage::NoOp),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(30),
        row![
            button("Back")
                .on_press(SettingsUiMessage::BackToSettings.into())
                .style(theme::Button::Secondary.style())
                .padding([10, 20]),
            Space::new().width(10),
            button("Save Changes")
                .on_press(UiMessage::NoOp) // TODO: Implement save
                .style(theme::Button::Primary.style())
                .padding([10, 20]),
        ],
    ]
    .spacing(10)
    .padding(20)
    .max_width(600);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

use crate::{theme, Message, State, player::state::TranscodingStatus};
use iced::{
    widget::{button, column, container, row, text, Space, progress_bar},
    Element, Length,
};
use lucide_icons::Icon;

// Helper function to create icon text
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

pub fn view_loading_video<'a>(state: &'a State, url: &'a str) -> Element<'a, Message> {
    let mut content = column![].spacing(20).align_x(iced::Alignment::Center);

    // Back button
    content = content.push(
        container(
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
            )
            .on_press(Message::BackToLibrary)
            .style(theme::Button::Secondary.style()),
        )
        .padding(20),
    );

    content = content.push(Space::with_height(Length::Fill));

    // Loading indicator with status
    let mut loading_content = column![].spacing(20).align_x(iced::Alignment::Center);
    
    // Spinner icon (using refresh icon that will be animated via CSS)
    loading_content = loading_content.push(
        text(Icon::RefreshCw.unicode())
            .font(lucide_font())
            .size(48)
            .color(theme::MediaServerTheme::TEXT_PRIMARY)
    );
    
    // Main loading text
    let loading_text = if state.player.using_hls {
        "Starting Adaptive Streaming"
    } else {
        "Loading Video"
    };
    
    loading_content = loading_content.push(
        text(loading_text)
            .size(24)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );
    
    // Status message based on transcoding state
    let status_message = match &state.player.transcoding_status {
        Some(TranscodingStatus::Pending) => "Initializing transcoding...".to_string(),
        Some(TranscodingStatus::Queued) => "Waiting in transcoding queue...".to_string(),
        Some(TranscodingStatus::Processing { progress }) => {
            format!("Processing: {:.0}%", progress * 100.0)
        }
        Some(TranscodingStatus::Completed) => "Video ready, starting playback...".to_string(),
        Some(TranscodingStatus::Failed { error }) => format!("Error: {}", error),
        Some(TranscodingStatus::Cancelled) => "Transcoding cancelled".to_string(),
        None => {
            if state.player.using_hls {
                "Preparing adaptive bitrate streams...".to_string()
            } else {
                "Connecting to server...".to_string()
            }
        }
    };
    
    loading_content = loading_content.push(
        text(status_message)
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    );
    
    // Show progress bar if transcoding
    if let Some(TranscodingStatus::Processing { progress }) = &state.player.transcoding_status {
        loading_content = loading_content.push(Space::with_height(10));
        loading_content = loading_content.push(
            container(
                progress_bar(0.0..=1.0, *progress)
            )
            .width(Length::Fixed(300.0))
            .height(Length::Fixed(8.0))
        );
    }
    
    // Additional info
    if state.player.using_hls {
        loading_content = loading_content.push(Space::with_height(20));
        loading_content = loading_content.push(
            column![
                text("✓ Non-blocking adaptive streaming enabled")
                    .size(14)
                    .color(theme::MediaServerTheme::SUCCESS),
                text("✓ Quality will adjust based on bandwidth")
                    .size(14)
                    .color(theme::MediaServerTheme::SUCCESS),
            ]
            .spacing(5)
            .align_x(iced::Alignment::Center)
        );
    }
    
    loading_content = loading_content.push(Space::with_height(20));
    loading_content = loading_content.push(
        text(url)
            .size(12)
            .color(theme::MediaServerTheme::TEXT_DIMMED),
    );

    content = content.push(loading_content);
    content = content.push(Space::with_height(Length::Fill));

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Default.style())
        .into()
}

use iced::widget::canvas::{self, Canvas, Geometry, Path, Program, Stroke};
use iced::{
    alignment,
    widget::{container, text, Container, Row, Space},
    Background, Color, Element, Length, Padding, Renderer, Theme, Vector,
};

/// A progress badge that shows watch progress for media items
pub struct ProgressBadge {
    progress: f32,
    is_completed: bool,
    show_percentage: bool,
}

impl ProgressBadge {
    /// Create a new progress badge
    pub fn new(progress: f32, is_completed: bool) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            is_completed,
            show_percentage: false,
        }
    }

    /// Show percentage text on the badge
    pub fn show_percentage(mut self) -> Self {
        self.show_percentage = true;
        self
    }
}

/// Create a progress badge widget
pub fn progress_badge(progress: f32, is_completed: bool) -> ProgressBadge {
    ProgressBadge::new(progress, is_completed)
}

impl<Message> From<ProgressBadge> for Element<'_, Message, Theme, Renderer>
where
    Message: 'static,
{
    fn from(badge: ProgressBadge) -> Self {
        if badge.is_completed {
            // Checkmark badge for completed items
            container(text("✓").size(16))
                .padding(4)
                .style(|_theme: &Theme| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.2, 0.7, 0.2))),
                    text_color: Some(Color::WHITE),
                    border: iced::Border {
                        radius: 12.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    ..Default::default()
                })
                .into()
        } else if badge.progress > 0.0 {
            // Progress bar for items being watched
            let progress_bar = Canvas::new(ProgressBar {
                progress: badge.progress,
            })
            .width(Length::Fixed(60.0))
            .height(Length::Fixed(4.0));

            if badge.show_percentage {
                // Show progress with percentage
                let percentage = format!("{}%", (badge.progress * 100.0) as u8);
                Row::new()
                    .push(progress_bar)
                    .push(Space::with_width(4))
                    .push(text(percentage).size(12))
                    .align_y(alignment::Vertical::Center)
                    .into()
            } else {
                progress_bar.into()
            }
        } else {
            // No badge for unwatched items
            Space::new(0, 0).into()
        }
    }
}

/// Canvas-based progress bar
struct ProgressBar {
    progress: f32,
}

impl<Message> Program<Message> for ProgressBar {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Background track
        let track = Path::rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(bounds.width, bounds.height),
        );
        frame.fill(&track, Color::from_rgba(1.0, 1.0, 1.0, 0.2));

        // Progress fill
        if self.progress > 0.0 {
            let progress_width = bounds.width * self.progress;
            let progress_rect = Path::rectangle(
                iced::Point::ORIGIN,
                iced::Size::new(progress_width, bounds.height),
            );
            frame.fill(&progress_rect, Color::from_rgb(0.9, 0.9, 0.9));
        }

        vec![frame.into_geometry()]
    }
}

/// Position of the progress badge on a poster
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgePosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Default for BadgePosition {
    fn default() -> Self {
        BadgePosition::TopRight
    }
}

/// Create a container with a progress badge
/// Note: This is a simplified version that returns the badge separately.
/// In actual use, you would overlay this on your poster using iced's Stack or Column/Row layout
pub fn create_progress_badge_element<'a, Message: 'static>(
    progress: f32,
    is_completed: bool,
) -> Option<Element<'a, Message, Theme, Renderer>> {
    if progress <= 0.0 && !is_completed {
        // No badge needed
        return None;
    }

    let badge = progress_badge(progress, is_completed);

    // Create badge with background
    Some(
        container(badge)
            .padding(8)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
                border: iced::Border {
                    radius: 8.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..Default::default()
            })
            .into(),
    )
}

/// Create a "NEW" badge for unwatched series items
pub fn new_badge<Message: 'static>() -> Element<'static, Message, Theme, Renderer> {
    container(text("NEW").size(12))
        .padding(Padding::from([2, 6]))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.8, 0.2, 0.2))),
            text_color: Some(Color::WHITE),
            border: iced::Border {
                radius: 6.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            ..Default::default()
        })
        .into()
}

/// Create an episode count badge for TV shows
pub fn episode_count_badge<Message: 'static>(
    count: usize,
) -> Element<'static, Message, Theme, Renderer> {
    container(text(format!("{} EP", count)).size(12))
        .padding(Padding::from([2, 6]))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.8))),
            text_color: Some(Color::from_rgb(0.8, 0.8, 0.8)),
            border: iced::Border {
                radius: 6.0.into(),
                width: 1.0,
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            },
            ..Default::default()
        })
        .into()
}

pub use self::sort_option::SortOption;

use crate::domains::ui::{messages::Message, theme::MediaServerTheme, SortBy};
use iced::{
    widget::{container, pick_list, Container},
    Background, Border, Color, Element, Length,
};

mod sort_option {
    use super::*;

    /// Display labels for sort options
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SortOption {
        pub value: SortBy,
        pub label: &'static str,
    }

    impl SortOption {
        pub const OPTIONS: &'static [SortOption] = &[
            SortOption {
                value: SortBy::DateAdded,
                label: "Date Added",
            },
            SortOption {
                value: SortBy::Title,
                label: "Title",
            },
            SortOption {
                value: SortBy::Year,
                label: "Release Year",
            },
            SortOption {
                value: SortBy::Rating,
                label: "Rating",
            },
            SortOption {
                value: SortBy::Runtime,
                label: "Runtime",
            },
            SortOption {
                value: SortBy::FileSize,
                label: "File Size",
            },
            SortOption {
                value: SortBy::Resolution,
                label: "Resolution",
            },
            SortOption {
                value: SortBy::LastWatched,
                label: "Last Watched",
            },
            SortOption {
                value: SortBy::Genre,
                label: "Genre",
            },
            SortOption {
                value: SortBy::Popularity,
                label: "Popularity",
            },
        ];
    }

    impl std::fmt::Display for SortOption {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.label)
        }
    }
}

/// Creates a sort dropdown widget with consistent styling
pub fn sort_dropdown<'a>(current_sort: SortBy) -> Element<'a, Message> {
    let selected = SortOption::OPTIONS
        .iter()
        .find(|opt| opt.value == current_sort)
        .copied();

    container(
        pick_list(SortOption::OPTIONS, selected, |option| {
            Message::SetSortBy(option.value)
        })
        .placeholder("Sort by...")
        .width(Length::Fixed(160.0))
        .style(sort_dropdown_style),
    )
    .height(Length::Fixed(36.0))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// Custom style for the sort dropdown
fn sort_dropdown_style(
    _theme: &iced::Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let (background, border_color, text_color) = match status {
        iced::widget::pick_list::Status::Active => (
            Color::from_rgba(0.1, 0.1, 0.1, 0.8),
            MediaServerTheme::BORDER_COLOR,
            MediaServerTheme::TEXT_PRIMARY,
        ),
        iced::widget::pick_list::Status::Hovered => (
            Color::from_rgba(0.15, 0.15, 0.15, 0.9),
            MediaServerTheme::ACCENT_BLUE,
            MediaServerTheme::TEXT_PRIMARY,
        ),
        iced::widget::pick_list::Status::Opened { is_hovered: _ } => (
            Color::from_rgba(0.15, 0.15, 0.15, 0.95),
            MediaServerTheme::ACCENT_BLUE,
            MediaServerTheme::TEXT_PRIMARY,
        ),
    };

    iced::widget::pick_list::Style {
        text_color,
        placeholder_color: MediaServerTheme::TEXT_DIMMED,
        handle_color: MediaServerTheme::TEXT_SECONDARY,
        background: Background::Color(background),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 6.0.into(),
        },
    }
}

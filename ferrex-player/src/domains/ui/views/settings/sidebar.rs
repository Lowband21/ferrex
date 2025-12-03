//! Settings sidebar component
//!
//! Provides the sidebar navigation for the unified settings view.
//! Uses a simple column-based layout with permission-based filtering.

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};

use crate::domains::auth::permissions::StatePermissionExt;
use crate::domains::settings::state::SettingsSection;
use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::SettingsUiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::infra::design_tokens::FontTokens;
use crate::state::State;

/// Base width of the settings sidebar in pixels (before scaling)
const SIDEBAR_BASE_WIDTH: f32 = 200.0;

/// Build the settings sidebar with section navigation
///
/// This creates a simple column-based sidebar that:
/// - Shows all user sections
/// - Conditionally shows admin sections based on permissions
/// - Highlights the active section
pub fn build_settings_sidebar<'a>(
    state: &'a State,
    active_section: SettingsSection,
) -> Element<'a, UiMessage> {
    let permissions = state.permission_checker();
    let fonts = state.domains.ui.state.size_provider.font;
    let sidebar_width = state
        .domains
        .ui
        .state
        .size_provider
        .scale(SIDEBAR_BASE_WIDTH);

    let mut sidebar_content = column![].spacing(4).padding(8);

    // User sections header
    sidebar_content = sidebar_content.push(
        text("Settings")
            .size(fonts.small)
            .color(MediaServerTheme::TEXT_SUBDUED),
    );
    sidebar_content = sidebar_content.push(Space::new().height(4));

    // User sections (always visible)
    for section in SettingsSection::user_sections() {
        sidebar_content = sidebar_content.push(section_button(
            *section,
            active_section,
            fonts,
        ));
    }

    // Admin sections (permission-gated)
    if permissions.can_view_admin_dashboard() {
        sidebar_content = sidebar_content.push(Space::new().height(16));
        sidebar_content = sidebar_content.push(
            text("Administration")
                .size(fonts.small)
                .color(MediaServerTheme::TEXT_SUBDUED),
        );
        sidebar_content = sidebar_content.push(Space::new().height(4));

        for section in SettingsSection::admin_sections() {
            // Additional permission checks per section
            let show_section = match section {
                SettingsSection::Libraries => {
                    permissions.can_view_library_settings()
                }
                SettingsSection::Users => permissions.can_view_users(),
                SettingsSection::Server => {
                    permissions.can_view_admin_dashboard()
                }
                _ => false,
            };

            if show_section {
                sidebar_content = sidebar_content.push(section_button(
                    *section,
                    active_section,
                    fonts,
                ));
            }
        }
    }

    let sidebar = container(scrollable(sidebar_content).height(Length::Fill))
        .width(Length::Fixed(sidebar_width))
        .height(Length::Fill)
        .style(theme::Container::Card.style());

    sidebar.into()
}

/// Create a button for a settings section
fn section_button<'a>(
    section: SettingsSection,
    active_section: SettingsSection,
    fonts: FontTokens,
) -> Element<'a, UiMessage> {
    let is_active = section == active_section;
    let label = section.label();

    // Use Primary style for active, Secondary for inactive
    let button_style = if is_active {
        theme::Button::Primary.style()
    } else {
        theme::Button::Secondary.style()
    };

    let content = row![text(label).size(fonts.caption),]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([8, 12]);

    button(content)
        .width(Length::Fill)
        .style(button_style)
        .on_press(SettingsUiMessage::NavigateToSection(section).into())
        .into()
}

/// Build the main settings view with sidebar and content area
pub fn build_settings_layout<'a>(
    state: &'a State,
    active_section: SettingsSection,
    content: Element<'a, UiMessage>,
) -> Element<'a, UiMessage> {
    let sidebar = build_settings_sidebar(state, active_section);

    let content_area = container(scrollable(content).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(20)
        .style(theme::Container::Default.style());

    row![sidebar, content_area]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

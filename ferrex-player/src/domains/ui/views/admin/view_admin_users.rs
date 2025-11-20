use crate::domains::auth::permissions::StatePermissionExt;
use crate::domains::ui::messages::Message;
use crate::domains::ui::theme;
use crate::infra::api_types::AdminUserInfo;
use crate::state::State;
use chrono::Utc;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length};

pub fn view_admin_users(state: &State) -> Element<'_, Message> {
    // Permission gate: require ability to view users
    let permissions = state.permission_checker();
    if !permissions.can_view_users() {
        return container(
            text("You do not have permission to view user management.")
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .padding(20)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    }

    let list = &state.domains.user_management.state.users;

    // Header bar for this view's content area
    let header = row![
        text("User Management")
            .size(20)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::new().width(Length::Fill),
        button("Create User")
            .style(theme::Button::Primary.style())
            .padding([8, 14])
            .on_press(Message::NoOp), // TODO: Wire to create modal
    ]
    .align_y(iced::Alignment::Center);

    // Table header
    let table_header = row![
        text("Username")
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        Space::new().width(20),
        text("Display Name")
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        Space::new().width(20),
        text("Roles")
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        Space::new().width(20),
        text("Sessions")
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        Space::new().width(20),
        text("Created")
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        Space::new().width(Length::Fill),
        text("Actions")
            .size(14)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(10);

    // Rows
    let mut rows = column![table_header].spacing(8);
    for user in list.iter() {
        rows = rows.push(user_row(user));
    }

    let scroll = scrollable(
        container(rows.spacing(8).padding(10))
            .style(theme::Container::Card.style())
            .width(Length::Fill),
    )
    .on_scroll(|_v| Message::NoOp)
    .height(Length::Fill);

    container(
        column![
            container(header)
                .style(theme::Container::Card.style())
                .padding(16)
                .width(Length::Fill),
            Space::new().height(12),
            scroll,
        ]
        .spacing(12)
        .padding(20),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn user_row(user: &AdminUserInfo) -> Element<'_, Message> {
    let roles = if user.roles.is_empty() {
        "-".to_string()
    } else {
        user.roles.join(", ")
    };

    let created = chrono::DateTime::<Utc>::from_timestamp(user.created_at, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| user.created_at.to_string());

    let actions = row![
        button("Edit")
            .style(theme::Button::Secondary.style())
            .padding([6, 10])
            .on_press(Message::NoOp), // TODO: Wire to edit modal
        Space::new().width(8),
        button("Delete")
            .style(theme::Button::Danger.style())
            .padding([6, 10])
            .on_press(Message::UserAdminDelete(user.id)),
    ]
    .align_y(iced::Alignment::Center);

    container(
        row![
            text(&user.username).size(16),
            Space::new().width(20),
            text(&user.display_name).size(16),
            Space::new().width(20),
            text(roles)
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(20),
            text(format!("{}", user.session_count)).size(14),
            Space::new().width(20),
            text(created)
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            actions,
        ]
        .align_y(iced::Alignment::Center)
        .spacing(10),
    )
    .style(theme::Container::Card.style())
    .padding([10, 12])
    .width(Length::Fill)
    .into()
}

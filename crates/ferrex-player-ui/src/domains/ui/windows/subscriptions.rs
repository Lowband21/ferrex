use crate::{
    common::messages::DomainMessage,
    domains::ui::{
        shell_ui::UiShellMessage, window_ui::WindowUiMessage,
        windows::WindowKind,
    },
    state::State,
};

use iced::Subscription;

pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subs: Vec<Subscription<DomainMessage>> = Vec::new();

    subs.push(iced::window::close_events().map(|id| {
        DomainMessage::Ui(UiShellMessage::RawWindowClosed(id).into())
    }));

    // If the main window Id is not known yet (single-window mode),
    // capture the first Opened event and record it.
    if state.windows.get(WindowKind::Main).is_none() {
        subs.push(iced::window::events().map(|(id, event)| match event {
            iced::window::Event::Opened { .. } => {
                DomainMessage::Ui(UiShellMessage::MainWindowOpened(id).into())
            }
            _ => DomainMessage::NoOp,
        }));
    }

    if let Some(main_id) = state.windows.get(WindowKind::Main) {
        subs.push(iced::window::events().with(main_id).map(
            |(tracked_id, (id, event))| match event {
                iced::window::Event::Moved(position) if id == tracked_id => {
                    DomainMessage::Ui(
                        WindowUiMessage::WindowMoved(Some(position)).into(),
                    )
                }
                iced::window::Event::Opened { position, .. }
                    if id == tracked_id =>
                {
                    DomainMessage::Ui(
                        WindowUiMessage::WindowMoved(position).into(),
                    )
                }
                iced::window::Event::Resized(size) if id == tracked_id => {
                    DomainMessage::Ui(
                        WindowUiMessage::WindowResized(size).into(),
                    )
                }
                iced::window::Event::Focused if id == tracked_id => {
                    DomainMessage::Ui(UiShellMessage::MainWindowFocused.into())
                }
                iced::window::Event::Unfocused if id == tracked_id => {
                    DomainMessage::Ui(
                        UiShellMessage::MainWindowUnfocused.into(),
                    )
                }
                _ => DomainMessage::NoOp,
            },
        ));
    }

    if let Some(search_id) = state.search_window_id {
        subs.push(iced::window::events().with(search_id).map(
            |(tracked_id, (id, event))| match event {
                iced::window::Event::Focused if id == tracked_id => {
                    DomainMessage::Ui(UiShellMessage::FocusSearchInput.into())
                }
                _ => DomainMessage::NoOp,
            },
        ));
    }

    if let Some(overlay_id) = state.windows.get(WindowKind::PlayerOverlay) {
        subs.push(iced::window::events().with(overlay_id).map(
            |(tracked_id, (id, event))| match event {
                iced::window::Event::Moved(position) if id == tracked_id => {
                    DomainMessage::Ui(
                        WindowUiMessage::WindowMoved(Some(position)).into(),
                    )
                }
                iced::window::Event::Resized(size) if id == tracked_id => {
                    DomainMessage::Ui(
                        UiShellMessage::PlayerOverlayResized(id, size).into(),
                    )
                }
                iced::window::Event::Focused if id == tracked_id => {
                    DomainMessage::Ui(
                        UiShellMessage::PlayerOverlayFocused.into(),
                    )
                }
                iced::window::Event::Unfocused if id == tracked_id => {
                    DomainMessage::Ui(
                        UiShellMessage::PlayerOverlayUnfocused.into(),
                    )
                }
                _ => DomainMessage::NoOp,
            },
        ));
        // `exit_on_close_request` is disabled for this window so presenter
        // detach and host-lease release run before native destruction.
        if let Some(request) = state.windows.shell_hidden_for_playback() {
            // Include the request in the subscription identity as well as the
            // emitted message. A donor reused by a replacement must not keep
            // the previous request in an otherwise identical mapper.
            subs.push(
                iced::window::close_requests()
                    .with((overlay_id, request))
                    .map(|((tracked_id, tracked_request), id)| {
                        if id == tracked_id {
                            DomainMessage::Ui(
                                UiShellMessage::ClosePlayerOverlay {
                                    request: tracked_request,
                                }
                                .into(),
                            )
                        } else {
                            DomainMessage::NoOp
                        }
                    }),
            );
        }
    }

    Subscription::batch(subs)
}

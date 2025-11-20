use iced::Subscription;

use crate::common::messages::DomainMessage;
use crate::domains::ui::messages::Message as UiMessage;
use crate::domains::ui::windows::WindowKind;
use crate::state::State;

pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subs: Vec<Subscription<DomainMessage>> = Vec::new();

    subs.push(
        iced::window::close_events()
            .map(|id| DomainMessage::Ui(UiMessage::RawWindowClosed(id))),
    );

    // If the main window Id is not known yet (single-window mode),
    // capture the first Opened event and record it.
    if state.windows.get(WindowKind::Main).is_none() {
        subs.push(
            iced::window::events().map(|(id, event)| match event {
                iced::window::Event::Opened { .. } => {
                    DomainMessage::Ui(UiMessage::MainWindowOpened(id))
                }
                _ => DomainMessage::NoOp,
            }),
        );
    }

    if let Some(main_id) = state.windows.get(WindowKind::Main) {
        subs.push(iced::window::events().with(main_id).map(
            |(tracked_id, (id, event))| match event {
                iced::window::Event::Moved(position) if id == tracked_id => {
                    DomainMessage::Ui(UiMessage::WindowMoved(Some(position)))
                }
                iced::window::Event::Opened { position, .. }
                    if id == tracked_id =>
                {
                    DomainMessage::Ui(UiMessage::WindowMoved(position))
                }
                iced::window::Event::Resized(size) if id == tracked_id => {
                    DomainMessage::Ui(UiMessage::WindowResized(size))
                }
                _ => DomainMessage::NoOp,
            },
        ));
    }

    if let Some(search_id) = state.search_window_id {
        subs.push(iced::window::events().with(search_id).map(
            |(tracked_id, (id, event))| match event {
                iced::window::Event::Focused if id == tracked_id => {
                    DomainMessage::Ui(UiMessage::FocusSearchInput)
                }
                _ => DomainMessage::NoOp,
            },
        ));
    }

    Subscription::batch(subs)
}

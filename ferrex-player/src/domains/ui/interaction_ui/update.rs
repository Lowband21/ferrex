use iced::Task;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{
        interaction_ui::InteractionMessage,
        motion_controller,
        update_handlers::{home_focus, scroll_updates},
        utils::bump_keep_alive,
    },
    state::State,
};

pub fn update_interaction_ui(
    state: &mut State,
    message: InteractionMessage,
) -> DomainUpdateResult {
    match message {
        InteractionMessage::TabGridScrolled(viewport) => {
            bump_keep_alive(state);
            let task =
                scroll_updates::handle_tab_grid_scrolled(state, viewport);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        InteractionMessage::KineticScroll(inner) => {
            let task = motion_controller::update::update(state, inner);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        InteractionMessage::DetailViewScrolled(viewport) => {
            DomainUpdateResult::task(
                scroll_updates::handle_detail_view_scrolled(state, viewport)
                    .map(DomainMessage::Ui),
            )
        }
        InteractionMessage::HomeScrolled(viewport) => DomainUpdateResult::task(
            home_focus::handle_home_scrolled(state, viewport)
                .map(DomainMessage::Ui),
        ),
        InteractionMessage::HomeFocusNext => DomainUpdateResult::task(
            home_focus::handle_home_focus_next(state).map(DomainMessage::Ui),
        ),
        InteractionMessage::HomeFocusPrev => DomainUpdateResult::task(
            home_focus::handle_home_focus_prev(state).map(DomainMessage::Ui),
        ),
        InteractionMessage::HomeFocusTick => DomainUpdateResult::task(
            home_focus::handle_home_focus_tick(state).map(DomainMessage::Ui),
        ),
        InteractionMessage::MouseMoved => {
            state
                .domains
                .ui
                .state
                .carousel_focus
                .record_mouse_move(std::time::Instant::now());
            DomainUpdateResult::task(Task::none())
        }
        InteractionMessage::MediaHovered(instance_key) => {
            state.domains.ui.state.hovered_media_id = Some(instance_key);
            DomainUpdateResult::task(Task::none())
        }
        InteractionMessage::MediaUnhovered(instance_key) => {
            if state.domains.ui.state.hovered_media_id.as_ref()
                == Some(&instance_key)
            {
                state.domains.ui.state.hovered_media_id = None;
            }
            DomainUpdateResult::task(Task::none())
        }
    }
}

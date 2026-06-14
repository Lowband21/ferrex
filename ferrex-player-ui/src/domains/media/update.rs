use super::messages::MediaMessage;
use crate::common::{
    messages::{DomainMessage, DomainUpdateResult},
    task::into_iced_task,
};
use crate::domains::ui::view_model_ui::ViewModelMessage;
use ferrex_player_media::update::MediaUpdatePort;

struct PlayerMediaPort;

impl MediaUpdatePort for PlayerMediaPort {
    type AppMessage = DomainMessage;

    fn media_message(message: MediaMessage) -> Self::AppMessage {
        DomainMessage::Media(message)
    }

    fn refresh_view_model_filters() -> Self::AppMessage {
        DomainMessage::Ui(ViewModelMessage::UpdateViewModelFilters.into())
    }
}

/// Handle media domain messages through the extracted media crate.
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_media(
    state: &mut crate::domains::media::MediaDomainState,
    message: MediaMessage,
) -> DomainUpdateResult {
    let result = ferrex_player_media::update::update_media::<PlayerMediaPort>(
        state, message,
    );
    DomainUpdateResult::task(into_iced_task(result.task))
}

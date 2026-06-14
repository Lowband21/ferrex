use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::{
        media::MediaDomainState,
        player::messages::PlayerMessage,
        ui::{UIDomainState, playback_ui::PlaybackMessage, types::ViewState},
    },
    state::State,
};
use ferrex_core::player_prelude::{EpisodeID, MediaID};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_playback::update::{
    PlaybackEpisodeNavigator, PlaybackStartMode, PlaybackUiShell,
    PlaybackUpdateContext, PlaybackUpdatePort, PlaybackWatchProgressPort,
    PlaybackWindowEvent,
};
use std::sync::Arc;

impl PlaybackUiShell for UIDomainState {
    fn is_player_view(&self) -> bool {
        matches!(self.view, ViewState::Player)
    }

    fn set_player_view(&mut self) {
        self.view = ViewState::Player;
    }

    fn set_loading_video_view(&mut self, url: String) {
        self.view = ViewState::LoadingVideo { url };
    }

    fn set_video_error(&mut self, message: String) {
        self.error_message = Some(message.clone());
        self.view = ViewState::VideoError { message };
    }

    fn clear_error(&mut self) {
        self.error_message = None;
    }
}

struct PlayerWatchProgress<'a> {
    media: &'a mut MediaDomainState,
}

impl PlaybackWatchProgressPort for PlayerWatchProgress<'_> {
    fn take_pending_resume_position(&mut self) -> Option<f32> {
        self.media.pending_resume_position.take()
    }
}

struct PlayerEpisodeNavigator {
    accessor: crate::infra::repository::accessor::Accessor<
        crate::infra::repository::accessor::ReadOnly,
    >,
}

impl PlaybackEpisodeNavigator for PlayerEpisodeNavigator {
    fn next_episode(&self, current: EpisodeID) -> Option<EpisodeID> {
        crate::domains::media::selectors::next_episode_by_order_with_repo(
            &self.accessor,
            current,
        )
    }

    fn previous_episode(&self, current: EpisodeID) -> Option<EpisodeID> {
        crate::domains::media::selectors::previous_episode_by_order_with_repo(
            &self.accessor,
            current,
        )
    }
}

struct PlayerPlaybackPort;

impl PlaybackUpdatePort for PlayerPlaybackPort {
    type AppMessage = DomainMessage;

    fn playback_message(message: PlayerMessage) -> Self::AppMessage {
        DomainMessage::Player(message)
    }

    fn send_progress_update(
        media_id: MediaID,
        position: f64,
        duration: f64,
    ) -> Self::AppMessage {
        DomainMessage::Media(
            crate::domains::media::messages::MediaMessage::SendProgressUpdateWithData(
                media_id, position, duration,
            ),
        )
    }

    fn navigate_back() -> Self::AppMessage {
        DomainMessage::Ui(
            crate::domains::ui::shell_ui::UiShellMessage::NavigateBack.into(),
        )
    }

    fn navigate_home() -> Self::AppMessage {
        DomainMessage::Ui(
            crate::domains::ui::shell_ui::UiShellMessage::NavigateHome.into(),
        )
    }

    fn play_media_with_id(
        media_id: MediaID,
        mode: PlaybackStartMode,
    ) -> Self::AppMessage {
        let message = match mode {
            PlaybackStartMode::Internal => {
                PlaybackMessage::PlayMediaWithId(media_id)
            }
            PlaybackStartMode::External => {
                PlaybackMessage::PlayMediaWithIdInMpv(media_id)
            }
        };
        DomainMessage::Ui(crate::domains::ui::messages::UiMessage::Playback(
            message,
        ))
    }
}

/// Handle player domain messages through the extracted playback crate.
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_player(
    state: &mut State,
    message: PlayerMessage,
) -> DomainUpdateResult {
    let navigator = PlayerEpisodeNavigator {
        accessor: state.domains.ui.state.repo_accessor.clone(),
    };

    let api_service: Arc<dyn ApiService> = Arc::clone(&state.api_service);
    let server_url = state.server_url.clone();
    let window_size = state.window_size;
    let window_position = state.window_position;

    let result = {
        let mut watch_progress = PlayerWatchProgress {
            media: &mut state.domains.media.state,
        };
        let mut context = PlaybackUpdateContext {
            playback: &mut state.domains.player.state,
            watch_progress: &mut watch_progress,
            ui: &mut state.domains.ui.state,
            episodes: &navigator,
            api_service,
            server_url: &server_url,
            window_size,
            window_position,
        };

        ferrex_player_playback::update::update_player::<PlayerPlaybackPort>(
            &mut context,
            message,
        )
    };

    let events = result
        .events
        .into_iter()
        .map(|event| match event {
            PlaybackWindowEvent::SetWindowMode(mode) => {
                CrossDomainEvent::SetWindowMode(mode)
            }
            PlaybackWindowEvent::RestoreWindow(fullscreen) => {
                CrossDomainEvent::RestoreWindow(fullscreen)
            }
        })
        .collect();

    DomainUpdateResult::with_events(result.task, events)
}

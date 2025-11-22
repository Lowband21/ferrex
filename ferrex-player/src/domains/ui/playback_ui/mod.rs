pub mod update;

use crate::domains::ui::messages::UiMessage;
use ferrex_core::player_prelude::{MediaID, SeriesID};

pub use update::update_playback_ui;

#[derive(Clone)]
pub enum PlaybackMessage {
    PlayMediaWithId(MediaID),
    PlayMediaWithIdInMpv(MediaID),
    PlaySeriesNextEpisode(SeriesID),
}

impl From<PlaybackMessage> for UiMessage {
    fn from(msg: PlaybackMessage) -> Self {
        UiMessage::Playback(msg)
    }
}

impl PlaybackMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::PlayMediaWithId(_) => "UI::PlayMediaWithId",
            Self::PlayMediaWithIdInMpv(_) => "UI::PlayMediaWithIdInMpv",
            Self::PlaySeriesNextEpisode(_) => "UI::PlaySeriesNextEpisode",
        }
    }
}

impl std::fmt::Debug for PlaybackMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlayMediaWithId(id) => {
                write!(f, "UI::PlayMediaWithId({:?})", id)
            }
            Self::PlayMediaWithIdInMpv(id) => {
                write!(f, "UI::PlayMediaWithIdInMpv({:?})", id)
            }
            Self::PlaySeriesNextEpisode(series) => {
                write!(f, "UI::PlaySeriesNextEpisode({:?})", series)
            }
        }
    }
}

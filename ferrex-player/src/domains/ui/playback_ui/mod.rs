pub mod update;

use crate::domains::ui::messages::UiMessage;
use ferrex_core::player_prelude::{MediaID, SeriesID};

pub use update::update_playback_ui;

#[derive(Clone)]
pub enum PlaybackMessage {
    PlayMediaWithId(MediaID),
    PlayMediaWithIdFromStart(MediaID),
    PlayMediaWithIdInMpv(MediaID),
    PlayMediaWithIdInMpvFromStart(MediaID),
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
            Self::PlayMediaWithIdFromStart(_) => "UI::PlayMediaWithIdFromStart",
            Self::PlayMediaWithIdInMpv(_) => "UI::PlayMediaWithIdInMpv",
            Self::PlayMediaWithIdInMpvFromStart(_) => {
                "UI::PlayMediaWithIdInMpvFromStart"
            }
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
            Self::PlayMediaWithIdFromStart(id) => {
                write!(f, "UI::PlayMediaWithIdFromStart({:?})", id)
            }
            Self::PlayMediaWithIdInMpv(id) => {
                write!(f, "UI::PlayMediaWithIdInMpv({:?})", id)
            }
            Self::PlayMediaWithIdInMpvFromStart(id) => {
                write!(f, "UI::PlayMediaWithIdInMpvFromStart({:?})", id)
            }
            Self::PlaySeriesNextEpisode(series) => {
                write!(f, "UI::PlaySeriesNextEpisode({:?})", series)
            }
        }
    }
}

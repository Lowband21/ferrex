//! Audio and subtitle track-selection helpers.
//!
//! Helpers in this module format Ferrex-owned track models and update
//! `PlayerDomainState` without exposing backend track DTOs.

use std::time::Duration;

use super::{
    contract::{
        AudioTrack, Chapter, ChapterId, Edition, EditionId, SubtitleTrack,
        TrackId,
    },
    state::PlayerDomainState,
};

fn subtitle_enable_target(
    current: Option<&TrackId>,
    last: Option<&TrackId>,
    available: &[SubtitleTrack],
) -> Option<TrackId> {
    current
        .or(last)
        .cloned()
        .filter(|target| available.iter().any(|track| &track.id == target))
        .or_else(|| available.first().map(|track| track.id.clone()))
}

impl PlayerDomainState {
    /// Query and update available tracks from the active adapter.
    pub fn update_available_tracks(&mut self) {
        if let Some(video) = &mut self.video_opt {
            let catalog = video.refresh_tracks();
            self.track_catalog_generation = Some(video.snapshot().generation);
            self.current_audio_track = catalog.selected_audio.clone();
            self.current_subtitle_track = catalog.selected_subtitle.clone();
            self.subtitles_enabled = catalog.selected_subtitle.is_some()
                || video.subtitles_enabled();
            self.available_audio_tracks = catalog.audio;
            self.available_subtitle_tracks = catalog.subtitles;

            log::info!(
                "Available audio tracks: {}",
                self.available_audio_tracks.len()
            );
            log::info!(
                "Available subtitle tracks: {}",
                self.available_subtitle_tracks.len()
            );
        }
    }

    /// Select an audio track by stable Ferrex identity.
    pub fn select_audio_track(
        &mut self,
        track_id: TrackId,
    ) -> Result<(), String> {
        let track_name = self.format_audio_track(&track_id);
        let video = self
            .video_opt
            .as_mut()
            .ok_or_else(|| "No video loaded".to_string())?;
        video.select_audio_track(&track_id).map_err(|error| {
            format!("Failed to select audio track {track_id}: {error}")
        })?;
        self.current_audio_track = Some(track_id);
        self.show_track_notification(format!("Audio: {track_name}"));
        Ok(())
    }

    /// Select a subtitle track by identity, or `None` to disable subtitles.
    pub fn select_subtitle_track(
        &mut self,
        track_id: Option<TrackId>,
    ) -> Result<(), String> {
        log::info!("Selecting subtitle track: {track_id:?}");
        let message = track_id.as_ref().map_or_else(
            || "Subtitles: Disabled".to_string(),
            |track_id| {
                format!("Subtitles: {}", self.format_subtitle_track(track_id))
            },
        );

        let video = self
            .video_opt
            .as_mut()
            .ok_or_else(|| "No video loaded".to_string())?;
        video
            .select_subtitle_track(track_id.as_ref())
            .map_err(|error| {
                format!("Failed to select subtitle track {track_id:?}: {error}")
            })?;

        if let Some(selected) = track_id.as_ref() {
            self.last_subtitle_track = Some(selected.clone());
        } else if let Some(previous) = self.current_subtitle_track.as_ref() {
            self.last_subtitle_track = Some(previous.clone());
        }
        self.subtitles_enabled = track_id.is_some();
        self.current_subtitle_track = track_id;
        self.show_track_notification(message);
        Ok(())
    }

    /// Select a chapter by its stable Ferrex identity.
    pub fn select_chapter(
        &mut self,
        chapter_id: ChapterId,
    ) -> Result<(), String> {
        let chapter_name = self
            .playback_snapshot()
            .and_then(|snapshot| {
                snapshot
                    .chapters
                    .iter()
                    .enumerate()
                    .find(|(_, chapter)| chapter.id == chapter_id)
            })
            .map(|(index, chapter)| format_chapter(chapter, index))
            .ok_or_else(|| {
                format!("Unknown chapter {}", chapter_id.as_str())
            })?;
        let video = self
            .video_opt
            .as_mut()
            .ok_or_else(|| "No video loaded".to_string())?;
        video.select_chapter(&chapter_id).map_err(|error| {
            format!("Failed to select chapter {}: {error}", chapter_id.as_str())
        })?;
        self.show_track_notification(format!("Chapter: {chapter_name}"));
        Ok(())
    }

    /// Select a media edition by its stable Ferrex identity.
    pub fn select_edition(
        &mut self,
        edition_id: EditionId,
    ) -> Result<(), String> {
        let edition_name = self
            .playback_snapshot()
            .and_then(|snapshot| {
                snapshot
                    .editions
                    .iter()
                    .enumerate()
                    .find(|(_, edition)| edition.id == edition_id)
            })
            .map(|(index, edition)| format_edition(edition, index))
            .ok_or_else(|| {
                format!("Unknown edition {}", edition_id.as_str())
            })?;
        let video = self
            .video_opt
            .as_mut()
            .ok_or_else(|| "No video loaded".to_string())?;
        video.select_edition(&edition_id).map_err(|error| {
            format!("Failed to select edition {}: {error}", edition_id.as_str())
        })?;
        self.show_track_notification(format!("Edition: {edition_name}"));
        Ok(())
    }

    /// Toggle subtitles on/off.
    pub fn toggle_subtitles(&mut self) -> Result<(), String> {
        if self.video_opt.is_none() {
            return Err("No video loaded".to_string());
        }

        if self.subtitles_enabled {
            return self.select_subtitle_track(None);
        }

        let target = subtitle_enable_target(
            self.current_subtitle_track.as_ref(),
            self.last_subtitle_track.as_ref(),
            &self.available_subtitle_tracks,
        );

        if let Some(track_id) = target {
            self.select_subtitle_track(Some(track_id))
        } else {
            let video = self
                .video_opt
                .as_mut()
                .ok_or_else(|| "No video loaded".to_string())?;
            video.set_subtitles_enabled(true);
            self.subtitles_enabled = true;
            self.show_track_notification("Subtitles: On".to_string());
            Ok(())
        }
    }

    /// Cycle to the next audio track.
    pub fn cycle_audio_track(&mut self) -> Result<(), String> {
        if self.available_audio_tracks.is_empty() {
            return Err("No audio tracks available".to_string());
        }

        let current_index = self.current_audio_track.as_ref().and_then(|id| {
            self.available_audio_tracks
                .iter()
                .position(|track| &track.id == id)
        });
        let next_index = current_index
            .map(|index| (index + 1) % self.available_audio_tracks.len())
            .unwrap_or(0);
        self.select_audio_track(
            self.available_audio_tracks[next_index].id.clone(),
        )
    }

    /// Cycle to the next subtitle track (including disabled).
    pub fn cycle_subtitle_track(&mut self) -> Result<(), String> {
        if self.available_subtitle_tracks.is_empty() {
            return Ok(());
        }

        let next = match self.current_subtitle_track.as_ref().and_then(|id| {
            self.available_subtitle_tracks
                .iter()
                .position(|track| &track.id == id)
        }) {
            None => Some(self.available_subtitle_tracks[0].id.clone()),
            Some(index) if index + 1 < self.available_subtitle_tracks.len() => {
                Some(self.available_subtitle_tracks[index + 1].id.clone())
            }
            Some(_) => None,
        };

        self.select_subtitle_track(next)
    }

    /// Simple subtitle cycling: off -> first/last-used -> off.
    pub fn cycle_subtitle_simple(&mut self) -> Result<(), String> {
        if self.available_subtitle_tracks.is_empty() {
            return Ok(());
        }

        if self.subtitles_enabled {
            if let Some(current) = self.current_subtitle_track.clone() {
                self.last_subtitle_track = Some(current);
            }
            self.select_subtitle_track(None)
        } else {
            let target =
                self.last_subtitle_track.clone().unwrap_or_else(|| {
                    self.available_subtitle_tracks[0].id.clone()
                });
            self.select_subtitle_track(Some(target))
        }
    }

    pub fn format_audio_track(&self, track_id: &TrackId) -> String {
        self.available_audio_tracks
            .iter()
            .find(|track| &track.id == track_id)
            .map(format_audio_track)
            .unwrap_or_else(|| format!("Track {track_id}"))
    }

    pub fn format_subtitle_track(&self, track_id: &TrackId) -> String {
        self.available_subtitle_tracks
            .iter()
            .find(|track| &track.id == track_id)
            .map(format_subtitle_track)
            .unwrap_or_else(|| format!("Track {track_id}"))
    }
}

/// Return the chapter containing an observed playback position.
pub fn chapter_at_position(
    chapters: &[Chapter],
    position: Duration,
) -> Option<&Chapter> {
    chapters.iter().rev().find(|chapter| {
        chapter.start <= position
            && chapter.end.is_none_or(|end| position < end)
    })
}

/// Format a chapter for display.
pub fn format_chapter(chapter: &Chapter, index: usize) -> String {
    let title = chapter
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Chapter {}", index + 1));
    format!("{title} ({})", format_structure_time(chapter.start))
}

/// Format an edition for display.
pub fn format_edition(edition: &Edition, index: usize) -> String {
    let mut title = edition
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Edition {}", index + 1));
    if edition.is_default {
        title.push_str(" (Default)");
    }
    title
}

fn format_structure_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Format an audio track for display.
pub fn format_audio_track(track: &AudioTrack) -> String {
    let mut parts = Vec::new();

    if let Some(language) = &track.language {
        parts.push(format_language_code(language));
    } else if let Some(title) = &track.title {
        parts.push(title.clone());
    } else {
        parts.push(track.id.to_string());
    }

    let mut details = Vec::new();
    if let Some(codec) = &track.codec {
        details.push(format_audio_codec(codec));
    }
    if let Some(channels) = track.channels {
        details.push(format_channels(channels));
    }

    if !details.is_empty() {
        parts.push(format!("({})", details.join(" ")));
    }

    parts.join(" ")
}

/// Format a subtitle track for display.
pub fn format_subtitle_track(track: &SubtitleTrack) -> String {
    let mut parts = Vec::new();

    if let Some(language) = &track.language {
        parts.push(format_language_code(language));
    } else if let Some(title) = &track.title {
        parts.push(title.clone());
    } else {
        parts.push(track.id.to_string());
    }

    if let Some(codec) = &track.codec {
        parts.push(format!("({})", format_subtitle_codec(codec)));
    }

    parts.join(" ")
}

fn format_language_code(code: &str) -> String {
    match code.to_lowercase().as_str() {
        "en" | "eng" => "English",
        "es" | "spa" => "Spanish",
        "fr" | "fra" => "French",
        "de" | "deu" | "ger" => "German",
        "it" | "ita" => "Italian",
        "pt" | "por" => "Portuguese",
        "ru" | "rus" => "Russian",
        "ja" | "jpn" => "Japanese",
        "zh" | "chi" | "zho" => "Chinese",
        "ko" | "kor" => "Korean",
        "ar" | "ara" => "Arabic",
        "hi" | "hin" => "Hindi",
        "nl" | "nld" | "dut" => "Dutch",
        "sv" | "swe" => "Swedish",
        "no" | "nor" => "Norwegian",
        "da" | "dan" => "Danish",
        "fi" | "fin" => "Finnish",
        "pl" | "pol" => "Polish",
        "tr" | "tur" => "Turkish",
        "el" | "ell" | "gre" => "Greek",
        "he" | "heb" => "Hebrew",
        _ => code,
    }
    .to_string()
}

fn format_audio_codec(codec: &str) -> String {
    match codec.to_lowercase().as_str() {
        codec if codec.contains("aac") => "AAC",
        codec if codec.contains("ac3") || codec.contains("ac-3") => "AC3",
        codec if codec.contains("eac3") || codec.contains("eac-3") => "E-AC3",
        codec if codec.contains("dts") => "DTS",
        codec if codec.contains("truehd") => "TrueHD",
        codec if codec.contains("mp3") => "MP3",
        codec if codec.contains("opus") => "Opus",
        codec if codec.contains("vorbis") => "Vorbis",
        codec if codec.contains("flac") => "FLAC",
        codec if codec.contains("pcm") => "PCM",
        _ => codec,
    }
    .to_string()
}

fn format_subtitle_codec(codec: &str) -> String {
    match codec.to_lowercase().as_str() {
        codec if codec.contains("srt") => "SRT",
        codec if codec.contains("webvtt") || codec.contains("vtt") => "WebVTT",
        codec if codec.contains("ass") || codec.contains("ssa") => "ASS/SSA",
        codec if codec.contains("pgs") => "PGS",
        codec if codec.contains("dvb") => "DVB",
        codec if codec.contains("dvd") => "DVD",
        _ => codec,
    }
    .to_string()
}

fn format_channels(channels: u16) -> String {
    match channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        _ => format!("{channels} ch"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::SubtitleKind;

    fn subtitle(id: &str) -> SubtitleTrack {
        SubtitleTrack {
            id: TrackId::new(id),
            title: None,
            language: None,
            codec: None,
            kind: SubtitleKind::Unknown,
            is_default: false,
            is_forced: false,
            is_external: false,
        }
    }

    #[test]
    fn subtitle_enable_restores_last_available_selection() {
        let english = subtitle("subtitle:eng");
        let japanese = subtitle("subtitle:jpn");
        let available = vec![english.clone(), japanese.clone()];

        assert_eq!(
            subtitle_enable_target(None, Some(&japanese.id), &available,),
            Some(japanese.id.clone())
        );
        assert_eq!(
            subtitle_enable_target(
                None,
                Some(&TrackId::new("subtitle:removed")),
                &available,
            ),
            Some(english.id)
        );
    }

    #[test]
    fn chapter_projection_uses_half_open_boundaries() {
        let chapters = vec![
            Chapter {
                id: ChapterId::new("chapter:one"),
                title: Some("Opening".to_string()),
                start: Duration::ZERO,
                end: Some(Duration::from_secs(10)),
            },
            Chapter {
                id: ChapterId::new("chapter:two"),
                title: None,
                start: Duration::from_secs(10),
                end: None,
            },
        ];

        assert_eq!(
            chapter_at_position(&chapters, Duration::from_millis(9_999))
                .map(|chapter| &chapter.id),
            Some(&chapters[0].id)
        );
        assert_eq!(
            chapter_at_position(&chapters, Duration::from_secs(10))
                .map(|chapter| &chapter.id),
            Some(&chapters[1].id)
        );
        assert_eq!(format_chapter(&chapters[1], 1), "Chapter 2 (00:10)");
    }
}

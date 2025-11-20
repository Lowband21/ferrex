use super::state::PlayerDomainState;
use subwave_core::video::types::{AudioTrack, SubtitleTrack};

impl PlayerDomainState {
    /// Query and update available tracks from the video
    pub fn update_available_tracks(&mut self) {
        if let Some(video) = &mut self.video_opt {
            // Query audio tracks
            self.available_audio_tracks = video.audio_tracks();
            self.current_audio_track = video.current_audio_track();

            // Query subtitle tracks
            self.available_subtitle_tracks = video.subtitle_tracks();
            self.current_subtitle_track = video.current_subtitle_track();
            self.subtitles_enabled = video.subtitles_enabled();

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

    /// Select an audio track by index
    pub fn select_audio_track(&mut self, index: i32) -> Result<(), String> {
        if let Some(video) = &mut self.video_opt {
            if let Err(e) = video.select_audio_track(index) {
                return Err(format!("Failed to select audio track {}: {}", index, e));
            }
            self.current_audio_track = index;

            // Show notification
            let track_name = self.format_audio_track(index);
            self.show_track_notification(format!("Audio: {}", track_name));

            Ok(())
        } else {
            Err("No video loaded".to_string())
        }
    }

    /// Select a subtitle track by index, or None to disable
    pub fn select_subtitle_track(&mut self, index: Option<i32>) -> Result<(), String> {
        if let Some(video) = &mut self.video_opt {
            log::info!("Selecting subtitle track: {:?}", index);

            // Debug: print available tracks
            if let Some(idx) = index {
                if let Some(track) = self.available_subtitle_tracks.get(idx as usize) {
                    log::info!(
                        "Track {} details: lang={:?}, codec={:?}, title={:?}",
                        idx,
                        track.language,
                        track.codec,
                        track.title
                    );
                }
            }

            if let Err(e) = video.select_subtitle_track(index) {
                return Err(format!("Failed to select subtitle track {:?}: {}", index, e));
            }
            self.current_subtitle_track = index;

            // Update subtitle enabled state based on selection without sending a second SelectStreams
            // event. select_subtitle_track() already applied the change at the backend.
            self.subtitles_enabled = index.is_some();
            if self.subtitles_enabled {
                log::info!("Subtitles Enabled for track {:?}", index);
            } else {
                log::info!("Subtitles Disabled");
            }

            // Keep overlay (if present) in sync for Wayland backend
            if let Some(overlay) = self.overlay.as_ref() {
                overlay.select_subtitle_index(index);
            }

            // Show notification
            let message = if let Some(idx) = index {
                let track_name = self.format_subtitle_track(idx);
                format!("Subtitles: {}", track_name)
            } else {
                "Subtitles: Disabled".to_string()
            };
            self.show_track_notification(message);

            Ok(())
        } else {
            Err("No video loaded".to_string())
        }
    }

    /// Toggle subtitles on/off
    pub fn toggle_subtitles(&mut self) -> Result<(), String> {
        if let Some(video) = &mut self.video_opt {
            let enable = !self.subtitles_enabled;
            if enable {
                // When enabling via toggle, ensure a concrete track is selected for reliable behavior
                let target_index = if let Some(cur) = self.current_subtitle_track {
                    Some(cur)
                } else if !self.available_subtitle_tracks.is_empty() {
                    Some(0)
                } else {
                    None
                };

                if let Some(idx) = target_index {
                    // This updates backend and UI state, and shows the toast
                    self.select_subtitle_track(Some(idx))?;
                } else {
                    // No tracks, just set enabled flag
                    self.subtitles_enabled = true;
                    video.set_subtitles_enabled(true);
                    self.show_track_notification("Subtitles: On".to_string());
                }
            } else {
                // Disable reliably by selecting None (updates backend and UI state)
                self.select_subtitle_track(None)?;
                // Drop overlay when disabling; creation handled elsewhere if needed
                self.overlay = None;
            }
            Ok(())
        } else {
            Err("No video loaded".to_string())
        }
    }

    /// Cycle to the next audio track
    pub fn cycle_audio_track(&mut self) -> Result<(), String> {
        if self.available_audio_tracks.is_empty() {
            return Err("No audio tracks available".to_string());
        }

        let next_index = (self.current_audio_track + 1) % self.available_audio_tracks.len() as i32;
        self.select_audio_track(next_index)
    }

    /// Cycle to the next subtitle track (including None)
    pub fn cycle_subtitle_track(&mut self) -> Result<(), String> {
        if self.available_subtitle_tracks.is_empty() {
            return Ok(()); // No subtitle tracks, nothing to cycle
        }

        let next_index = match self.current_subtitle_track {
            None => Some(0), // Start with first track
            Some(idx) => {
                let next = idx + 1;
                if next >= self.available_subtitle_tracks.len() as i32 {
                    None // Wrap to "Off"
                } else {
                    Some(next)
                }
            }
        };

        self.select_subtitle_track(next_index)
    }

    /// Simple subtitle cycling: Off -> First -> Off -> Last Used -> Off
    pub fn cycle_subtitle_simple(&mut self) -> Result<(), String> {
        if self.available_subtitle_tracks.is_empty() {
            return Ok(()); // No subtitle tracks, nothing to cycle
        }

        let next_state = if !self.subtitles_enabled {
            // Currently off -> Enable with first track
            Some(0)
        } else if self.current_subtitle_track == Some(0) {
            // Currently showing first track -> Turn off and remember this track
            self.last_subtitle_track = Some(0);
            None
        } else if self.current_subtitle_track.is_none() && self.last_subtitle_track.is_some() {
            // Currently off but we have a last track -> Restore last track
            self.last_subtitle_track
        } else {
            // Any other state -> Turn off
            if let Some(current) = self.current_subtitle_track {
                self.last_subtitle_track = Some(current);
            }
            None
        };

        self.select_subtitle_track(next_state)
    }

    /// Format audio track for display
    pub fn format_audio_track(&self, index: i32) -> String {
        if let Some(track) = self.available_audio_tracks.get(index as usize) {
            format_audio_track(track)
        } else {
            format!("Track {}", index + 1)
        }
    }

    /// Format subtitle track for display
    pub fn format_subtitle_track(&self, index: i32) -> String {
        if let Some(track) = self.available_subtitle_tracks.get(index as usize) {
            format_subtitle_track(track)
        } else {
            format!("Track {}", index + 1)
        }
    }
}

/// Format an audio track for display
pub fn format_audio_track(track: &AudioTrack) -> String {
    let mut parts = Vec::new();

    // Add language or title
    if let Some(lang) = &track.language {
        parts.push(format_language_code(lang));
    } else if let Some(title) = &track.title {
        parts.push(title.clone());
    } else {
        parts.push(format!("Track {}", track.index + 1));
    }

    // Add codec and channel info in parentheses
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

/// Format a subtitle track for display
pub fn format_subtitle_track(track: &SubtitleTrack) -> String {
    let mut parts = Vec::new();

    // Add language or title
    if let Some(lang) = &track.language {
        parts.push(format_language_code(lang));
    } else if let Some(title) = &track.title {
        parts.push(title.clone());
    } else {
        parts.push(format!("Track {}", track.index + 1));
    }

    // Add codec in parentheses
    if let Some(codec) = &track.codec {
        parts.push(format!("({})", format_subtitle_codec(codec)));
    }

    parts.join(" ")
}

/// Convert language code to human-readable name
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

/// Format audio codec name
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

/// Format subtitle codec name
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

/// Format channel configuration
fn format_channels(channels: i32) -> String {
    match channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        _ => format!("{} ch", channels),
    }
}

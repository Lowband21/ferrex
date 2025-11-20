use crate::{MediaError, MediaMetadata, MediaType, ParsedMediaInfo, Result, TvParser, ExtrasParser, LibraryType};
use ffmpeg_next as ffmpeg;
use regex::Regex;
use std::path::Path;
use tracing::{debug, info};

pub struct MetadataExtractor {
    /// Whether FFmpeg has been initialized
    initialized: bool,
    /// Library context for type-specific parsing
    library_type: Option<LibraryType>,
}

impl Default for MetadataExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataExtractor {
    pub fn new() -> Self {
        Self { 
            initialized: false,
            library_type: None,
        }
    }

    /// Create a new extractor with library context
    pub fn with_library_type(library_type: LibraryType) -> Self {
        Self {
            initialized: false,
            library_type: Some(library_type),
        }
    }

    /// Set library type for context-aware parsing
    pub fn set_library_type(&mut self, library_type: Option<LibraryType>) {
        self.library_type = library_type;
    }

    /// Check if a file is likely a sample based on duration and file size
    pub fn is_sample(&self, metadata: &MediaMetadata) -> bool {
        // Define thresholds for sample detection
        const MAX_SAMPLE_DURATION_SECONDS: f64 = 180.0; // 2 minutes
        const MAX_SAMPLE_SIZE_BYTES: u64 = 50 * 1024 * 1024; // 50 MB
        const MIN_SAMPLE_DURATION_SECONDS: f64 = 10.0; // 10 seconds minimum for samples

        // Check duration threshold
        if let Some(duration) = metadata.duration {
            // Very short files (under 10 seconds) are likely not samples but other media
            if duration < MIN_SAMPLE_DURATION_SECONDS {
                return false;
            }

            // Files between 10 seconds and 2 minutes are likely samples
            if duration <= MAX_SAMPLE_DURATION_SECONDS {
                return true;
            }
        }

        // Check file size threshold for very small files
        if metadata.file_size <= MAX_SAMPLE_SIZE_BYTES {
            // If we have duration info and it's reasonable, don't filter by size alone
            if let Some(duration) = metadata.duration {
                // Allow small files if they're short but not sample-short
                if duration > MAX_SAMPLE_DURATION_SECONDS {
                    return false;
                }
            } else {
                // No duration info and small file - likely a sample
                return true;
            }
        }

        false
    }

    /// Initialize FFmpeg (call once per application)
    pub fn init(&mut self) -> Result<()> {
        if !self.initialized {
            ffmpeg::init().map_err(MediaError::Ffmpeg)?;
            self.initialized = true;
            info!("FFmpeg initialized successfully");
        }
        Ok(())
    }

    /// Extract complete metadata from a media file
    pub fn extract_metadata<P: AsRef<Path>>(&mut self, file_path: P) -> Result<MediaMetadata> {
        let file_path = file_path.as_ref();

        // Ensure FFmpeg is initialized
        self.init()?;

        info!("Extracting metadata from: {}", file_path.display());

        // Extract technical metadata with FFmpeg
        let technical_metadata = self.extract_ffmpeg_metadata(file_path)?;

        // Parse filename for show/episode info
        let parsed_info = self.parse_filename_with_type(file_path);

        // Get file size
        let file_size = file_path.metadata().map_err(MediaError::Io)?.len();

        Ok(MediaMetadata {
            duration: technical_metadata.duration,
            width: technical_metadata.width,
            height: technical_metadata.height,
            video_codec: technical_metadata.video_codec,
            audio_codec: technical_metadata.audio_codec,
            bitrate: technical_metadata.bitrate,
            framerate: technical_metadata.framerate,
            file_size,
            parsed_info,
            external_info: None, // Will be populated by future database lookup
        })
    }

    /// Extract technical metadata using FFmpeg
    fn extract_ffmpeg_metadata<P: AsRef<Path>>(&self, file_path: P) -> Result<TechnicalMetadata> {
        let file_path = file_path.as_ref();

        debug!("Opening file with FFmpeg: {}", file_path.display());

        let input = ffmpeg::format::input(file_path).map_err(MediaError::Ffmpeg)?;

        let mut technical = TechnicalMetadata::default();

        // Get duration
        if input.duration() != ffmpeg::ffi::AV_NOPTS_VALUE {
            technical.duration = Some(input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64);
        }

        // Get bitrate
        if input.bit_rate() > 0 {
            technical.bitrate = Some(input.bit_rate() as u64);
        }

        // Find video and audio streams - prioritize main streams over thumbnails
        let mut best_video_stream = None;
        let mut best_video_width = 0;
        let mut best_audio_stream = None;

        for (i, stream) in input.streams().enumerate() {
            let codec = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(MediaError::Ffmpeg)?;

            match codec.medium() {
                ffmpeg::media::Type::Video => {
                    if let Ok(video) = codec.decoder().video() {
                        let width = video.width();
                        let height = video.height();
                        let codec_name = video
                            .codec()
                            .map(|c| c.name().to_string())
                            .unwrap_or_default();

                        debug!(
                            "Found video stream {} - {}x{} {}",
                            i, width, height, codec_name
                        );

                        // Skip thumbnail streams (MJPEG, small dimensions, attached pictures)
                        let is_thumbnail = codec_name == "mjpeg"
                            || width < 400
                            || height < 400
                            || stream
                                .disposition()
                                .contains(ffmpeg::format::stream::Disposition::ATTACHED_PIC);

                        // Use this stream if it's not a thumbnail and has better resolution
                        if !is_thumbnail && width > best_video_width {
                            best_video_width = width;
                            best_video_stream = Some((stream, video));
                        }
                    }
                }
                ffmpeg::media::Type::Audio => {
                    debug!("Found audio stream {}", i);

                    if let Ok(audio) = codec.decoder().audio() {
                        // Use first audio stream (could be improved to select best quality)
                        if best_audio_stream.is_none() {
                            best_audio_stream = Some(audio);
                        }
                    }
                }
                _ => {
                    debug!("Found other stream type: {:?}", codec.medium());
                }
            }
        }

        // Extract metadata from the best video stream
        if let Some((stream, video)) = best_video_stream {
            technical.width = Some(video.width());
            technical.height = Some(video.height());

            // Get frame rate
            let frame_rate = stream.avg_frame_rate();
            if frame_rate.denominator() != 0 {
                technical.framerate =
                    Some(frame_rate.numerator() as f64 / frame_rate.denominator() as f64);
            }

            // Get codec name
            if let Some(codec) = video.codec() {
                technical.video_codec = Some(codec.name().to_string());
            }

            debug!(
                "Selected video stream: {}x{} {}",
                video.width(),
                video.height(),
                technical
                    .video_codec
                    .as_ref()
                    .unwrap_or(&"unknown".to_string())
            );
        }

        // Extract metadata from the best audio stream
        if let Some(audio) = best_audio_stream {
            if let Some(codec) = audio.codec() {
                technical.audio_codec = Some(codec.name().to_string());
            }
        }

        debug!("Technical metadata extracted: {:?}", technical);
        Ok(technical)
    }

    /// Parse filename with library type context
    fn parse_filename_with_type<P: AsRef<Path>>(&self, file_path: P) -> Option<ParsedMediaInfo> {
        let file_path = file_path.as_ref();
        
        // First check if this is an extra
        if let Some(extra_type) = ExtrasParser::parse_extra_info(file_path) {
            let filename = file_path.file_stem()?.to_str()?;
            let parent_title = ExtrasParser::extract_parent_title(file_path);
            
            return Some(ParsedMediaInfo {
                media_type: MediaType::Extra,
                title: self.clean_filename(filename),
                year: self.extract_year(filename),
                show_name: None,
                season: None,
                episode: None,
                episode_title: None,
                extra_type: Some(extra_type),
                parent_title,
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            });
        }
        
        // Use ExtrasParser for better type detection (which includes extras detection)
        let media_type = ExtrasParser::determine_media_type(file_path, self.library_type.as_ref());
        
        match media_type {
            MediaType::TvEpisode => {
                // Use TvParser for episode parsing
                if let Some(episode_info) = TvParser::parse_episode_info(file_path) {
                    let show_name = TvParser::extract_series_name(file_path)
                        .or_else(|| self.extract_show_name_from_path(file_path));
                    let episode_title = TvParser::extract_episode_title(file_path);
                    let filename = file_path.file_stem()?.to_str()?;
                    
                    return Some(ParsedMediaInfo {
                        media_type: MediaType::TvEpisode,
                        title: episode_title.clone().unwrap_or_else(|| self.clean_filename(filename)),
                        year: self.extract_year(filename),
                        show_name,
                        season: Some(episode_info.season),
                        episode: Some(episode_info.episode),
                        episode_title,
                        extra_type: None,
                        parent_title: None,
                        resolution: self.extract_resolution(filename),
                        source: self.extract_source(filename),
                        release_group: self.extract_release_group(filename),
                    });
                }
            }
            MediaType::Movie => {
                return self.parse_as_movie(file_path.file_stem()?.to_str()?, file_path);
            }
            MediaType::Extra => {
                // This should have been caught above, but handle it here too
                let filename = file_path.file_stem()?.to_str()?;
                let extra_type = ExtrasParser::parse_extra_info(file_path);
                let parent_title = ExtrasParser::extract_parent_title(file_path);
                
                return Some(ParsedMediaInfo {
                    media_type: MediaType::Extra,
                    title: self.clean_filename(filename),
                    year: self.extract_year(filename),
                    show_name: None,
                    season: None,
                    episode: None,
                    episode_title: None,
                    extra_type,
                    parent_title,
                    resolution: self.extract_resolution(filename),
                    source: self.extract_source(filename),
                    release_group: self.extract_release_group(filename),
                });
            }
            _ => {}
        }
        
        // Fallback to old parsing logic
        self.parse_filename(file_path)
    }

    /// Parse filename to extract show/episode information (legacy method)
    fn parse_filename<P: AsRef<Path>>(&self, file_path: P) -> Option<ParsedMediaInfo> {
        let file_path = file_path.as_ref();
        let filename = file_path.file_stem()?.to_str()?;

        info!("=== METADATA PARSING ===");
        info!("Full path: {:?}", file_path);
        info!("Filename: {}", filename);

        // First, check folder structure to determine media type
        let path_str = file_path.to_string_lossy();
        info!("Path string: {}", path_str);

        // Check for folder names case-insensitively
        let path_lower = path_str.to_lowercase();
        let is_in_movies_folder =
            path_lower.contains("/movies/") || path_lower.contains("\\movies\\");
        let is_in_tvshows_folder = path_lower.contains("/tvshows/")
            || path_lower.contains("\\tvshows\\")
            || path_lower.contains("/tv shows/")
            || path_lower.contains("\\tv shows\\")
            || path_lower.contains("/tv-shows/")
            || path_lower.contains("\\tv-shows\\")
            || path_lower.contains("/series/")
            || path_lower.contains("\\series\\");

        info!("Is in movies folder: {}", is_in_movies_folder);
        info!("Is in tvshows folder: {}", is_in_tvshows_folder);

        // If in movies folder, parse as movie
        if is_in_movies_folder {
            info!("File is in movies folder, parsing as movie");
            return self.parse_as_movie(filename, file_path);
        }

        // If in tvshows folder, parse as TV show
        if is_in_tvshows_folder {
            info!("File is in tvshows folder, parsing as TV show");

            // Try to extract show name from path first
            let show_name_from_path = self.extract_show_name_from_path(file_path);
            info!("Show name extracted from path: {:?}", show_name_from_path);

            if let Some(mut tv_info) = self.parse_tv_episode(filename) {
                info!("Successfully parsed TV episode pattern from filename");
                // If we got a show name from path, use it (it's more reliable than filename parsing)
                if let Some(path_show_name) = show_name_from_path {
                    tv_info.show_name = Some(path_show_name);
                }
                info!("Final TV info: {:?}", tv_info);
                return Some(tv_info);
            }

            // If TV parsing fails but we're in TV folder, create a basic TV episode entry
            info!("TV pattern parsing failed, creating basic TV episode entry");
            let show_name = show_name_from_path.unwrap_or_else(|| self.clean_filename(filename));

            // Try to extract season from folder path
            let season = self.extract_season_from_path(file_path);
            info!("Extracted season from path: {:?}", season);

            let cleaned_title = self.clean_filename(filename);
            let tv_info = ParsedMediaInfo {
                media_type: MediaType::TvEpisode,
                title: cleaned_title,
                year: self.extract_year(filename),
                show_name: Some(show_name),
                season,
                episode: self.extract_episode_number_from_filename(filename),
                episode_title: None,
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            };
            info!("Created basic TV info: {:?}", tv_info);
            return Some(tv_info);
        }

        // If not in a specific folder, try to detect based on patterns
        // Try TV show pattern first (SxxExx format)
        if let Some(tv_info) = self.parse_tv_episode(filename) {
            return Some(tv_info);
        }

        // Default to movie
        self.parse_as_movie(filename, file_path)
    }

    /// Extract show name from folder path structure
    fn extract_show_name_from_path(&self, file_path: &Path) -> Option<String> {
        // Try to extract show name from path like /tvshows/Show Name/Season X/file.mkv
        let path_str = file_path.to_string_lossy();
        let path_lower = path_str.to_lowercase();

        info!("Extracting show name from path: {}", path_str);

        // Find the position of TV folder variations in the path (case-insensitive)
        let tv_folder_patterns = vec![
            ("/tvshows/", "\\tvshows\\"),
            ("/tv shows/", "\\tv shows\\"),
            ("/tv-shows/", "\\tv-shows\\"),
            ("/series/", "\\series\\"),
        ];

        for (unix_pattern, win_pattern) in tv_folder_patterns {
            if let Some(pos) = path_lower
                .find(unix_pattern)
                .or_else(|| path_lower.find(win_pattern))
            {
                // Get the actual case-sensitive path part after the TV folder
                let pattern_len = unix_pattern.len();
                let after_tv_folder = &path_str[pos + pattern_len..];

                // Get the first directory after tvshows - this should be the show name
                let parts: Vec<&str> = after_tv_folder.split(&['/', '\\'][..]).collect();
                if !parts.is_empty() && !parts[0].is_empty() {
                    let mut show_name = parts[0].to_string();

                    // Clean up show name - remove year in parentheses if present
                    // This is important for TMDB searches
                    if let Some(year_match) =
                        Regex::new(r"\s*\(\d{4}\)\s*$").unwrap().find(&show_name)
                    {
                        show_name = show_name[..year_match.start()].to_string();
                        info!("Removed year from show name for cleaner search");
                    }

                    info!("Extracted show name: {}", show_name);
                    return Some(show_name);
                }
            }
        }

        info!("Could not extract show name from path");
        None
    }

    /// Extract season number from folder path
    fn extract_season_from_path(&self, file_path: &Path) -> Option<u32> {
        let path_str = file_path.to_string_lossy();

        // Look for patterns like "Season 1", "Season 01", "S1", "S01" in the path
        let season_patterns = vec![
            Regex::new(r"[/\\][Ss]eason\s*(\d{1,2})[/\\]").unwrap(),
            Regex::new(r"[/\\][Ss](\d{1,2})[/\\]").unwrap(),
        ];

        for pattern in season_patterns {
            if let Some(captures) = pattern.captures(&path_str) {
                if let Some(season_str) = captures.get(1) {
                    if let Ok(season) = season_str.as_str().parse::<u32>() {
                        return Some(season);
                    }
                }
            }
        }

        None
    }

    /// Try to extract episode number from filename even without standard patterns
    fn extract_episode_number_from_filename(&self, filename: &str) -> Option<u32> {
        // Look for standalone numbers that might be episode numbers
        // E.g., "01.mkv", "episode_01.mkv", "01 - Title.mkv"
        let patterns = vec![
            Regex::new(r"^(\d{1,3})\.").unwrap(), // Starts with number
            Regex::new(r"[Ee]pisode[\s_-]*(\d{1,3})").unwrap(), // "Episode 01"
            Regex::new(r"[Ee]p[\s_-]*(\d{1,3})").unwrap(), // "Ep 01"
            Regex::new(r"[\s_-](\d{1,3})[\s_-]").unwrap(), // " 01 " or "_01_"
        ];

        for pattern in patterns {
            if let Some(captures) = pattern.captures(filename) {
                if let Some(ep_str) = captures.get(1) {
                    if let Ok(episode) = ep_str.as_str().parse::<u32>() {
                        if episode > 0 && episode < 1000 {
                            // Reasonable episode range
                            return Some(episode);
                        }
                    }
                }
            }
        }

        None
    }

    /// Force parse as movie (used when we know from folder structure)
    fn parse_as_movie(&self, filename: &str, file_path: &Path) -> Option<ParsedMediaInfo> {
        // First, try to parse the parent folder name
        if let Some(parent) = file_path.parent() {
            if let Some(folder_name) = parent.file_name() {
                if let Some(folder_str) = folder_name.to_str() {
                    info!("Trying to parse movie from folder name: {}", folder_str);
                    
                    // Try to match "movie_name (year)" pattern in folder name
                    let folder_regex = Regex::new(r"^(.+?)\s*\((\d{4})\)\s*$").ok();
                    if let Some(regex) = folder_regex {
                        if let Some(captures) = regex.captures(folder_str) {
                            if let (Some(title_match), Some(year_match)) = (captures.get(1), captures.get(2)) {
                                let title = title_match.as_str().trim().to_string();
                                if let Ok(year) = year_match.as_str().parse::<u32>() {
                                    if (1900..=2100).contains(&year) {
                                        info!("Successfully parsed movie from folder: {} ({})", title, year);
                                        return Some(ParsedMediaInfo {
                                            media_type: MediaType::Movie,
                                            title,
                                            year: Some(year),
                                            show_name: None,
                                            season: None,
                                            episode: None,
                                            episode_title: None,
                                            extra_type: None,
                                            parent_title: None,
                                            resolution: self.extract_resolution(filename),
                                            source: self.extract_source(filename),
                                            release_group: self.extract_release_group(filename),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Fall back to filename parsing if folder parsing failed
        info!("Folder parsing failed, falling back to filename parsing");
        
        // Extract year from original filename first (before any modifications)
        let year = self.extract_year(filename);
        
        // Remove file extension first before any processing
        let mut cleaned_title = filename.to_string();
        cleaned_title = Regex::new(r"(?i)\.(mkv|mp4|avi|mov|wmv|flv|webm|m4v|mpg|mpeg)$")
            .unwrap()
            .replace(&cleaned_title, "")
            .to_string();

        // Handle multi-language titles (e.g., "Il Gladiatore II - Gladiator II")
        // If there's a dash with potential duplicate title, take the part after the dash
        if let Some(dash_pos) = cleaned_title.find(" - ") {
            let _before_dash = &cleaned_title[..dash_pos];
            let after_dash = &cleaned_title[dash_pos + 3..];

            // Check if the part after dash looks like an English title
            if after_dash.chars().any(|c| c.is_ascii_alphabetic())
                && !after_dash.chars().all(|c| c.is_ascii_uppercase())
            {
                // Use the part after the dash if it looks like a proper title
                cleaned_title = after_dash.to_string();
            }
        }

        // Remove year from the title if present
        if let Some(y) = year {
            cleaned_title = cleaned_title.replace(&format!(" {y}"), "");
            cleaned_title = cleaned_title.replace(&format!("({y})"), "");
            cleaned_title = cleaned_title.replace(&format!(".{y}"), "");  // Handle .2008 at end
            cleaned_title = cleaned_title.replace(&format!(".{y}."), " ");
            cleaned_title = cleaned_title.replace(&format!(" {y} "), " ");
        }

        // Now clean the title
        cleaned_title = self.clean_movie_title(&cleaned_title);

        Some(ParsedMediaInfo {
            media_type: MediaType::Movie,
            title: cleaned_title,
            year,
            show_name: None,
            season: None,
            episode: None,
            episode_title: None,
            extra_type: None,
            parent_title: None,
            resolution: self.extract_resolution(filename),
            source: self.extract_source(filename),
            release_group: self.extract_release_group(filename),
        })
    }

    /// Clean movie title more aggressively for TMDB search
    fn clean_movie_title(&self, title: &str) -> String {
        let mut cleaned = title.to_string();

        // Remove file extensions first (case-insensitive)
        cleaned = Regex::new(r"(?i)\.(mkv|mp4|avi|mov|wmv|flv|webm|m4v|mpg|mpeg)$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // First pass: Remove everything in square brackets
        cleaned = Regex::new(r"\[.*?\]")
            .unwrap()
            .replace_all(&cleaned, " ")
            .to_string();

        // Remove everything after the first occurrence of common quality/format indicators
        // This handles cases like "Movie Title (2014) (1080p..." by cutting at the second parenthesis
        let quality_cutoff_regex = Regex::new(
            r"(?i)\s*[\(\[]?\s*(BluRay|Bluray|BDRip|BRRip|WEBRip|WEB-DL|WEBDL-1080p|WebDl|SDTV|HDTV|DVDRip|CAM|TS|HC|HDCAM|HDRip|dvd|dvdrip|xvid|divx|x264|x265|h264|h265|hevc|10bit|10\s*bit|HDR|HDR10|DV|AC3|AAC|DTS|FLAC|Remux|REMUX|1080p|720p|480p|2160p|4K|UHD|[\(\[]?\d{3,4}p).*$"
        ).unwrap();
        cleaned = quality_cutoff_regex.replace(&cleaned, "").to_string();

        // Remove edition info
        let edition_regex = Regex::new(
            r"(?i)[\s\-]*(unrated|extended|director'?s?\s*cut|theatrical|special\s*edition|ultimate\s*edition|final\s*cut|remastered|uncut|unknown).*$"
        ).unwrap();
        cleaned = edition_regex.replace(&cleaned, "").to_string();

        // Now remove any remaining content in parentheses (but be careful about nested or unmatched)
        // This regex handles nested parentheses better
        while cleaned.contains('(') || cleaned.contains(')') {
            let old_len = cleaned.len();
            cleaned = Regex::new(r"\([^()]*\)")
                .unwrap()
                .replace_all(&cleaned, " ")
                .to_string();
            // Also remove any lone parentheses
            cleaned = cleaned.replace(['(', ')'], " ");
            if cleaned.len() == old_len {
                break; // Prevent infinite loop
            }
        }

        // Replace dots and underscores with spaces
        cleaned = cleaned.replace(['.', '_'], " ");
        
        // Remove standalone years (1900-2100)
        cleaned = Regex::new(r"\b(19|20)\d{2}\b")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();

        // Remove release group patterns (dash followed by group name at end)
        cleaned = Regex::new(r"\s*-\s*\w+$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // Clean up extra whitespace and punctuation
        cleaned = cleaned.split_whitespace().collect::<Vec<&str>>().join(" ");

        // Final cleanup: remove any trailing punctuation
        cleaned = cleaned
            .trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
            .to_string();

        cleaned
    }

    /// Parse TV episode filename
    fn parse_tv_episode(&self, filename: &str) -> Option<ParsedMediaInfo> {
        // Try multiple TV patterns

        // Pattern 1: Show.Name.S01E01.Episode.Title.Quality.Info-Group
        let tv_regex1 =
            Regex::new(r"^(.+?)\.S(\d{1,2})E(\d{1,3})\.(.+?)\.(\d{3,4}p)\.(.+?)-(\w+)$").ok()?;

        if let Some(captures) = tv_regex1.captures(filename) {
            let show_name = captures.get(1)?.as_str().replace('.', " ");
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;
            let episode_title = captures.get(4)?.as_str().replace('.', " ");
            let resolution = captures.get(5)?.as_str().to_string();
            let quality_info = captures.get(6)?.as_str();
            let release_group = captures.get(7)?.as_str().to_string();

            debug!(
                "Parsed TV episode (pattern 1): {} S{}E{} - {}",
                show_name, season, episode, episode_title
            );

            return Some(ParsedMediaInfo {
                media_type: MediaType::TvEpisode,
                title: format!(
                    "{show_name} - S{season:02}E{episode:02} - {episode_title}"
                ),
                year: None,
                show_name: Some(show_name),
                season: Some(season),
                episode: Some(episode),
                episode_title: Some(episode_title),
                extra_type: None,
                parent_title: None,
                resolution: Some(resolution),
                source: self.extract_source(quality_info),
                release_group: Some(release_group),
            });
        }

        // Pattern 2: Show Name S01E01 or Show.Name.S01E01 (more flexible)
        let tv_regex2 =
            Regex::new(r"(?i)^(.+?)[\s\.]S(\d{1,2})E(\d{1,3})(?:[\s\.\-](.+))?$").ok()?;

        if let Some(captures) = tv_regex2.captures(filename) {
            let show_name = captures
                .get(1)?
                .as_str()
                .replace(['.', '_'], " ")
                .trim()
                .to_string();
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;

            // Extract episode title - don't clean it if it's already readable
            let remainder = captures.get(4).map(|m| m.as_str()).unwrap_or("");
            let episode_title = if remainder.contains('.') || remainder.contains('_') {
                self.clean_filename(remainder)
            } else {
                remainder.trim().to_string()
            };

            debug!(
                "Parsed TV episode (pattern 2): {} S{}E{}",
                show_name, season, episode
            );

            return Some(ParsedMediaInfo {
                media_type: MediaType::TvEpisode,
                title: format!("{show_name} - S{season:02}E{episode:02}"),
                year: self.extract_year(&show_name),
                show_name: Some(self.clean_filename(&show_name)),
                season: Some(season),
                episode: Some(episode),
                episode_title: if episode_title.is_empty() {
                    None
                } else {
                    Some(episode_title)
                },
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            });
        }

        // Pattern 3: Show Name 1x01 or Show.Name.1x01
        let tv_regex3 =
            Regex::new(r"(?i)^(.+?)[\s\.](\d{1,2})x(\d{1,3})(?:[\s\.\-](.+))?$").ok()?;

        if let Some(captures) = tv_regex3.captures(filename) {
            let show_name = captures
                .get(1)?
                .as_str()
                .replace(['.', '_'], " ")
                .trim()
                .to_string();
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;

            let remainder = captures.get(4).map(|m| m.as_str()).unwrap_or("");
            let episode_title = if remainder.contains('.') || remainder.contains('_') {
                self.clean_filename(remainder)
            } else {
                remainder.trim().to_string()
            };

            debug!(
                "Parsed TV episode (pattern 3): {} {}x{}",
                show_name, season, episode
            );

            return Some(ParsedMediaInfo {
                media_type: MediaType::TvEpisode,
                title: format!("{show_name} - S{season:02}E{episode:02}"),
                year: self.extract_year(&show_name),
                show_name: Some(self.clean_filename(&show_name)),
                season: Some(season),
                episode: Some(episode),
                episode_title: if episode_title.is_empty() {
                    None
                } else {
                    Some(episode_title)
                },
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            });
        }

        // Pattern 4: Show.Name.101 (absolute episode number)
        let tv_regex4 = Regex::new(r"(?i)^(.+?)[\s\.](\d)(\d{2})(?:[\s\.\-](.+))?$").ok()?;

        if let Some(captures) = tv_regex4.captures(filename) {
            let show_name = captures
                .get(1)?
                .as_str()
                .replace(['.', '_'], " ")
                .trim()
                .to_string();
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;

            // Only accept if it looks like a valid season/episode combo
            if (1..=20).contains(&season) && (1..=99).contains(&episode) {
                // Extract episode title if present
                let remainder = captures.get(4).map(|m| m.as_str()).unwrap_or("");
                let episode_title = if remainder.contains('.') || remainder.contains('_') {
                    self.clean_filename(remainder)
                } else {
                    remainder.trim().to_string()
                };

                debug!(
                    "Parsed TV episode (pattern 4 - absolute): {} {}x{}",
                    show_name, season, episode
                );

                return Some(ParsedMediaInfo {
                    media_type: MediaType::TvEpisode,
                    title: format!("{show_name} - S{season:02}E{episode:02}"),
                    year: self.extract_year(&show_name),
                    show_name: Some(self.clean_filename(&show_name)),
                    season: Some(season),
                    episode: Some(episode),
                    episode_title: if episode_title.is_empty() {
                        None
                    } else {
                        Some(episode_title)
                    },
                    extra_type: None,
                    parent_title: None,
                    resolution: self.extract_resolution(filename),
                    source: self.extract_source(filename),
                    release_group: self.extract_release_group(filename),
                });
            }
        }

        None
    }

    /// Parse movie filename
    fn parse_movie(&self, filename: &str) -> Option<ParsedMediaInfo> {
        // Check if this looks like a TV show pattern first
        if filename.to_uppercase().contains("S0")
            || filename.contains("x0")
            || Regex::new(r"(?i)(episode|ep\.?\s*\d)")
                .unwrap()
                .is_match(filename)
        {
            return None;
        }

        // Try multiple patterns for movies

        // Pattern 1: Movie.Name.Year.Quality.Info-Group
        let movie_regex1 = Regex::new(r"^(.+?)\.(\d{4})\.(.+?)-(\w+)$").ok()?;

        if let Some(captures) = movie_regex1.captures(filename) {
            let title = captures.get(1)?.as_str().replace('.', " ");
            let year: u32 = captures.get(2)?.as_str().parse().ok()?;
            let quality_info = captures.get(3)?.as_str();
            let release_group = captures.get(4)?.as_str().to_string();

            debug!("Parsed movie (pattern 1): {} ({})", title, year);

            return Some(ParsedMediaInfo {
                media_type: MediaType::Movie,
                title: title.clone(),
                year: Some(year),
                show_name: None,
                season: None,
                episode: None,
                episode_title: None,
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(quality_info),
                source: self.extract_source(quality_info),
                release_group: Some(release_group),
            });
        }

        // Pattern 2: Movie Name (Year) Quality-Resolution
        let movie_regex2 = Regex::new(r"^(.+?)\s*\((\d{4})\)\s*(.+?)$").ok()?;

        if let Some(captures) = movie_regex2.captures(filename) {
            let title = captures.get(1)?.as_str().trim().to_string();
            let year: u32 = captures.get(2)?.as_str().parse().ok()?;
            let quality_info = captures.get(3)?.as_str();

            debug!("Parsed movie (pattern 2): {} ({})", title, year);

            return Some(ParsedMediaInfo {
                media_type: MediaType::Movie,
                title: title.clone(),
                year: Some(year),
                show_name: None,
                season: None,
                episode: None,
                episode_title: None,
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(quality_info),
                source: self.extract_source(quality_info),
                release_group: self.extract_release_group(quality_info),
            });
        }

        // Pattern 3: Movie.Name.Year (simple with dots)
        let movie_regex3 = Regex::new(r"^(.+?)\.(\d{4})$").ok()?;

        if let Some(captures) = movie_regex3.captures(filename) {
            let title = captures.get(1)?.as_str().replace('.', " ");
            let year: u32 = captures.get(2)?.as_str().parse().ok()?;

            debug!("Parsed movie (pattern 3): {} ({})", title, year);

            return Some(ParsedMediaInfo {
                media_type: MediaType::Movie,
                title: title.clone(),
                year: Some(year),
                show_name: None,
                season: None,
                episode: None,
                episode_title: None,
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            });
        }

        // Pattern 4: Movie Name Year (simple with spaces)
        let movie_regex4 = Regex::new(r"^(.+?)\s+(\d{4})(?:\s|$)").ok()?;

        if let Some(captures) = movie_regex4.captures(filename) {
            let title = captures.get(1)?.as_str().trim().to_string();
            let year: u32 = captures.get(2)?.as_str().parse().ok()?;

            if (1900..=2100).contains(&year) {
                debug!("Parsed movie (pattern 4): {} ({})", title, year);

                return Some(ParsedMediaInfo {
                    media_type: MediaType::Movie,
                    title: title.clone(),
                    year: Some(year),
                    show_name: None,
                    season: None,
                    episode: None,
                    episode_title: None,
                    extra_type: None,
                    parent_title: None,
                    resolution: self.extract_resolution(filename),
                    source: self.extract_source(filename),
                    release_group: self.extract_release_group(filename),
                });
            }
        }

        // Pattern 5: Just a clean title (no year) - use cleaned filename
        let cleaned_title = self.clean_filename(filename);
        if !cleaned_title.is_empty() {
            debug!("Parsed movie (pattern 5 - no year): {}", cleaned_title);

            return Some(ParsedMediaInfo {
                media_type: MediaType::Movie,
                title: cleaned_title,
                year: None,
                show_name: None,
                season: None,
                episode: None,
                episode_title: None,
                extra_type: None,
                parent_title: None,
                resolution: self.extract_resolution(filename),
                source: self.extract_source(filename),
                release_group: self.extract_release_group(filename),
            });
        }

        None
    }

    /// Clean show name for TMDB searches by removing year and other metadata
    fn clean_show_name_for_search(&self, show_name: &str) -> String {
        let mut cleaned = show_name.to_string();

        // Remove year in parentheses (e.g., "The Americans (2013)" -> "The Americans")
        cleaned = Regex::new(r"\s*\(\d{4}\)\s*$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // Remove year after title (e.g., "The Americans 2013" -> "The Americans")
        cleaned = Regex::new(r"\s+\d{4}\s*$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // Clean up any remaining dots or underscores
        cleaned = cleaned.replace(['.', '_'], " ");

        // Normalize whitespace
        cleaned = cleaned.split_whitespace().collect::<Vec<&str>>().join(" ");

        cleaned.trim().to_string()
    }

    /// Clean filename by removing known metadata patterns (similar to Jellyfin's CleanStrings)
    fn clean_filename(&self, filename: &str) -> String {
        // Regex pattern inspired by Jellyfin's CleanStrings
        let clean_regex = Regex::new(
            r"(?i)[ _\,\.\(\)\[\]\-](3d|sbs|tab|hsbs|htab|mvc|HDR|HDC|UHD|UltraHD|4k|ac3|dts|custom|dc|divx|divx5|dsr|dsrip|dutch|dvd|dvdrip|dvdscr|dvdscreener|screener|dvdivx|cam|fragment|fs|sdtv|hdtv|hdrip|hdtvrip|internal|limited|multisubs|ntsc|ogg|ogm|pal|pdtv|proper|repack|rerip|retail|cd[1-9]|r3|r5|bd5|se|svcd|swedish|german|read\.nfo|nfofix|unrated|ws|telesync|ts|telecine|tc|brrip|bdrip|480p|480i|576p|576i|720p|720i|1080p|1080i|2160p|hrhd|hrhdtv|hddvd|bluray|x264|h264|xvid|xvidvd|xxx|www\.www|\[.*\])([ _\,\.\(\)\[\]\-]|$)"
        ).unwrap_or_else(|_| Regex::new(r"$^").unwrap());

        let mut cleaned = filename.to_string();

        // Remove anything in square brackets
        cleaned = Regex::new(r"\[.*?\]")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();

        // Remove year in parentheses temporarily to clean the title
        let year_regex = Regex::new(r"\s*\((\d{4})\)\s*").unwrap();
        let year_match = year_regex.find(&cleaned);
        if let Some(m) = year_match {
            cleaned = cleaned[..m.start()].to_string() + &cleaned[m.end()..];
        }

        // Apply the main cleaning regex repeatedly until no more changes
        loop {
            let before = cleaned.clone();
            cleaned = clean_regex.replace_all(&cleaned, " ").to_string();
            if cleaned == before {
                break;
            }
        }

        // Clean up file extensions if any remain
        cleaned = Regex::new(r"\.(mkv|mp4|avi|mov|wmv|flv|webm)$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();

        // Replace dots and underscores with spaces
        cleaned = cleaned.replace(['.', '_'], " ");
        
        // Additional cleanup for remaining quality indicators that might have been missed
        let quality_terms = Regex::new(r"(?i)\b(WEB-DL|WEBDL|WEB DL|BluRay|Bluray|BDRip|BRRip|DVDRip|HDTV|x264|x265|h264|h265|HEVC)\b").unwrap();
        cleaned = quality_terms.replace_all(&cleaned, "").to_string();

        // Remove extra whitespace
        cleaned = cleaned.split_whitespace().collect::<Vec<&str>>().join(" ");

        cleaned.trim().to_string()
    }

    /// Extract year from filename
    fn extract_year(&self, filename: &str) -> Option<u32> {
        // Try year in parentheses first (Movie Title (2023))
        if let Some(captures) = Regex::new(r"\((\d{4})\)").unwrap().captures(filename) {
            if let Ok(year) = captures.get(1)?.as_str().parse::<u32>() {
                if (1900..=2100).contains(&year) {
                    return Some(year);
                }
            }
        }

        // Try year with dots (Movie.Title.2023.BluRay)
        if let Some(captures) = Regex::new(r"\.(\d{4})\.").unwrap().captures(filename) {
            if let Ok(year) = captures.get(1)?.as_str().parse::<u32>() {
                if (1900..=2100).contains(&year) {
                    return Some(year);
                }
            }
        }

        // Try year at end of title before quality info
        if let Some(captures) = Regex::new(r"\s(\d{4})\s").unwrap().captures(filename) {
            if let Ok(year) = captures.get(1)?.as_str().parse::<u32>() {
                if (1900..=2100).contains(&year) {
                    return Some(year);
                }
            }
        }
        
        // Try year at end with dot prefix (Movie.Title.1999)
        if let Some(captures) = Regex::new(r"\.(\d{4})$").unwrap().captures(filename) {
            if let Ok(year) = captures.get(1)?.as_str().parse::<u32>() {
                if (1900..=2100).contains(&year) {
                    return Some(year);
                }
            }
        }

        None
    }

    /// Extract resolution from filename
    fn extract_resolution(&self, filename: &str) -> Option<String> {
        let filename_lower = filename.to_lowercase();

        if filename_lower.contains("2160p")
            || filename_lower.contains("4k")
            || filename_lower.contains("uhd")
        {
            Some("2160p".to_string())
        } else if filename_lower.contains("1080p") || filename_lower.contains("1080i") {
            Some("1080p".to_string())
        } else if filename_lower.contains("720p") || filename_lower.contains("720i") {
            Some("720p".to_string())
        } else if filename_lower.contains("576p") || filename_lower.contains("576i") {
            Some("576p".to_string())
        } else if filename_lower.contains("480p") || filename_lower.contains("480i") {
            Some("480p".to_string())
        } else {
            None
        }
    }

    /// Extract source/quality from filename
    fn extract_source(&self, filename: &str) -> Option<String> {
        let filename_lower = filename.to_lowercase();

        if filename_lower.contains("bluray")
            || filename_lower.contains("blu-ray")
            || filename_lower.contains("bdrip")
            || filename_lower.contains("brrip")
        {
            Some("BluRay".to_string())
        } else if filename_lower.contains("web-dl")
            || filename_lower.contains("webdl")
            || filename_lower.contains("webrip")
        {
            Some("WEB-DL".to_string())
        } else if filename_lower.contains("hdtv") {
            Some("HDTV".to_string())
        } else if filename_lower.contains("dvdrip") || filename_lower.contains("dvd") {
            Some("DVD".to_string())
        } else if filename_lower.contains("cam") || filename_lower.contains("hdcam") {
            Some("CAM".to_string())
        } else if filename_lower.contains("screener") || filename_lower.contains("scr") {
            Some("Screener".to_string())
        } else {
            None
        }
    }

    /// Extract release group from filename
    fn extract_release_group(&self, filename: &str) -> Option<String> {
        // Try to find release group after a dash at the end
        if let Some(captures) = Regex::new(r"-(\w+)(?:\.\w{3,4})?$")
            .unwrap()
            .captures(filename)
        {
            return Some(captures.get(1)?.as_str().to_string());
        }

        // Try to find release group in square brackets
        if let Some(captures) = Regex::new(r"\[(\w+)\]").unwrap().captures(filename) {
            return Some(captures.get(1)?.as_str().to_string());
        }

        None
    }
}

#[derive(Debug, Default)]
struct TechnicalMetadata {
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
    pub framerate: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_extract_metadata_real_file() {
        // Test with a real file if it exists
        let test_path = "./test-media/It's Always Sunny in Philadelphia/Season 1/Its.Always.Sunny.in.Philadelphia.S01E01.The.Gang.Gets.Racist.480p.WEB-DL.AAC2.0.H.264-BTN.mkv";

        if Path::new(test_path).exists() {
            let mut extractor = MetadataExtractor::new();

            match extractor.extract_metadata(test_path) {
                Ok(metadata) => {
                    println!("Duration: {:?}", metadata.duration);
                    println!(
                        "Resolution: {}x{}",
                        metadata.width.unwrap_or(0),
                        metadata.height.unwrap_or(0)
                    );
                    println!("File size: {} bytes", metadata.file_size);

                    // Check that we got some basic metadata
                    assert!(metadata.file_size > 0);

                    // Check parsed info
                    if let Some(parsed) = &metadata.parsed_info {
                        assert_eq!(parsed.media_type, MediaType::TvEpisode);
                        assert_eq!(
                            parsed.show_name,
                            Some("Its Always Sunny in Philadelphia".to_string())
                        );
                        assert_eq!(parsed.season, Some(1));
                        assert_eq!(parsed.episode, Some(1));
                    }
                }
                Err(e) => {
                    println!(
                        "Metadata extraction failed (expected if file doesn't exist): {e}"
                    );
                }
            }
        } else {
            println!("Test file not found, skipping real file test");
        }
    }

    #[test]
    fn test_parse_tv_episode() {
        let extractor = MetadataExtractor::new();
        let filename = "Its.Always.Sunny.in.Philadelphia.S01E07.Charlie.Got.Molested.480p.WEB-DL.AAC2.0.H.264-BTN";

        let result = extractor.parse_tv_episode(filename).unwrap();

        assert_eq!(result.media_type, MediaType::TvEpisode);
        assert_eq!(
            result.show_name,
            Some("Its Always Sunny in Philadelphia".to_string())
        );
        assert_eq!(result.season, Some(1));
        assert_eq!(result.episode, Some(7));
        assert_eq!(
            result.episode_title,
            Some("Charlie Got Molested".to_string())
        );
        assert_eq!(result.resolution, Some("480p".to_string()));
        assert_eq!(result.source, Some("WEB-DL".to_string()));
        assert_eq!(result.release_group, Some("BTN".to_string()));
    }

    #[test]
    fn test_parse_movie() {
        let extractor = MetadataExtractor::new();
        let filename = "The.Dark.Knight.2008.1080p.BluRay.x264-GROUP";

        let result = extractor.parse_movie(filename).unwrap();

        assert_eq!(result.media_type, MediaType::Movie);
        assert_eq!(result.title, "The Dark Knight");
        assert_eq!(result.year, Some(2008));
        assert_eq!(result.resolution, Some("1080p".to_string()));
        assert_eq!(result.source, Some("BluRay".to_string()));
        assert_eq!(result.release_group, Some("GROUP".to_string()));
    }

    #[test]
    fn test_parse_movie_various_formats() {
        let extractor = MetadataExtractor::new();

        // Test simple title
        let result = extractor.parse_filename("Deadpool.mkv").unwrap();
        assert_eq!(result.media_type, MediaType::Movie);
        assert_eq!(result.title, "Deadpool");
        assert_eq!(result.year, None);

        // Test title with year in parentheses
        let result = extractor
            .parse_filename("Deadpool & Wolverine (2024) Bluray-2160p.mkv")
            .unwrap();
        assert_eq!(result.media_type, MediaType::Movie);
        assert_eq!(result.title, "Deadpool & Wolverine");
        assert_eq!(result.year, Some(2024));
        assert_eq!(result.resolution, Some("2160p".to_string()));
        assert_eq!(result.source, Some("BluRay".to_string()));

        // Test title with dots and year
        let result = extractor.parse_filename("The.Matrix.1999.mkv").unwrap();
        assert_eq!(result.media_type, MediaType::Movie);
        assert_eq!(result.title, "The Matrix");
        assert_eq!(result.year, Some(1999));

        // Test title with quality info
        let result = extractor
            .parse_filename("Inception 2010 1080p BluRay.mkv")
            .unwrap();
        assert_eq!(result.media_type, MediaType::Movie);
        assert_eq!(result.title, "Inception");
        assert_eq!(result.year, Some(2010));
        assert_eq!(result.resolution, Some("1080p".to_string()));
        assert_eq!(result.source, Some("BluRay".to_string()));
    }

    #[test]
    fn test_parse_tv_various_formats() {
        let extractor = MetadataExtractor::new();

        // Test S01E01 format
        let result = extractor.parse_filename("Breaking Bad S01E01.mkv").unwrap();
        assert_eq!(result.media_type, MediaType::TvEpisode);
        assert_eq!(result.show_name, Some("Breaking Bad".to_string()));
        assert_eq!(result.season, Some(1));
        assert_eq!(result.episode, Some(1));

        // Test 1x01 format
        let result = extractor
            .parse_filename("Game.of.Thrones.1x01.Winter.Is.Coming.mkv")
            .unwrap();
        assert_eq!(result.media_type, MediaType::TvEpisode);
        assert_eq!(result.show_name, Some("Game of Thrones".to_string()));
        assert_eq!(result.season, Some(1));
        assert_eq!(result.episode, Some(1));
        assert_eq!(result.episode_title, Some("Winter Is Coming".to_string()));

        // Test absolute numbering
        let result = extractor
            .parse_filename("Naruto.101.The.Fight.Begins.mkv")
            .unwrap();
        assert_eq!(result.media_type, MediaType::TvEpisode);
        assert_eq!(result.show_name, Some("Naruto".to_string()));
        assert_eq!(result.season, Some(1));
        assert_eq!(result.episode, Some(1));
        assert_eq!(result.episode_title, Some("The Fight Begins".to_string()));
    }

    #[test]
    fn test_clean_filename() {
        let extractor = MetadataExtractor::new();

        assert_eq!(extractor.clean_filename("Movie.1080p.BluRay.x264"), "Movie");
        assert_eq!(
            extractor.clean_filename("Movie [2023] 720p WEB-DL"),
            "Movie"
        );
        assert_eq!(
            extractor.clean_filename("Movie.Title.2160p.4K.UHD.HDR"),
            "Movie Title"
        );
        assert_eq!(
            extractor.clean_filename("Movie_Name_HDTV_XviD"),
            "Movie Name"
        );
    }

    #[test]
    fn test_is_sample() {
        let extractor = MetadataExtractor::new();

        // Test sample detection by duration
        let sample_metadata = MediaMetadata {
            duration: Some(90.0),         // 1.5 minutes - should be detected as sample
            file_size: 100 * 1024 * 1024, // 100 MB
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            bitrate: Some(5000000),
            framerate: Some(24.0),
            parsed_info: None,
            external_info: None,
        };
        assert!(extractor.is_sample(&sample_metadata));

        // Test normal file (not a sample)
        let normal_metadata = MediaMetadata {
            duration: Some(3600.0),            // 1 hour - should not be detected as sample
            file_size: 2 * 1024 * 1024 * 1024, // 2 GB
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            bitrate: Some(5000000),
            framerate: Some(24.0),
            parsed_info: None,
            external_info: None,
        };
        assert!(!extractor.is_sample(&normal_metadata));

        // Test sample detection by file size (small file, no duration)
        let small_file_metadata = MediaMetadata {
            duration: None,              // No duration info
            file_size: 30 * 1024 * 1024, // 30 MB - should be detected as sample
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            bitrate: Some(5000000),
            framerate: Some(24.0),
            parsed_info: None,
            external_info: None,
        };
        assert!(extractor.is_sample(&small_file_metadata));

        // Test edge case: small file with long duration (not a sample)
        let small_long_metadata = MediaMetadata {
            duration: Some(3600.0),      // 1 hour
            file_size: 40 * 1024 * 1024, // 40 MB - small but long duration
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            bitrate: Some(5000000),
            framerate: Some(24.0),
            parsed_info: None,
            external_info: None,
        };
        assert!(!extractor.is_sample(&small_long_metadata));

        // Test very short file (should not be sample due to minimum threshold)
        let very_short_metadata = MediaMetadata {
            duration: Some(5.0),         // 5 seconds
            file_size: 10 * 1024 * 1024, // 10 MB
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            bitrate: Some(5000000),
            framerate: Some(24.0),
            parsed_info: None,
            external_info: None,
        };
        assert!(!extractor.is_sample(&very_short_metadata));
    }
    
    #[test]
    fn test_parse_movie_from_folder_name() {
        let extractor = MetadataExtractor::new();
        
        // Test standard "Movie Name (Year)" format
        let path = Path::new("/movies/The Matrix (1999)/The.Matrix.1999.BluRay.1080p.mkv");
        let result = extractor.parse_as_movie("The.Matrix.1999.BluRay.1080p.mkv", path);
        let parsed = result.expect("Should parse movie from folder name");
        
        assert_eq!(parsed.media_type, MediaType::Movie);
        assert_eq!(parsed.title, "The Matrix");
        assert_eq!(parsed.year, Some(1999));
        
        // Test folder name with spaces and special characters
        let path2 = Path::new("/movies/Star Wars Episode IV - A New Hope (1977)/star.wars.mkv");
        let result2 = extractor.parse_as_movie("star.wars.mkv", path2);
        let parsed2 = result2.expect("Should parse movie with complex folder name");
        
        assert_eq!(parsed2.title, "Star Wars Episode IV - A New Hope");
        assert_eq!(parsed2.year, Some(1977));
        
        // Test fallback to filename parsing when folder doesn't match pattern
        let path3 = Path::new("/movies/random_folder/The.Dark.Knight.2008.mkv");
        let result3 = extractor.parse_as_movie("The.Dark.Knight.2008.mkv", path3);
        let parsed3 = result3.expect("Should fall back to filename parsing");
        
        assert_eq!(parsed3.title, "The Dark Knight");
        assert_eq!(parsed3.year, Some(2008));
        
        // Test folder without year - should fall back to filename
        let path4 = Path::new("/movies/Inception/Inception.2010.BluRay.mkv");
        let result4 = extractor.parse_as_movie("Inception.2010.BluRay.mkv", path4);
        let parsed4 = result4.expect("Should fall back when folder has no year");
        
        assert_eq!(parsed4.title, "Inception");
        assert_eq!(parsed4.year, Some(2010));
    }
}

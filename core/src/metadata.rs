use crate::{MediaError, MediaMetadata, ParsedMediaInfo, MediaType, Result};
use ffmpeg_next as ffmpeg;
use std::path::Path;
use tracing::{debug, info};
use regex::Regex;

pub struct MetadataExtractor {
    /// Whether FFmpeg has been initialized
    initialized: bool,
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
        }
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
        let parsed_info = self.parse_filename(file_path);
        
        // Get file size
        let file_size = file_path.metadata()
            .map_err(MediaError::Io)?
            .len();
        
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
        
        let input = ffmpeg::format::input(file_path)
            .map_err(MediaError::Ffmpeg)?;
        
        let mut technical = TechnicalMetadata::default();
        
        // Get duration
        if input.duration() != ffmpeg::ffi::AV_NOPTS_VALUE {
            technical.duration = Some(input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64);
        }
        
        // Get bitrate
        if input.bit_rate() > 0 {
            technical.bitrate = Some(input.bit_rate() as u64);
        }
        
        // Find video and audio streams
        for (i, stream) in input.streams().enumerate() {
            let codec = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(MediaError::Ffmpeg)?;
            
            match codec.medium() {
                ffmpeg::media::Type::Video => {
                    debug!("Found video stream {}", i);
                    
                    if let Ok(video) = codec.decoder().video() {
                        technical.width = Some(video.width());
                        technical.height = Some(video.height());
                        
                        // Get frame rate
                        let frame_rate = stream.avg_frame_rate();
                        if frame_rate.denominator() != 0 {
                            technical.framerate = Some(
                                frame_rate.numerator() as f64 / frame_rate.denominator() as f64
                            );
                        }
                        
                        // Get codec name
                        if let Some(codec) = video.codec() {
                            technical.video_codec = Some(codec.name().to_string());
                        }
                    }
                }
                ffmpeg::media::Type::Audio => {
                    debug!("Found audio stream {}", i);
                    
                    if let Ok(audio) = codec.decoder().audio() {
                        // Get codec name
                        if let Some(codec) = audio.codec() {
                            technical.audio_codec = Some(codec.name().to_string());
                        }
                    }
                }
                _ => {
                    debug!("Found other stream type: {:?}", codec.medium());
                }
            }
        }
        
        debug!("Technical metadata extracted: {:?}", technical);
        Ok(technical)
    }

    /// Parse filename to extract show/episode information
    fn parse_filename<P: AsRef<Path>>(&self, file_path: P) -> Option<ParsedMediaInfo> {
        let file_path = file_path.as_ref();
        let filename = file_path.file_stem()?.to_str()?;
        
        debug!("Parsing filename: {}", filename);
        
        // Try TV show pattern first (SxxExx format)
        if let Some(tv_info) = self.parse_tv_episode(filename) {
            return Some(tv_info);
        }
        
        // Try movie pattern
        if let Some(movie_info) = self.parse_movie(filename) {
            return Some(movie_info);
        }
        
        // Fallback to unknown
        Some(ParsedMediaInfo {
            media_type: MediaType::Unknown,
            title: filename.replace('.', " ").replace('_', " "),
            year: None,
            show_name: None,
            season: None,
            episode: None,
            episode_title: None,
            resolution: None,
            source: None,
            release_group: None,
        })
    }

    /// Parse TV episode filename
    fn parse_tv_episode(&self, filename: &str) -> Option<ParsedMediaInfo> {
        // Pattern: Show.Name.S01E01.Episode.Title.Quality.Info-Group
        let tv_regex = Regex::new(
            r"^(.+?)\.S(\d{1,2})E(\d{1,3})\.(.+?)\.(\d{3,4}p)\.(.+?)-(\w+)$"
        ).ok()?;
        
        if let Some(captures) = tv_regex.captures(filename) {
            let show_name = captures.get(1)?.as_str().replace('.', " ");
            let season: u32 = captures.get(2)?.as_str().parse().ok()?;
            let episode: u32 = captures.get(3)?.as_str().parse().ok()?;
            let episode_title = captures.get(4)?.as_str().replace('.', " ");
            let resolution = captures.get(5)?.as_str().to_string();
            let quality_info = captures.get(6)?.as_str();
            let release_group = captures.get(7)?.as_str().to_string();
            
            // Extract source from quality info
            let source = if quality_info.contains("WEB-DL") {
                Some("WEB-DL".to_string())
            } else if quality_info.contains("BluRay") {
                Some("BluRay".to_string())
            } else if quality_info.contains("HDTV") {
                Some("HDTV".to_string())
            } else {
                None
            };
            
            debug!("Parsed TV episode: {} S{}E{} - {}", show_name, season, episode, episode_title);
            
            return Some(ParsedMediaInfo {
                media_type: MediaType::TvEpisode,
                title: format!("{} - S{:02}E{:02} - {}", show_name, season, episode, episode_title),
                year: None,
                show_name: Some(show_name),
                season: Some(season),
                episode: Some(episode),
                episode_title: Some(episode_title),
                resolution: Some(resolution),
                source,
                release_group: Some(release_group),
            });
        }
        
        None
    }

    /// Parse movie filename
    fn parse_movie(&self, filename: &str) -> Option<ParsedMediaInfo> {
        // Pattern: Movie.Name.Year.Quality.Info-Group
        let movie_regex = Regex::new(
            r"^(.+?)\.(\d{4})\.(.+?)-(\w+)$"
        ).ok()?;
        
        if let Some(captures) = movie_regex.captures(filename) {
            let title = captures.get(1)?.as_str().replace('.', " ");
            let year: u32 = captures.get(2)?.as_str().parse().ok()?;
            let quality_info = captures.get(3)?.as_str();
            let release_group = captures.get(4)?.as_str().to_string();
            
            // Extract resolution and source
            let resolution = if quality_info.contains("1080p") {
                Some("1080p".to_string())
            } else if quality_info.contains("720p") {
                Some("720p".to_string())
            } else if quality_info.contains("480p") {
                Some("480p".to_string())
            } else {
                None
            };
            
            let source = if quality_info.contains("WEB-DL") {
                Some("WEB-DL".to_string())
            } else if quality_info.contains("BluRay") {
                Some("BluRay".to_string())
            } else {
                None
            };
            
            debug!("Parsed movie: {} ({})", title, year);
            
            return Some(ParsedMediaInfo {
                media_type: MediaType::Movie,
                title: format!("{} ({})", title, year),
                year: Some(year),
                show_name: None,
                season: None,
                episode: None,
                episode_title: None,
                resolution,
                source,
                release_group: Some(release_group),
            });
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
                    println!("Resolution: {}x{}", 
                        metadata.width.unwrap_or(0), 
                        metadata.height.unwrap_or(0)
                    );
                    println!("File size: {} bytes", metadata.file_size);
                    
                    // Check that we got some basic metadata
                    assert!(metadata.file_size > 0);
                    
                    // Check parsed info
                    if let Some(parsed) = &metadata.parsed_info {
                        assert_eq!(parsed.media_type, MediaType::TvEpisode);
                        assert_eq!(parsed.show_name, Some("Its Always Sunny in Philadelphia".to_string()));
                        assert_eq!(parsed.season, Some(1));
                        assert_eq!(parsed.episode, Some(1));
                    }
                }
                Err(e) => {
                    println!("Metadata extraction failed (expected if file doesn't exist): {}", e);
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
        assert_eq!(result.show_name, Some("Its Always Sunny in Philadelphia".to_string()));
        assert_eq!(result.season, Some(1));
        assert_eq!(result.episode, Some(7));
        assert_eq!(result.episode_title, Some("Charlie Got Molested".to_string()));
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
        assert_eq!(result.title, "The Dark Knight (2008)".to_string());
        assert_eq!(result.year, Some(2008));
        assert_eq!(result.resolution, Some("1080p".to_string()));
        assert_eq!(result.source, Some("BluRay".to_string()));
    }
}
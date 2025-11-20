use iced::{
    widget::{button, column, container, image, text, Column, Row},
    Element, Length,
};
use serde::{Deserialize, Serialize};

use crate::{
    poster_cache::{PosterCache, PosterState},
    Message,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub library_type: String, // "Movies" or "TvShows"
    pub paths: Vec<String>,
    pub scan_interval_minutes: u32,
    pub last_scan: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub size: u64,
    pub created_at: String,
    pub metadata: Option<MediaMetadata>,
    pub library_id: Option<String>, // Library this media belongs to
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub duration: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
    pub framerate: Option<f32>,
    pub file_size: u64,
    pub parsed_info: Option<ParsedInfo>,
    pub external_info: Option<ExternalInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedInfo {
    pub media_type: String,
    pub title: String,
    pub year: Option<u32>,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub episode_title: Option<String>,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub release_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalInfo {
    pub tmdb_id: Option<u32>,
    pub tvdb_id: Option<u32>,
    pub imdb_id: Option<String>,
    pub description: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub genres: Vec<String>,
    pub rating: Option<f32>,
    pub release_date: Option<String>,
    pub show_description: Option<String>,
    pub show_poster_url: Option<String>,
    pub season_poster_url: Option<String>,
    pub episode_still_url: Option<String>,
    pub extra_type: Option<String>, // For extras: "BehindTheScenes", "DeletedScenes", etc.
    pub parent_title: Option<String>, // For extras: title of parent movie/show
}

impl MediaFile {
    pub fn is_tv_episode(&self) -> bool {
        if let Some(metadata) = &self.metadata {
            if let Some(parsed) = &metadata.parsed_info {
                // The server sends MediaType enum which gets serialized as strings
                // MediaType::TvEpisode gets serialized as "TvEpisode" due to PascalCase serde setting
                let media_type_lower = parsed.media_type.to_lowercase();
                return media_type_lower == "tvepisode"
                    || media_type_lower == "tv_episode"
                    || media_type_lower == "episode";
            }
        }
        
        // Fallback: Check filename for common TV episode patterns when no metadata
        let filename_lower = self.filename.to_lowercase();
        
        // Common TV episode patterns: S##E##, #x##, Episode ##
        let tv_patterns = [
            r"s\d{1,2}e\d{1,2}", // S01E01, S1E1
            r"\d{1,2}x\d{1,2}",  // 1x01, 10x05
            r"episode\s*\d+",     // Episode 1, Episode 10
        ];
        
        for pattern in &tv_patterns {
            if regex::Regex::new(pattern).unwrap().is_match(&filename_lower) {
                return true;
            }
        }
        
        false
    }

    pub fn get_show_name(&self) -> Option<String> {
        if let Some(metadata) = &self.metadata {
            if let Some(parsed) = &metadata.parsed_info {
                return parsed.show_name.clone();
            }
        }
        
        // Fallback: Try to extract show name from filename
        if self.is_tv_episode() {
            let filename = &self.filename;
            
            // Try to extract show name before episode pattern
            // Remove file extension first
            let name_without_ext = filename.rsplit_once('.')
                .map(|(name, _)| name)
                .unwrap_or(filename);
            
            // Common patterns to find where show name ends
            let episode_patterns = [
                regex::Regex::new(r"[Ss]\d{1,2}[Ee]\d{1,2}").unwrap(), // S01E01
                regex::Regex::new(r"\d{1,2}x\d{1,2}").unwrap(),       // 1x01
                regex::Regex::new(r"[Ee]pisode\s*\d+").unwrap(),       // Episode 1
            ];
            
            for pattern in &episode_patterns {
                if let Some(match_) = pattern.find(name_without_ext) {
                    let show_name = &name_without_ext[..match_.start()];
                    // Clean up the show name (remove trailing dots, dashes, spaces)
                    let cleaned = show_name.trim_end_matches(|c: char| c == '.' || c == '-' || c == ' ');
                    if !cleaned.is_empty() {
                        // Replace dots with spaces for better formatting
                        return Some(cleaned.replace('.', " "));
                    }
                }
            }
        }
        
        None
    }

    pub fn display_title(&self) -> String {
        if let Some(metadata) = &self.metadata {
            if let Some(parsed) = &metadata.parsed_info {
                if let Some(show) = &parsed.show_name {
                    if let (Some(season), Some(episode)) = (parsed.season, parsed.episode) {
                        return format!("{} S{:02}E{:02}", show, season, episode);
                    }
                    return show.clone();
                }
                return parsed.title.clone();
            }
        }

        // Fallback to filename without extension
        self.filename
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(&self.filename)
            .to_string()
    }

    pub fn display_info(&self) -> String {
        let mut info = Vec::new();

        if let Some(metadata) = &self.metadata {
            // Duration
            let duration = metadata.duration as i64;
            let hours = duration / 3600;
            let minutes = (duration % 3600) / 60;
            if hours > 0 {
                info.push(format!("{}h {}m", hours, minutes));
            } else {
                info.push(format!("{}m", minutes));
            }

            // Resolution
            if let Some(parsed) = &metadata.parsed_info {
                if let Some(res) = &parsed.resolution {
                    info.push(res.clone());
                }
            } else if let (Some(w), Some(h)) = (metadata.width, metadata.height) {
                info.push(format!("{}x{}", w, h));
            }

            // Year
            if let Some(parsed) = &metadata.parsed_info {
                if let Some(year) = parsed.year {
                    info.push(year.to_string());
                }
            }
        }

        info.join(" • ")
    }

    pub fn has_poster(&self) -> bool {
        self.metadata
            .as_ref()
            .and_then(|m| m.external_info.as_ref())
            .and_then(|e| e.poster_url.as_ref())
            .is_some()
    }

    pub fn poster_url(&self, server_url: &str) -> String {
        format!("{}/poster/{}", server_url, self.id)
    }
}

#[derive(Debug, Default)]
pub struct MediaLibrary {
    pub files: Vec<MediaFile>,
    pub server_url: String,
}

impl MediaLibrary {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            server_url: String::new(),
        }
    }

    pub fn set_files(&mut self, files: Vec<MediaFile>) {
        self.files = files;
    }

    pub fn set_server_url(&mut self, server_url: String) {
        self.server_url = server_url;
    }

    pub fn view_grid(&self, poster_cache: &PosterCache) -> Element<Message> {
        if self.files.is_empty() {
            // Return a non-Fill height element for empty state
            return container(text("No media files found").size(18))
                .padding(50)
                .width(Length::Fill)
                .into();
        }

        // Create a grid layout with 4-6 items per row depending on width
        let items_per_row = 5;
        let mut rows: Vec<Element<Message>> = Vec::new();
        let mut current_row: Vec<Element<Message>> = Vec::new();

        for (i, file) in self.files.iter().enumerate() {
            let item = self.create_media_item(file, poster_cache);
            current_row.push(item);

            if current_row.len() >= items_per_row || i == self.files.len() - 1 {
                rows.push(Row::with_children(current_row).spacing(15).into());
                current_row = Vec::new();
            }
        }

        container(Column::with_children(rows).spacing(15).padding(20))
            .width(Length::Fill)
            .into()
    }

    pub fn create_media_item(
        &self,
        file: &MediaFile,
        poster_cache: &PosterCache,
    ) -> Element<Message> {
        let title = file.display_title();
        let info = file.display_info();

        // Create poster element - always check cache
        let poster_element: Element<Message> = match poster_cache.get(&file.id) {
            Some(PosterState::Loaded { thumbnail, .. }) => {
                // Display the loaded poster with fade-in effect
                container(
                    image(thumbnail)
                        .content_fit(iced::ContentFit::Cover)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(300.0))
                .style(container::bordered_box)
                .into()
            }
            Some(PosterState::Loading) => {
                // Show loading state
                container(
                    column![text("⏳").size(32), text("Loading...").size(12)]
                        .align_x(iced::Alignment::Center)
                        .spacing(5),
                )
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(300.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(container::bordered_box)
                .into()
            }
            _ => {
                // Failed, not started, or no poster - show placeholder
                container(text("🎬").size(48))
                    .width(Length::Fixed(200.0))
                    .height(Length::Fixed(300.0))
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
                    .style(container::bordered_box)
                    .into()
            }
        };

        // Media item card
        let content = column![
            poster_element,
            // Title
            text(title).size(14).width(Length::Fixed(200.0)),
            // Info
            text(info)
                .size(12)
                .color(iced::Color::from_rgb(0.7, 0.7, 0.7))
                .width(Length::Fixed(200.0)),
        ]
        .spacing(5);

        button(content)
            .on_press(Message::ViewDetails(file.clone()))
            .padding(10)
            .style(button::secondary)
            .into()
    }
}

/// Fetch all media files (legacy function - for backward compatibility)
pub async fn fetch_library(server_url: String) -> Result<Vec<MediaFile>, anyhow::Error> {
    log::info!("Fetching library from: {}/library", server_url);
    let start = std::time::Instant::now();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client.get(format!("{}/library", server_url)).send().await?;

    log::info!("Server responded in {:?}", start.elapsed());

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    // Get response bytes first
    let bytes = response.bytes().await?;
    log::info!("Downloaded {} bytes in {:?}", bytes.len(), start.elapsed());

    // Move JSON parsing to background thread to avoid blocking UI
    let parse_result = tokio::task::spawn_blocking(move || {
        let parse_start = std::time::Instant::now();
        let json: serde_json::Value = serde_json::from_slice(&bytes)?;
        log::info!("JSON parsed in {:?}", parse_start.elapsed());

        // Check if response has the expected structure
        if let Some(media_files) = json.get("media_files") {
            let deserialize_start = std::time::Instant::now();
            let files: Vec<MediaFile> = serde_json::from_value(media_files.clone())?;
            log::info!(
                "Deserialized {} media files in {:?}",
                files.len(),
                deserialize_start.elapsed()
            );
            Ok(files)
        } else if let Some(error) = json.get("error") {
            Err(anyhow::anyhow!("Server error: {}", error))
        } else {
            // Empty library
            log::info!("Library is empty");
            Ok(Vec::new())
        }
    })
    .await?;

    log::info!("Total fetch time: {:?}", start.elapsed());
    parse_result
}

// TV Show API functions
pub async fn fetch_tv_shows(
    server_url: String,
) -> anyhow::Result<Vec<crate::models::TvShowDetails>> {
    let client = reqwest::Client::new();
    let response = client.get(format!("{}/shows", server_url)).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(shows) = json.get("shows") {
        let show_list: Vec<crate::models::TvShowDetails> = serde_json::from_value(shows.clone())?;
        log::info!("Fetched {} TV shows", show_list.len());
        Ok(show_list)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Ok(Vec::new())
    }
}

pub async fn fetch_tv_show_details(
    server_url: String,
    show_name: String,
) -> anyhow::Result<crate::models::TvShowDetails> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{}/shows/{}",
            server_url,
            url::form_urlencoded::byte_serialize(show_name.as_bytes()).collect::<String>()
        ))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(show) = json.get("show") {
        let show_details: crate::models::TvShowDetails = serde_json::from_value(show.clone())?;
        log::info!("Fetched details for show: {}", show_name);
        Ok(show_details)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Err(anyhow::anyhow!("Show not found"))
    }
}

pub async fn check_posters_batch(
    server_url: &str,
    media_ids: Vec<String>,
) -> Result<Vec<(String, Option<String>)>, String> {
    let url = format!("{}/posters/batch", server_url);
    let client = reqwest::Client::new();

    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "media_ids": media_ids
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to check posters: {}", e))?;

    if response.status().is_success() {
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(posters) = json["posters"].as_array() {
            Ok(posters
                .iter()
                .filter_map(|p| {
                    let media_id = p["media_id"].as_str()?.to_string();
                    let poster_url = p["poster_url"].as_str().map(|s| s.to_string());
                    Some((media_id, poster_url))
                })
                .collect())
        } else {
            Ok(Vec::new())
        }
    } else {
        Err(format!("Server returned error: {}", response.status()))
    }
}

pub async fn queue_missing_metadata(
    server_url: &str,
    media_ids: Vec<String>,
) -> Result<(), String> {
    let url = format!("{}/metadata/queue-missing", server_url);
    let client = reqwest::Client::new();

    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "media_ids": media_ids
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to queue metadata: {}", e))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("Server returned error: {}", response.status()))
    }
}

pub async fn fetch_season_details(
    server_url: String,
    show_name: String,
    season_num: u32,
) -> anyhow::Result<crate::models::SeasonDetails> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{}/shows/{}/seasons/{}",
            server_url,
            url::form_urlencoded::byte_serialize(show_name.as_bytes()).collect::<String>(),
            season_num
        ))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(season) = json.get("season") {
        let season_details: crate::models::SeasonDetails = serde_json::from_value(season.clone())?;
        log::info!(
            "Fetched details for show: {}, season: {}",
            show_name,
            season_num
        );
        Ok(season_details)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Err(anyhow::anyhow!("Season not found"))
    }
}

// Library Management API Functions

/// Fetch all libraries
pub async fn fetch_libraries(server_url: String) -> anyhow::Result<Vec<Library>> {
    log::info!("Fetching libraries from: {}/libraries", server_url);
    let client = reqwest::Client::new();
    let response = client.get(format!("{}/libraries", server_url)).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;
    
    if let Some(libraries) = json.get("libraries") {
        let library_list: Vec<Library> = serde_json::from_value(libraries.clone())?;
        log::info!("Fetched {} libraries", library_list.len());
        Ok(library_list)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Ok(Vec::new())
    }
}

/// Create a new library
pub async fn create_library(
    server_url: String,
    library: Library,
) -> anyhow::Result<Library> {
    log::info!("Creating library: {}", library.name);
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/libraries", server_url))
        .json(&library)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;
    
    if let Some(created_library) = json.get("library") {
        let library: Library = serde_json::from_value(created_library.clone())?;
        log::info!("Created library: {}", library.name);
        Ok(library)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Err(anyhow::anyhow!("Invalid response from server"))
    }
}

/// Update an existing library
pub async fn update_library(
    server_url: String,
    library: Library,
) -> anyhow::Result<Library> {
    log::info!("Updating library: {}", library.name);
    let client = reqwest::Client::new();
    let response = client
        .put(format!("{}/libraries/{}", server_url, library.id))
        .json(&library)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;
    
    if let Some(updated_library) = json.get("library") {
        let library: Library = serde_json::from_value(updated_library.clone())?;
        log::info!("Updated library: {}", library.name);
        Ok(library)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Err(anyhow::anyhow!("Invalid response from server"))
    }
}

/// Delete a library
pub async fn delete_library(server_url: String, library_id: String) -> anyhow::Result<()> {
    log::info!("Deleting library: {}", library_id);
    let client = reqwest::Client::new();
    let response = client
        .delete(format!("{}/libraries/{}", server_url, library_id))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    log::info!("Deleted library: {}", library_id);
    Ok(())
}

/// Scan a specific library
pub async fn scan_library(
    server_url: String,
    library_id: String,
    streaming: bool,
) -> anyhow::Result<String> {
    log::info!("Starting scan for library: {} (streaming: {})", library_id, streaming);
    let client = reqwest::Client::new();
    
    let mut url = format!("{}/libraries/{}/scan", server_url, library_id);
    if streaming {
        url.push_str("?streaming=true");
    }
    
    let response = client.post(&url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;
    
    if let Some(scan_id) = json.get("scan_id").and_then(|id| id.as_str()) {
        log::info!("Started scan with ID: {}", scan_id);
        Ok(scan_id.to_string())
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Scan error: {}", error))
    } else {
        Err(anyhow::anyhow!("Invalid response from server"))
    }
}

/// Fetch media files from a specific library
pub async fn fetch_library_media(
    server_url: String,
    library_id: String,
) -> Result<Vec<MediaFile>, anyhow::Error> {
    log::info!("Fetching media from library: {}", library_id);
    let start = std::time::Instant::now();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client
        .get(format!("{}/libraries/{}/media", server_url, library_id))
        .send()
        .await?;

    log::info!("Server responded in {:?}", start.elapsed());

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    // Get response bytes first
    let bytes = response.bytes().await?;
    log::info!("Downloaded {} bytes in {:?}", bytes.len(), start.elapsed());

    // Move JSON parsing to background thread to avoid blocking UI
    let parse_result = tokio::task::spawn_blocking(move || {
        let parse_start = std::time::Instant::now();
        let json: serde_json::Value = serde_json::from_slice(&bytes)?;
        log::info!("JSON parsed in {:?}", parse_start.elapsed());

        // Check if response has the expected structure
        if let Some(media_files) = json.get("media_files") {
            let deserialize_start = std::time::Instant::now();
            let files: Vec<MediaFile> = serde_json::from_value(media_files.clone())?;
            log::info!(
                "Deserialized {} media files in {:?}",
                files.len(),
                deserialize_start.elapsed()
            );
            Ok(files)
        } else if let Some(error) = json.get("error") {
            Err(anyhow::anyhow!("Server error: {}", error))
        } else {
            // Empty library
            log::info!("Library is empty");
            Ok(Vec::new())
        }
    })
    .await?;

    log::info!("Total fetch time: {:?}", start.elapsed());
    parse_result
}

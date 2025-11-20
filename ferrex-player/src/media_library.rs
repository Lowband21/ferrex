use crate::api_types::{
    ApiResponse, CreateLibraryRequest, LibraryReference, LibraryType, UpdateLibraryRequest,
};
use crate::messages::metadata::Message as MetadataMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub id: Uuid,
    pub name: String,
    pub library_type: LibraryType, // Using core's LibraryType enum
    pub paths: Vec<String>,
    pub scan_interval_minutes: u32,
    pub last_scan: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub media: Vec<crate::api_types::MediaReference>, // Central store of media (MovieReference or SeriesReference)
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
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
    pub framerate: Option<f64>,
    pub file_size: u64,
    // HDR metadata
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub bit_depth: Option<u32>,
    pub parsed_info: Option<ParsedInfo>,
    pub external_info: Option<ExternalInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedInfo {
    pub media_type: String,
    pub title: String,
    pub year: Option<u16>,
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
    pub fn is_hdr(&self) -> bool {
        if let Some(metadata) = &self.metadata {
            // Check color transfer for HDR indicators
            if let Some(transfer) = &metadata.color_transfer {
                // PQ (SMPTE 2084) is HDR10/Dolby Vision
                // HLG (ARIB STD-B67) is Hybrid Log-Gamma
                if transfer == "smpte2084" || transfer == "arib-std-b67" {
                    return true;
                }
            }

            // Check color space for Dolby Vision
            if let Some(space) = &metadata.color_space {
                if space == "ictcp" {
                    return true;
                }
            }

            // DO NOT check bit depth alone - 10-bit doesn't mean HDR
            // We already checked for HDR transfer functions above
            // Any other case is SDR, even if it's 10-bit with BT.2020

            // DO NOT guess HDR based on resolution or filename
            // This causes false positives when server hasn't extracted metadata
        }

        false
    }

    pub fn get_video_info(&self) -> String {
        let mut info = Vec::new();

        if let Some(metadata) = &self.metadata {
            // Video codec
            if let Some(codec) = &metadata.video_codec {
                info.push(codec.to_uppercase());
            }

            // Resolution
            if let (Some(width), Some(height)) = (metadata.width, metadata.height) {
                info.push(format!("{}x{}", width, height));
            }

            // HDR info
            if self.is_hdr() {
                if let Some(transfer) = &metadata.color_transfer {
                    if transfer == "smpte2084" {
                        info.push("HDR10".to_string());
                    } else if transfer == "arib-std-b67" {
                        info.push("HLG".to_string());
                    }
                }
                if let Some(space) = &metadata.color_space {
                    if space == "ictcp" {
                        info.push("Dolby Vision".to_string());
                    }
                }
            }

            // Bit depth
            if let Some(depth) = metadata.bit_depth {
                info.push(format!("{}-bit", depth));
            }
        }

        info.join(" • ")
    }

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
            r"episode\s*\d+",    // Episode 1, Episode 10
        ];

        for pattern in &tv_patterns {
            if regex::Regex::new(pattern)
                .unwrap()
                .is_match(&filename_lower)
            {
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
            let name_without_ext = filename
                .rsplit_once('.')
                .map(|(name, _)| name)
                .unwrap_or(filename);

            // Common patterns to find where show name ends
            let episode_patterns = [
                regex::Regex::new(r"[Ss]\d{1,2}[Ee]\d{1,2}").unwrap(), // S01E01
                regex::Regex::new(r"\d{1,2}x\d{1,2}").unwrap(),        // 1x01
                regex::Regex::new(r"[Ee]pisode\s*\d+").unwrap(),       // Episode 1
            ];

            for pattern in &episode_patterns {
                if let Some(match_) = pattern.find(name_without_ext) {
                    let show_name = &name_without_ext[..match_.start()];
                    // Clean up the show name (remove trailing dots, dashes, spaces)
                    let cleaned =
                        show_name.trim_end_matches(|c: char| c == '.' || c == '-' || c == ' ');
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
            if let Some(duration_f64) = metadata.duration {
                let duration = duration_f64 as i64;
                let hours = duration / 3600;
                let minutes = (duration % 3600) / 60;
                if hours > 0 {
                    info.push(format!("{}h {}m", hours, minutes));
                } else {
                    info.push(format!("{}m", minutes));
                }
            }

            // Video codec
            if let Some(codec) = &metadata.video_codec {
                info.push(codec.to_uppercase());
            }

            // Resolution
            if let Some(parsed) = &metadata.parsed_info {
                if let Some(res) = &parsed.resolution {
                    info.push(res.clone());
                }
            } else if let (Some(w), Some(h)) = (metadata.width, metadata.height) {
                info.push(format!("{}x{}", w, h));
            }

            // HDR info
            if self.is_hdr() {
                if let Some(transfer) = &metadata.color_transfer {
                    if transfer == "smpte2084" {
                        info.push("HDR10".to_string());
                    } else if transfer == "arib-std-b67" {
                        info.push("HLG".to_string());
                    }
                }
                if let Some(space) = &metadata.color_space {
                    if space == "ictcp" {
                        info.push("DV".to_string());
                    }
                }
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

    /// DEPRECATED: This uses the old /poster/{id} endpoint
    /// New code should use MediaReference types with /images/ endpoints
    #[deprecated(
        since = "0.1.0",
        note = "Use MediaReference types with new /images/ endpoints"
    )]
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

// Movie API functions
pub async fn fetch_movies(server_url: String) -> anyhow::Result<Vec<crate::models::MovieDetails>> {
    log::info!("Fetching movies from: {}/movies", server_url);
    let start = std::time::Instant::now();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client.get(format!("{}/movies", server_url)).send().await?;

    log::info!("Server responded in {:?}", start.elapsed());

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(movies) = json.get("movies") {
        let movie_list: Vec<crate::models::MovieDetails> = serde_json::from_value(movies.clone())?;
        log::info!(
            "Fetched {} movies in {:?}",
            movie_list.len(),
            start.elapsed()
        );
        Ok(movie_list)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Ok(Vec::new())
    }
}

pub async fn fetch_movie_details(
    server_url: String,
    movie_id: String,
) -> anyhow::Result<crate::models::MovieDetails> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/movies/{}", server_url, movie_id))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(movie) = json.get("movie") {
        let movie_details: crate::models::MovieDetails = serde_json::from_value(movie.clone())?;
        log::info!("Fetched details for movie ID: {}", movie_id);
        Ok(movie_details)
    } else if let Some(error) = json.get("error") {
        Err(anyhow::anyhow!("Server error: {}", error))
    } else {
        Err(anyhow::anyhow!("Movie not found"))
    }
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
    let response = client
        .get(format!("{}/libraries", server_url))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned error: {}",
            response.status()
        ));
    }

    // Parse the response as ApiResponse<Vec<LibraryReference>>
    let api_response: ApiResponse<Vec<LibraryReference>> = response.json().await?;

    if api_response.status == "success" {
        if let Some(library_refs) = api_response.data {
            // Convert LibraryReference to Library with empty media_references
            let library_list: Vec<Library> = library_refs
                .into_iter()
                .map(|lib_ref| Library {
                    id: lib_ref.id,
                    name: lib_ref.name,
                    library_type: lib_ref.library_type,
                    paths: lib_ref
                        .paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    scan_interval_minutes: 60, // Default value, actual value should be fetched separately
                    last_scan: None,           // Not included in LibraryReference
                    enabled: true, // Default to enabled, actual value should be fetched separately
                    media: Vec::new(), // Empty initially
                })
                .collect();
            log::info!("Fetched {} libraries", library_list.len());
            Ok(library_list)
        } else {
            log::warn!("No library data in response");
            Ok(vec![])
        }
    } else {
        Err(anyhow::anyhow!(
            "API error: {}",
            api_response
                .error
                .unwrap_or_else(|| "Unknown error".to_string())
        ))
    }
}

/// Create a new library
pub async fn create_library(server_url: String, library: Library) -> anyhow::Result<Library> {
    log::info!("Creating library: {}", library.name);

    // Convert Library to CreateLibraryRequest
    let create_request = CreateLibraryRequest {
        name: library.name.clone(),
        library_type: library.library_type,
        paths: library.paths.clone(),
        scan_interval_minutes: library.scan_interval_minutes,
        enabled: library.enabled,
    };

    // Log the JSON being sent
    match serde_json::to_string_pretty(&create_request) {
        Ok(json_str) => log::debug!("Sending CreateLibraryRequest JSON: {}", json_str),
        Err(e) => log::error!("Failed to serialize request for logging: {}", e),
    }

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/libraries", server_url))
        .json(&create_request)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!("Server returned error: {}", error_text));
    }

    // Parse the response as ApiResponse<uuid::Uuid>
    let api_response: ApiResponse<uuid::Uuid> = response.json().await?;

    // Debug log the entire response
    log::debug!("Server response: {:?}", api_response);

    // Check if the response was successful and has data
    if api_response.status == "success" {
        if let Some(library_id) = api_response.data {
            // Server returned the library ID, we need to fetch the full library
            log::info!(
                "Library created with ID: {}, fetching full details",
                library_id
            );

            // Fetch the created library by ID
            let fetch_response = client
                .get(format!("{}/libraries/{}", server_url, library_id))
                .send()
                .await?;

            if !fetch_response.status().is_success() {
                let error_text = fetch_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(anyhow::anyhow!(
                    "Failed to fetch created library: {}",
                    error_text
                ));
            }

            // Parse as ApiResponse<LibraryReference>
            let library_response: ApiResponse<LibraryReference> = fetch_response.json().await?;

            if let Some(library_ref) = library_response.data {
                // Convert LibraryReference to legacy Library
                let created_library = Library {
                    id: library_ref.id,
                    name: library_ref.name,
                    library_type: library_ref.library_type,
                    paths: library_ref
                        .paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                    scan_interval_minutes: library.scan_interval_minutes, // Use the value we sent
                    last_scan: None,          // New library has no last scan
                    enabled: library.enabled, // Use the value we sent
                    media: Vec::new(), // Empty initially, will be populated when media is loaded
                };
                log::info!(
                    "Successfully fetched created library: {}",
                    created_library.name
                );
                return Ok(created_library);
            } else {
                return Err(anyhow::anyhow!("Server returned empty library data"));
            }
        } else {
            return Err(anyhow::anyhow!("Server response missing library ID"));
        }
    } else {
        return Err(anyhow::anyhow!(
            "API error: {}",
            api_response
                .error
                .unwrap_or_else(|| "Unknown error".to_string())
        ));
    }
}

/// Update an existing library
pub async fn update_library(server_url: String, library: Library) -> anyhow::Result<Library> {
    log::info!("Updating library: {}", library.name);

    // Create UpdateLibraryRequest with only the fields that can be updated
    let update_request = UpdateLibraryRequest {
        name: Some(library.name.clone()),
        paths: Some(library.paths.clone()),
        scan_interval_minutes: Some(library.scan_interval_minutes),
        enabled: Some(library.enabled),
    };

    let client = reqwest::Client::new();
    let response = client
        .put(format!("{}/libraries/{}", server_url, library.id))
        .json(&update_request)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!("Server returned error: {}", error_text));
    }

    // Parse the response as ApiResponse<String>
    let api_response: ApiResponse<String> = response.json().await?;

    if api_response.status == "success" {
        // Return the library with preserved media_references
        log::info!("Updated library: {}", library.name);
        Ok(library)
    } else {
        Err(anyhow::anyhow!(
            "API error: {}",
            api_response
                .error
                .unwrap_or_else(|| "Unknown error".to_string())
        ))
    }
}

/// Delete a library
pub async fn delete_library(server_url: String, library_id: Uuid) -> anyhow::Result<()> {
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
    library_id: Uuid,
    streaming: bool,
) -> anyhow::Result<String> {
    log::info!(
        "Starting scan for library: {} (streaming: {})",
        library_id,
        streaming
    );
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
pub async fn fetch_media_by_id(
    server_url: String,
    media_id: String,
) -> Result<MediaFile, anyhow::Error> {
    let url = format!("{}/media/{}", server_url, media_id);
    log::debug!("Fetching media from: {}", url);

    let response = reqwest::get(&url).await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch media: {}",
            response.status()
        ));
    }

    let media = response.json::<MediaFile>().await?;
    Ok(media)
}

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
        .get(format!("{}/library?library_id={}", server_url, library_id))
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

// ===== NEW API FUNCTIONS FOR REFERENCE-BASED SYSTEM =====

use crate::api_types::{
    FetchMediaRequest, LibraryMediaResponse, MediaId, MediaReference, SeasonReference,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Fetch lightweight media references for a library
pub async fn fetch_library_media_references(
    server_url: String,
    library_id: Uuid,
) -> anyhow::Result<LibraryMediaResponse> {
    log::info!("Fetching media references for library: {}", library_id);
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
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!(
            "Server returned error {}: {}",
            status,
            error_text
        ));
    }

    let bytes = response.bytes().await?;
    log::info!("Downloaded {} bytes in {:?}", bytes.len(), start.elapsed());

    // Parse on background thread
    let parse_result = tokio::task::spawn_blocking(move || {
        let parse_start = std::time::Instant::now();
        let api_response: ApiResponse<LibraryMediaResponse> = serde_json::from_slice(&bytes)?;
        log::info!("JSON parsed in {:?}", parse_start.elapsed());

        match api_response.data {
            Some(data) => {
                log::info!("Fetched {} media references", data.media.len());
                Ok(data)
            }
            None => {
                let error = api_response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string());
                Err(anyhow::anyhow!("Server error: {}", error))
            }
        }
    })
    .await??;

    log::info!("Total fetch time: {:?}", start.elapsed());
    Ok(parse_result)
}

/// Fetch full details for a specific media item
pub async fn fetch_media_details(
    server_url: String,
    library_id: Uuid,
    media_id: MediaId,
) -> anyhow::Result<MediaReference> {
    log::debug!("Fetching details for media: {:?}", media_id);

    let client = reqwest::Client::new();

    // The /api/media/:id endpoint expects the media ID in the URL path
    let media_id_str = match &media_id {
        MediaId::Movie(id) => id.as_str(),
        MediaId::Series(id) => id.as_str(),
        MediaId::Season(id) => id.as_str(),
        MediaId::Episode(id) => id.as_str(),
        MediaId::Person(id) => id.as_str(),
    };

    let response = client
        .get(format!("{}/api/media/{}", server_url, media_id_str))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow::anyhow!(
            "Server returned error {}: {}",
            status,
            error_text
        ));
    }

    // The /api/media/:id endpoint returns ApiResponse<MediaReference>
    let api_response: ApiResponse<MediaReference> = response.json().await?;

    match api_response.data {
        Some(media_reference) => Ok(media_reference),
        None => {
            let error = api_response
                .error
                .unwrap_or_else(|| "No data in response".to_string());
            Err(anyhow::anyhow!("API error: {}", error))
        }
    }
}

/// Fetch full details for multiple media items in a single request
pub async fn fetch_media_details_batch(
    api_client: &crate::api_client::ApiClient,
    library_id: Uuid,
    media_ids: Vec<MediaId>,
) -> anyhow::Result<crate::api_types::BatchMediaResponse> {
    if media_ids.is_empty() {
        return Ok(crate::api_types::BatchMediaResponse {
            items: vec![],
            errors: vec![],
        });
    }

    log::info!("Fetching batch details for {} media items", media_ids.len());

    let request = crate::api_types::BatchMediaRequest {
        library_id,
        media_ids,
    };

    // Use the authenticated API client
    let response: Result<crate::api_types::BatchMediaResponse, anyhow::Error> =
        api_client.post("/api/media/batch", &request).await;

    match response {
        Ok(batch_response) => {
            log::info!(
                "Successfully fetched batch of {} items",
                batch_response.items.len()
            );
            if !batch_response.errors.is_empty() {
                log::warn!("Batch fetch had {} errors", batch_response.errors.len());
            }
            Ok(batch_response)
        }
        Err(e) => {
            log::error!("Batch request failed: {}", e);
            // Check specifically for authentication errors
            if e.to_string().contains("401") || e.to_string().contains("Unauthorized") {
                log::error!("Authentication error in batch fetch - API client may not have valid auth token");
            }
            // For now, return an error instead of falling back
            // This will help identify auth issues
            Err(anyhow::anyhow!("Batch fetch failed: {}", e))
        }
    }
}

/// Fallback to individual requests if batch endpoint is not available
async fn fetch_media_details_fallback(
    server_url: String,
    library_id: Uuid,
    media_ids: Vec<MediaId>,
) -> anyhow::Result<crate::api_types::BatchMediaResponse> {
    use futures::future::join_all;

    let mut items = Vec::new();
    let mut errors = Vec::new();

    // Create futures for all requests
    let futures: Vec<_> = media_ids
        .into_iter()
        .map(|media_id| {
            let server_url = server_url.clone();
            async move {
                let result = fetch_media_details(server_url, library_id, media_id.clone()).await;
                (media_id, result)
            }
        })
        .collect();

    // Execute all requests concurrently (limited by reqwest's connection pool)
    let results = join_all(futures).await;

    for (media_id, result) in results {
        match result {
            Ok(media_ref) => items.push(media_ref),
            Err(e) => errors.push((media_id, e.to_string())),
        }
    }

    Ok(crate::api_types::BatchMediaResponse { items, errors })
}

/// Background details fetcher that can be used to fetch details for visible items
#[derive(Debug)]
pub struct BackgroundDetailsFetcher {
    server_url: String,
    pub library_id: Uuid,
    fetch_queue: Arc<RwLock<Vec<MediaId>>>,
    completed_items: Arc<RwLock<Vec<MediaReference>>>,
    message_sender: Option<tokio::sync::mpsc::UnboundedSender<MetadataMessage>>,
    pub metadata_cache: Arc<RwLock<HashMap<MediaId, MediaReference>>>,
}

impl BackgroundDetailsFetcher {
    pub fn new(server_url: String, library_id: Uuid) -> Self {
        Self {
            server_url,
            library_id,
            fetch_queue: Arc::new(RwLock::new(Vec::new())),
            completed_items: Arc::new(RwLock::new(Vec::new())),
            message_sender: None,
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_message_sender(
        mut self,
        sender: tokio::sync::mpsc::UnboundedSender<MetadataMessage>,
    ) -> Self {
        self.message_sender = Some(sender);
        self
    }

    pub fn with_cache(mut self, cache: Arc<RwLock<HashMap<MediaId, MediaReference>>>) -> Self {
        self.metadata_cache = cache;
        self
    }

    /// Queue media items for background detail fetching
    pub async fn queue_items(&self, items: Vec<MediaId>) {
        let mut queue = self.fetch_queue.write().await;
        for item in items {
            if !queue.contains(&item) {
                queue.push(item);
            }
        }
    }

    /// Poll for completed items (used by the UI subscription)
    pub async fn poll_completed(&self) -> Vec<MediaReference> {
        let mut completed = self.completed_items.write().await;
        let items = completed.drain(..).collect();
        items
    }

    /// Start background fetching (returns a handle to the task)
    pub fn start_fetching(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                // Get next item from queue
                let next_item = {
                    let mut queue = self.fetch_queue.write().await;
                    queue.pop()
                };

                match next_item {
                    Some(media_id) => {
                        // Check cache first
                        let cached = self.metadata_cache.read().await.get(&media_id).cloned();

                        if let Some(media) = cached {
                            log::debug!("Found cached details for {:?}", media_id);
                            // Store in completed items for polling
                            self.completed_items.write().await.push(media.clone());
                            // Also send update message to UI if sender is available
                            if let Some(sender) = &self.message_sender {
                                let _ = sender.send(MetadataMessage::MediaDetailsUpdated(media));
                            }
                        } else {
                            // Fetch details in background
                            match fetch_media_details(
                                self.server_url.clone(),
                                self.library_id,
                                media_id.clone(),
                            )
                            .await
                            {
                                Ok(media) => {
                                    log::debug!("Fetched details for {:?}", media_id);
                                    // Store in cache
                                    self.metadata_cache
                                        .write()
                                        .await
                                        .insert(media_id.clone(), media.clone());
                                    // Store in completed items for polling
                                    self.completed_items.write().await.push(media.clone());
                                    // Also send update message to UI if sender is available
                                    if let Some(sender) = &self.message_sender {
                                        let _ = sender
                                            .send(MetadataMessage::MediaDetailsUpdated(media));
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to fetch details for {:?}: {}", media_id, e);
                                }
                            }
                        }
                    }
                    None => {
                        // Queue is empty, wait a bit
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }
        })
    }
}

/// Fetch details for all seasons of a series in the background
pub async fn fetch_series_season_details(
    server_url: String,
    library_id: Uuid,
    _series_id: crate::api_types::SeriesID,
    season_refs: Vec<SeasonReference>,
) -> Vec<SeasonReference> {
    let tasks: Vec<_> = season_refs
        .into_iter()
        .map(|season| {
            let server_url = server_url.clone();
            let library_id = library_id;
            let season_id = season.id.clone();

            tokio::spawn(async move {
                match fetch_media_details(server_url, library_id, MediaId::Season(season_id)).await
                {
                    Ok(MediaReference::Season(updated_season)) => Some(updated_season),
                    Ok(_) => {
                        log::warn!("Unexpected media type returned for season");
                        None
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch season details: {}", e);
                        None
                    }
                }
            })
        })
        .collect();

    // Wait for all tasks to complete
    let results = futures::future::join_all(tasks).await;

    results
        .into_iter()
        .filter_map(|result| result.ok().flatten())
        .collect()
}

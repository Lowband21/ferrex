use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MasterPlaylist {
    pub variants: Vec<Variant>,
    pub media_id: String,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub bandwidth: u64,
    pub resolution: Option<(u32, u32)>, // width, height
    pub profile: String,
    pub playlist_url: String,
}

#[derive(Debug, Clone)]
pub struct VariantPlaylist {
    pub segments: Vec<Segment>,
    pub target_duration: f64,
    pub media_sequence: u64,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub duration: f64,
    pub url: String,
    pub sequence_number: u64,
}

// Use shared types from ferrex-core

#[derive(Debug)]
struct HlsClientInner {
    current_variant: Option<Variant>,
    segment_cache: HashMap<String, Vec<u8>>,
    bandwidth_history: Vec<(Instant, u64)>, // (time, bits per second)
}

#[derive(Debug, Clone)]
pub struct HlsClient {
    http_client: Client,
    server_url: String,
    inner: Arc<Mutex<HlsClientInner>>,
}

impl HlsClient {
    pub fn new(server_url: String) -> Self {
        Self {
            http_client: Client::builder()
                // Don't set a global timeout, use per-request timeouts instead
                .pool_max_idle_per_host(4)
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .unwrap_or_else(|_| Client::new()),
            server_url,
            inner: Arc::new(Mutex::new(HlsClientInner {
                current_variant: None,
                segment_cache: HashMap::new(),
                bandwidth_history: Vec::new(),
            })),
        }
    }

    /// Start adaptive transcoding for a media file with retry logic
    pub async fn start_adaptive_transcoding_with_retry(&self, media_id: &str, max_retries: u32) -> Result<String, String> {
        let mut retries = 0;
        let mut last_error = String::new();
        
        while retries <= max_retries {
            match self.start_adaptive_transcoding(media_id).await {
                Ok(job_id) => return Ok(job_id),
                Err(e) => {
                    last_error = e;
                    retries += 1;
                    
                    if retries <= max_retries {
                        log::warn!("Transcoding start failed, retrying ({}/{}): {}", retries, max_retries, last_error);
                        // Longer delays: 2s, 4s, 8s
                        let delay_secs = 2u64.saturating_mul(1 << (retries - 1));
                        log::info!("Waiting {} seconds before retry...", delay_secs);
                        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    }
                }
            }
        }
        
        Err(format!("Failed after {} retries: {}", max_retries, last_error))
    }
    
    /// Start adaptive transcoding for a media file
    pub async fn start_adaptive_transcoding(&self, media_id: &str) -> Result<String, String> {
        let url = format!("{}/transcode/{}/adaptive", self.server_url, media_id);
        
        log::info!("=== ADAPTIVE TRANSCODING REQUEST ===");
        log::info!("Media ID: {}", media_id);
        log::info!("POST URL: {}", url);
        log::info!("Timestamp: {:?}", std::time::SystemTime::now());
        
        let start_time = std::time::Instant::now();
        let response = self.http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .body("{}") // Empty JSON body
            .timeout(Duration::from_secs(60)) // Increased timeout for transcoding initialization
            .send()
            .await
            .map_err(|e| {
                let elapsed = start_time.elapsed();
                log::error!("Failed to start transcoding after {:?}: {}", elapsed, e);
                format!("Failed to start transcoding: {}", e)
            })?;
        
        let elapsed = start_time.elapsed();
        log::info!("Transcoding request completed in {:?}", elapsed);
            
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Server error {}: {}", status, body));
        }
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
            
        let master_job_id = json["master_job_id"]
            .as_str()
            .ok_or("No master_job_id in response")?
            .to_string();
            
        log::info!("Adaptive transcoding started with job ID: {}", master_job_id);
        Ok(master_job_id)
    }
    
    /// Check transcoding job status
    pub async fn check_transcoding_status(&self, job_id: &str) -> Result<ferrex_core::TranscodingJobResponse, String> {
        let url = format!("{}/transcode/status/{}", self.server_url, job_id);
        
        log::debug!("Checking transcoding status for job: {}", job_id);
        
        let response = self.http_client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("Failed to check status: {}", e))?;
            
        if !response.status().is_success() {
            return Err(format!("Status check failed: {}", response.status()));
        }
        
        let job_response: ferrex_core::TranscodingJobResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse status: {}", e))?;
            
        Ok(job_response)
    }

    /// Fetch and parse the master playlist with retry logic
    pub async fn fetch_master_playlist_with_retry(&self, media_id: &str, max_retries: u32) -> Result<MasterPlaylist, String> {
        let mut retries = 0;
        let mut last_error = String::new();
        
        while retries <= max_retries {
            match self.fetch_master_playlist(media_id).await {
                Ok(playlist) => return Ok(playlist),
                Err(e) => {
                    last_error = e;
                    retries += 1;
                    
                    if retries <= max_retries {
                        log::warn!("Master playlist fetch failed, retrying ({}/{}): {}", retries, max_retries, last_error);
                        tokio::time::sleep(Duration::from_secs(2)).await; // Wait 2 seconds between retries
                    }
                }
            }
        }
        
        Err(format!("Master playlist not available after {} retries: {}", max_retries, last_error))
    }
    
    /// Fetch and parse the master playlist
    pub async fn fetch_master_playlist(&self, media_id: &str) -> Result<MasterPlaylist, String> {
        let url = format!("{}/transcode/{}/master.m3u8", self.server_url, media_id);
        
        log::info!("Fetching master playlist: {}", url);
        
        let response = self.http_client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch master playlist: {}", e))?;
            
        if !response.status().is_success() {
            return Err(format!("Master playlist not ready: {}", response.status()));
        }
        
        let content = response
            .text()
            .await
            .map_err(|e| format!("Failed to read playlist: {}", e))?;
            
        self.parse_master_playlist(&content, media_id)
    }

    /// Parse master playlist content
    fn parse_master_playlist(&self, content: &str, media_id: &str) -> Result<MasterPlaylist, String> {
        let mut variants = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            
            if line.starts_with("#EXT-X-STREAM-INF:") {
                // Parse stream info
                let info = &line[18..]; // Skip "#EXT-X-STREAM-INF:"
                let mut bandwidth = 0;
                let mut resolution = None;
                
                // Parse attributes
                for attr in info.split(',') {
                    let parts: Vec<&str> = attr.split('=').collect();
                    if parts.len() == 2 {
                        match parts[0] {
                            "BANDWIDTH" => {
                                bandwidth = parts[1].parse().unwrap_or(0);
                            }
                            "RESOLUTION" => {
                                let res_parts: Vec<&str> = parts[1].split('x').collect();
                                if res_parts.len() == 2 {
                                    if let (Ok(w), Ok(h)) = (res_parts[0].parse::<u32>(), res_parts[1].parse::<u32>()) {
                                        resolution = Some((w, h));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                
                // Next line should be the playlist URL
                if i + 1 < lines.len() {
                    let playlist_path = lines[i + 1].trim();
                    if !playlist_path.starts_with('#') {
                        // Extract profile from URL (e.g., "adaptive_360p", "adaptive_720p", etc.)
                        // URL format: variant/adaptive_1080p/playlist.m3u8
                        let profile = playlist_path
                            .split('/')
                            .nth(1)  // Get the second part (adaptive_xxx)
                            .unwrap_or("unknown")
                            .to_string();
                            
                        let playlist_url = if playlist_path.starts_with("http") {
                            playlist_path.to_string()
                        } else {
                            format!("{}/transcode/{}/{}", self.server_url, media_id, playlist_path)
                        };
                        
                        variants.push(Variant {
                            bandwidth,
                            resolution,
                            profile,
                            playlist_url,
                        });
                        
                        i += 1; // Skip the URL line
                    }
                }
            }
            
            i += 1;
        }
        
        if variants.is_empty() {
            return Err("No variants found in master playlist".to_string());
        }
        
        // Sort variants by bandwidth
        variants.sort_by_key(|v| v.bandwidth);
        
        Ok(MasterPlaylist {
            variants,
            media_id: media_id.to_string(),
        })
    }

    /// Select the best variant based on current bandwidth
    pub fn select_variant<'a>(&self, master_playlist: &'a MasterPlaylist) -> &'a Variant {
        // Calculate average bandwidth from recent history
        let avg_bandwidth = self.calculate_average_bandwidth();
        
        // Find the best variant
        // Use 80% of available bandwidth to leave headroom
        let target_bandwidth = (avg_bandwidth as f64 * 0.8) as u64;
        
        let variant = master_playlist.variants
            .iter()
            .filter(|v| v.bandwidth <= target_bandwidth)
            .last()
            .unwrap_or(&master_playlist.variants[0]); // Fallback to lowest quality
            
        log::info!(
            "Selected variant: {} ({}bps) for bandwidth {}bps",
            variant.profile,
            variant.bandwidth,
            avg_bandwidth
        );
        
        {
            let mut inner = self.inner.lock().unwrap();
            inner.current_variant = Some(variant.clone());
        }
        
        variant
    }

    /// Fetch and parse a variant playlist
    pub async fn fetch_variant_playlist(&self, variant: &Variant) -> Result<VariantPlaylist, String> {
        log::info!("Fetching variant playlist: {}", variant.playlist_url);
        
        let response = self.http_client
            .get(&variant.playlist_url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch variant playlist: {}", e))?;
            
        if !response.status().is_success() {
            return Err(format!("Variant playlist error: {}", response.status()));
        }
        
        let content = response
            .text()
            .await
            .map_err(|e| format!("Failed to read playlist: {}", e))?;
            
        self.parse_variant_playlist(&content)
    }

    /// Parse variant playlist content
    fn parse_variant_playlist(&self, content: &str) -> Result<VariantPlaylist, String> {
        let mut segments = Vec::new();
        let mut target_duration = 4.0;
        let mut media_sequence = 0;
        let lines: Vec<&str> = content.lines().collect();
        
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            
            if line.starts_with("#EXT-X-TARGETDURATION:") {
                target_duration = line[22..]
                    .parse()
                    .unwrap_or(4.0);
            } else if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
                media_sequence = line[22..]
                    .parse()
                    .unwrap_or(0);
            } else if line.starts_with("#EXTINF:") {
                // Parse segment duration
                let duration_str = &line[8..];
                let duration = duration_str
                    .split(',')
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(4.0);
                    
                // Next line should be the segment URL
                if i + 1 < lines.len() {
                    let segment_path = lines[i + 1].trim();
                    if !segment_path.starts_with('#') {
                        let segment_url = if segment_path.starts_with("http") {
                            segment_path.to_string()
                        } else {
                            format!("{}{}", self.server_url, segment_path)
                        };
                        
                        segments.push(Segment {
                            duration,
                            url: segment_url,
                            sequence_number: media_sequence + segments.len() as u64,
                        });
                        
                        i += 1; // Skip the URL line
                    }
                }
            }
            
            i += 1;
        }
        
        Ok(VariantPlaylist {
            segments,
            target_duration,
            media_sequence,
        })
    }

    /// Fetch a segment with bandwidth tracking
    pub async fn fetch_segment(&self, segment: &Segment) -> Result<Vec<u8>, String> {
        // Check cache first
        {
            let inner = self.inner.lock().unwrap();
            if let Some(data) = inner.segment_cache.get(&segment.url) {
                log::debug!("Segment {} found in cache", segment.sequence_number);
                return Ok(data.clone());
            }
        }
        
        log::info!("Fetching segment {}: {}", segment.sequence_number, segment.url);
        
        let start = Instant::now();
        let response = self.http_client
            .get(&segment.url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch segment: {}", e))?;
            
        if !response.status().is_success() {
            return Err(format!("Segment fetch error: {}", response.status()));
        }
        
        let data = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read segment: {}", e))?
            .to_vec();
            
        let duration = start.elapsed();
        
        // Calculate bandwidth (bits per second)
        let bandwidth = ((data.len() as f64 * 8.0) / duration.as_secs_f64()) as u64;
        self.record_bandwidth(bandwidth);
        
        log::info!(
            "Segment {} downloaded: {} bytes in {:?} ({}bps)",
            segment.sequence_number,
            data.len(),
            duration,
            bandwidth
        );
        
        // Cache the segment and limit cache size
        {
            let mut inner = self.inner.lock().unwrap();
            inner.segment_cache.insert(segment.url.clone(), data.clone());
            
            // Limit cache size
            if inner.segment_cache.len() > 10 {
                // Remove oldest segments
                let keys: Vec<String> = inner.segment_cache.keys().cloned().collect();
                if let Some(oldest) = keys.first() {
                    inner.segment_cache.remove(oldest);
                }
            }
        }
        
        Ok(data)
    }

    /// Record bandwidth measurement
    fn record_bandwidth(&self, bandwidth: u64) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner.bandwidth_history.push((now, bandwidth));
        
        // Keep only recent measurements (last 30 seconds)
        inner.bandwidth_history.retain(|(time, _)| {
            now.duration_since(*time).as_secs() < 30
        });
    }

    /// Calculate average bandwidth from recent history
    fn calculate_average_bandwidth(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        if inner.bandwidth_history.is_empty() {
            // Default to 2 Mbps if no history
            return 2_000_000;
        }
        
        let sum: u64 = inner.bandwidth_history.iter().map(|(_, bw)| bw).sum();
        sum / inner.bandwidth_history.len() as u64
    }

    /// Check if we should switch to a different quality variant
    pub fn should_switch_variant<'a>(&self, master_playlist: &'a MasterPlaylist) -> Option<&'a Variant> {
        let avg_bandwidth = self.calculate_average_bandwidth();
        let inner = self.inner.lock().unwrap();
        let current = inner.current_variant.as_ref()?;
        
        // Check if we should switch up
        if avg_bandwidth > current.bandwidth * 120 / 100 { // 20% headroom
            // Find next higher quality
            master_playlist.variants
                .iter()
                .find(|v| v.bandwidth > current.bandwidth && v.bandwidth <= avg_bandwidth * 80 / 100)
        } else if avg_bandwidth < current.bandwidth * 80 / 100 { // Below 80% of current
            // Find next lower quality
            master_playlist.variants
                .iter()
                .rev()
                .find(|v| v.bandwidth < current.bandwidth)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_master_playlist() {
        let client = HlsClient::new("http://localhost:8000".to_string());
        let content = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-STREAM-INF:BANDWIDTH=865000,RESOLUTION=640x360
/transcode/123/variant/360p/playlist.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2100000,RESOLUTION=854x480
/transcode/123/variant/480p/playlist.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=4100000,RESOLUTION=1280x720
/transcode/123/variant/720p/playlist.m3u8"#;

        let playlist = client.parse_master_playlist(content, "123").unwrap();
        assert_eq!(playlist.variants.len(), 3);
        assert_eq!(playlist.variants[0].bandwidth, 865000);
        assert_eq!(playlist.variants[0].resolution, Some((640, 360)));
        assert_eq!(playlist.variants[0].profile, "360p");
    }

    #[test]
    fn test_parse_variant_playlist() {
        let client = HlsClient::new("http://localhost:8000".to_string());
        let content = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:4.0,
/transcode/job123/segment/0
#EXTINF:4.0,
/transcode/job123/segment/1
#EXTINF:4.0,
/transcode/job123/segment/2"#;

        let playlist = client.parse_variant_playlist(content).unwrap();
        assert_eq!(playlist.segments.len(), 3);
        assert_eq!(playlist.target_duration, 4.0);
        assert_eq!(playlist.media_sequence, 0);
        assert_eq!(playlist.segments[0].duration, 4.0);
        assert_eq!(playlist.segments[0].sequence_number, 0);
    }
}
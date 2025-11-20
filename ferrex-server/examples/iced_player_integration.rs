// Example integration for Iced player with Ferrex transcoding service

use std::time::{Duration, Instant};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct FerrexStreamingSource {
    server_url: String,
    media_id: String,
    client: Client,
    current_variant: String,
    master_job_id: Option<String>,
    segment_cache: Vec<(u32, Vec<u8>)>, // Simple cache for demo
}

#[derive(Deserialize)]
struct TranscodeResponse {
    status: String,
    master_job_id: Option<String>,
    job_id: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct JobStatus {
    status: String,
    job: Option<Job>,
}

#[derive(Deserialize)]
struct Job {
    id: String,
    media_id: String,
    profile: String,
    status: String,
    progress: Option<f32>,
}

impl FerrexStreamingSource {
    pub fn new(server_url: String, media_id: String) -> Self {
        Self {
            server_url,
            media_id,
            client: Client::new(),
            current_variant: "720p".to_string(), // Default
            master_job_id: None,
            segment_cache: Vec::new(),
        }
    }

    /// Initialize adaptive streaming
    pub async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Start adaptive transcoding
        let response: TranscodeResponse = self.client
            .post(&format!("{}/transcode/{}/adaptive", self.server_url, self.media_id))
            .send()
            .await?
            .json()
            .await?;

        if response.status == "success" {
            self.master_job_id = response.master_job_id;
            
            // Wait a moment for initial segments to be ready
            tokio::time::sleep(Duration::from_secs(2)).await;
            
            Ok(())
        } else {
            Err("Failed to start transcoding".into())
        }
    }

    /// Get a specific segment
    pub async fn get_segment(&mut self, segment_number: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Check cache first
        if let Some((_, data)) = self.segment_cache.iter().find(|(num, _)| *num == segment_number) {
            return Ok(data.clone());
        }

        // Fetch from server
        let start = Instant::now();
        let url = format!("{}/transcode/{}/segment/{}", 
            self.server_url, 
            self.master_job_id.as_ref().unwrap_or(&self.media_id), 
            segment_number
        );

        let response = self.client.get(&url).send().await?;
        let segment_data = response.bytes().await?.to_vec();
        let download_time = start.elapsed();

        // Simple bandwidth calculation
        let bandwidth_mbps = (segment_data.len() as f64 * 8.0) / download_time.as_secs_f64() / 1_000_000.0;
        
        // Adjust quality based on bandwidth
        self.adapt_quality(bandwidth_mbps);

        // Cache the segment
        self.segment_cache.push((segment_number, segment_data.clone()));
        if self.segment_cache.len() > 10 {
            self.segment_cache.remove(0);
        }

        // Prefetch next segments
        self.prefetch_segments(segment_number).await;

        Ok(segment_data)
    }

    /// Get variant playlist URL
    pub fn get_playlist_url(&self) -> String {
        format!("{}/transcode/{}/variant/{}/playlist.m3u8", 
            self.server_url, self.media_id, self.current_variant)
    }

    /// Get master playlist URL
    pub fn get_master_playlist_url(&self) -> String {
        format!("{}/transcode/{}/master.m3u8", self.server_url, self.media_id)
    }

    /// Adapt quality based on bandwidth
    fn adapt_quality(&mut self, bandwidth_mbps: f64) {
        let new_variant = match bandwidth_mbps {
            b if b < 1.0 => "360p",
            b if b < 3.0 => "480p", 
            b if b < 6.0 => "720p",
            b if b < 12.0 => "1080p",
            _ => "4k",
        };

        if new_variant != self.current_variant {
            println!("Switching quality from {} to {} (bandwidth: {:.2} Mbps)", 
                self.current_variant, new_variant, bandwidth_mbps);
            self.current_variant = new_variant.to_string();
        }
    }

    /// Prefetch upcoming segments
    async fn prefetch_segments(&self, current_segment: u32) {
        let client = self.client.clone();
        let server_url = self.server_url.clone();
        let job_id = self.master_job_id.clone().unwrap_or(self.media_id.clone());

        // Prefetch next 2 segments in background
        for i in 1..=2 {
            let segment_num = current_segment + i;
            let url = format!("{}/transcode/{}/segment/{}", server_url, job_id, segment_num);
            let client = client.clone();
            
            tokio::spawn(async move {
                let _ = client.get(&url).send().await;
            });
        }
    }

    /// Check transcoding status
    pub async fn check_status(&self) -> Result<Option<f32>, Box<dyn std::error::Error>> {
        if let Some(job_id) = &self.master_job_id {
            let response: JobStatus = self.client
                .get(&format!("{}/transcode/status/{}", self.server_url, job_id))
                .send()
                .await?
                .json()
                .await?;

            if let Some(job) = response.job {
                return Ok(job.progress);
            }
        }
        Ok(None)
    }

    /// Cancel transcoding
    pub async fn cancel(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(job_id) = &self.master_job_id {
            self.client
                .post(&format!("{}/transcode/cancel/{}", self.server_url, job_id))
                .send()
                .await?;
        }
        Ok(())
    }
}

// Example usage in Iced player
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = FerrexStreamingSource::new(
        "http://localhost:8080".to_string(),
        "media-uuid-here".to_string()
    );

    // Initialize streaming
    source.initialize().await?;

    // Simulate player requesting segments
    for segment_num in 0..10 {
        println!("Requesting segment {}", segment_num);
        let segment_data = source.get_segment(segment_num).await?;
        println!("Received segment {} ({} bytes)", segment_num, segment_data.len());
        
        // Simulate playback time
        tokio::time::sleep(Duration::from_secs(4)).await;
    }

    Ok(())
}
//! Bounded server-side HLS transcoding and cache publication.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use ferrex_core::database::repository_ports::media_files::PlaybackMediaSource;
use ferrex_model::{
    TranscodeJobState, TranscodeJobStatusResponse, TranscodeQualityProfile,
};
use tokio::sync::{RwLock, Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

const MAX_CONCURRENT_TRANSCODES: usize = 2;
const JOB_RETENTION: Duration = Duration::from_secs(6 * 60 * 60);
const QUEUE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const TRANSCODE_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone)]
pub struct TranscodeManager {
    inner: Arc<TranscodeManagerInner>,
}

#[derive(Debug)]
struct TranscodeManagerInner {
    ffmpeg_path: String,
    cache_root: PathBuf,
    jobs: RwLock<HashMap<Uuid, JobRecord>>,
    permits: Semaphore,
}

#[derive(Debug, Clone)]
struct JobRecord {
    owner_id: Uuid,
    created_at: Instant,
    response: TranscodeJobStatusResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeStatusLookupError {
    NotFound,
    Forbidden,
}

impl TranscodeManager {
    pub fn new(ffmpeg_path: String, cache_root: PathBuf) -> Self {
        Self {
            inner: Arc::new(TranscodeManagerInner {
                ffmpeg_path,
                cache_root,
                jobs: RwLock::new(HashMap::new()),
                permits: Semaphore::new(MAX_CONCURRENT_TRANSCODES),
            }),
        }
    }

    pub async fn start(
        &self,
        owner_id: Uuid,
        source: PlaybackMediaSource,
        profile: TranscodeQualityProfile,
    ) -> TranscodeJobStatusResponse {
        self.prune_expired_jobs().await;

        let playback_path = playback_path(source.id, profile);
        if cache_is_current(&self.inner.cache_root, &source, profile).await {
            let response = TranscodeJobStatusResponse {
                job_id: Uuid::now_v7().to_string(),
                media_id: source.id.to_string(),
                profile,
                state: TranscodeJobState::Completed,
                progress: Some(1.0),
                message: Some("Cached rendition is ready".to_string()),
                playback_path: Some(playback_path),
            };
            self.insert(owner_id, response.clone()).await;
            return response;
        }

        {
            let jobs = self.inner.jobs.read().await;
            let media_id = source.id.to_string();
            if let Some(existing) = jobs.values().find(|record| {
                record.owner_id == owner_id
                    && record.response.media_id == media_id
                    && record.response.profile == profile
                    && matches!(
                        record.response.state,
                        TranscodeJobState::Queued | TranscodeJobState::Running
                    )
            }) {
                return existing.response.clone();
            }
        }

        let job_id = Uuid::now_v7();
        let response = TranscodeJobStatusResponse {
            job_id: job_id.to_string(),
            media_id: source.id.to_string(),
            profile,
            state: TranscodeJobState::Queued,
            progress: Some(0.0),
            message: Some("Waiting for a transcoder worker".to_string()),
            playback_path: None,
        };
        self.insert(owner_id, response.clone()).await;

        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            run_job(inner, job_id, source, profile, playback_path).await;
        });

        response
    }

    pub async fn status(
        &self,
        owner_id: Uuid,
        job_id: Uuid,
    ) -> Result<TranscodeJobStatusResponse, TranscodeStatusLookupError> {
        let jobs = self.inner.jobs.read().await;
        let record = jobs
            .get(&job_id)
            .ok_or(TranscodeStatusLookupError::NotFound)?;
        if record.owner_id != owner_id {
            return Err(TranscodeStatusLookupError::Forbidden);
        }
        Ok(record.response.clone())
    }

    async fn insert(
        &self,
        owner_id: Uuid,
        response: TranscodeJobStatusResponse,
    ) {
        let Ok(job_id) = Uuid::parse_str(&response.job_id) else {
            return;
        };
        self.inner.jobs.write().await.insert(
            job_id,
            JobRecord {
                owner_id,
                created_at: Instant::now(),
                response,
            },
        );
    }

    async fn prune_expired_jobs(&self) {
        self.inner
            .jobs
            .write()
            .await
            .retain(|_, job| job.created_at.elapsed() < JOB_RETENTION);
    }
}

async fn run_job(
    inner: Arc<TranscodeManagerInner>,
    job_id: Uuid,
    source: PlaybackMediaSource,
    profile: TranscodeQualityProfile,
    playback_path: String,
) {
    let _permit = match tokio::time::timeout(
        QUEUE_TIMEOUT,
        inner.permits.acquire(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            update_failed(&inner, job_id, "Transcoder is shutting down").await;
            return;
        }
        Err(_) => {
            update_failed(&inner, job_id, "Transcoder queue timed out").await;
            return;
        }
    };

    update_job(&inner, job_id, |response| {
        response.state = TranscodeJobState::Running;
        response.progress = None;
        response.message = Some("Generating HLS rendition".to_string());
    })
    .await;

    let staging_root = inner.cache_root.join(".jobs").join(job_id.to_string());
    let final_root = rendition_root(&inner.cache_root, source.id, profile);

    if let Err(err) = tokio::fs::create_dir_all(&staging_root).await {
        error!(?err, %job_id, "could not create transcode staging directory");
        update_failed(&inner, job_id, "Could not prepare transcode output")
            .await;
        return;
    }

    let playlist = staging_root.join("index.m3u8");
    let segment_pattern = staging_root.join("segment-%05d.ts");
    let args = ffmpeg_args(&source.path, profile, &segment_pattern, &playlist);
    info!(%job_id, media_id = %source.id, profile = %profile, "starting HLS transcode");

    let mut command = tokio::process::Command::new(&inner.ffmpeg_path);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let succeeded = match tokio::time::timeout(
        TRANSCODE_TIMEOUT,
        command.status(),
    )
    .await
    {
        Ok(Ok(status)) if status.success() => true,
        Ok(Ok(status)) => {
            warn!(%job_id, status = ?status.code(), "FFmpeg transcode failed");
            false
        }
        Ok(Err(err)) => {
            error!(?err, %job_id, "could not launch FFmpeg transcoder");
            false
        }
        Err(_) => {
            warn!(%job_id, timeout_seconds = TRANSCODE_TIMEOUT.as_secs(), "FFmpeg transcode timed out and was terminated");
            false
        }
    };

    if !succeeded || !valid_hls_output(&staging_root).await {
        let _ = tokio::fs::remove_dir_all(&staging_root).await;
        update_failed(
            &inner,
            job_id,
            "FFmpeg could not generate this rendition",
        )
        .await;
        return;
    }

    if let Some(parent) = final_root.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        error!(?err, %job_id, "could not create transcode cache directory");
        let _ = tokio::fs::remove_dir_all(&staging_root).await;
        update_failed(&inner, job_id, "Could not publish transcode output")
            .await;
        return;
    }
    if tokio::fs::try_exists(&final_root).await.unwrap_or(false)
        && let Err(err) = tokio::fs::remove_dir_all(&final_root).await
    {
        error!(?err, %job_id, "could not replace stale transcode cache");
        let _ = tokio::fs::remove_dir_all(&staging_root).await;
        update_failed(&inner, job_id, "Could not publish transcode output")
            .await;
        return;
    }
    if let Err(err) = tokio::fs::rename(&staging_root, &final_root).await {
        error!(?err, %job_id, "could not atomically publish transcode output");
        let _ = tokio::fs::remove_dir_all(&staging_root).await;
        update_failed(&inner, job_id, "Could not publish transcode output")
            .await;
        return;
    }

    update_job(&inner, job_id, |response| {
        response.state = TranscodeJobState::Completed;
        response.progress = Some(1.0);
        response.message = Some("HLS rendition is ready".to_string());
        response.playback_path = Some(playback_path);
    })
    .await;
    info!(%job_id, media_id = %source.id, profile = %profile, "HLS transcode completed");
}

async fn update_failed(
    inner: &TranscodeManagerInner,
    job_id: Uuid,
    message: &'static str,
) {
    update_job(inner, job_id, |response| {
        response.state = TranscodeJobState::Failed;
        response.progress = None;
        response.message = Some(message.to_string());
        response.playback_path = None;
    })
    .await;
}

async fn update_job(
    inner: &TranscodeManagerInner,
    job_id: Uuid,
    update: impl FnOnce(&mut TranscodeJobStatusResponse),
) {
    if let Some(job) = inner.jobs.write().await.get_mut(&job_id) {
        update(&mut job.response);
    }
}

fn playback_path(media_id: Uuid, profile: TranscodeQualityProfile) -> String {
    format!(
        "/api/v1/transcode/{}/{}/index.m3u8",
        media_id,
        profile.as_str()
    )
}

pub fn rendition_root(
    cache_root: &Path,
    media_id: Uuid,
    profile: TranscodeQualityProfile,
) -> PathBuf {
    cache_root.join(media_id.to_string()).join(profile.as_str())
}

async fn cache_is_current(
    cache_root: &Path,
    source: &PlaybackMediaSource,
    profile: TranscodeQualityProfile,
) -> bool {
    let root = rendition_root(cache_root, source.id, profile);
    if !valid_hls_output(&root).await {
        return false;
    }

    let Ok(source_modified) = tokio::fs::metadata(&source.path)
        .await
        .and_then(|metadata| metadata.modified())
    else {
        return false;
    };
    tokio::fs::metadata(root.join("index.m3u8"))
        .await
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|manifest_modified| manifest_modified >= source_modified)
}

async fn valid_hls_output(root: &Path) -> bool {
    let Ok(manifest) = tokio::fs::read_to_string(root.join("index.m3u8")).await
    else {
        return false;
    };
    if !manifest.starts_with("#EXTM3U")
        || !manifest.lines().any(|line| line.ends_with(".ts"))
    {
        return false;
    }

    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return false;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("ts")
            && entry
                .metadata()
                .await
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        {
            return true;
        }
    }
    false
}

fn ffmpeg_args(
    source: &Path,
    profile: TranscodeQualityProfile,
    segment_pattern: &Path,
    playlist: &Path,
) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-nostdin".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-i".to_string(),
        source.to_string_lossy().into_owned(),
        "-map".to_string(),
        "0:v:0".to_string(),
        "-map".to_string(),
        "0:a:0?".to_string(),
    ];

    let (dimensions, bitrate, maxrate, buffer) = match profile {
        TranscodeQualityProfile::Original => (None, "12M", "14M", "24M"),
        TranscodeQualityProfile::P1080 => {
            (Some((1920, 1080)), "8M", "9M", "16M")
        }
        TranscodeQualityProfile::P720 => (Some((1280, 720)), "4M", "5M", "8M"),
        TranscodeQualityProfile::P480 => {
            (Some((854, 480)), "2M", "2400k", "4M")
        }
        TranscodeQualityProfile::P360 => {
            (Some((640, 360)), "800k", "960k", "1600k")
        }
    };
    if let Some((width, height)) = dimensions {
        args.extend([
            "-vf".to_string(),
            format!(
                "scale=w={width}:h={height}:force_original_aspect_ratio=decrease:force_divisible_by=2"
            ),
        ]);
    }

    args.extend([
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "veryfast".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-b:v".to_string(),
        bitrate.to_string(),
        "-maxrate".to_string(),
        maxrate.to_string(),
        "-bufsize".to_string(),
        buffer.to_string(),
        "-force_key_frames".to_string(),
        "expr:gte(t,n_forced*4)".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "160k".to_string(),
        "-ac".to_string(),
        "2".to_string(),
        "-max_muxing_queue_size".to_string(),
        "2048".to_string(),
        "-f".to_string(),
        "hls".to_string(),
        "-hls_time".to_string(),
        "4".to_string(),
        "-hls_playlist_type".to_string(),
        "vod".to_string(),
        "-hls_flags".to_string(),
        "independent_segments".to_string(),
        "-hls_segment_filename".to_string(),
        segment_pattern.to_string_lossy().into_owned(),
        playlist.to_string_lossy().into_owned(),
    ]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn profiles_map_to_argument_separated_ffmpeg_commands() {
        for profile in TranscodeQualityProfile::ALL {
            let args = ffmpeg_args(
                Path::new("/media/a file;not-a-command.mkv"),
                profile,
                Path::new("/cache/segment-%05d.ts"),
                Path::new("/cache/index.m3u8"),
            );
            assert_eq!(
                args.iter()
                    .position(|arg| arg == "-i")
                    .and_then(|index| args.get(index + 1))
                    .map(String::as_str),
                Some("/media/a file;not-a-command.mkv")
            );
            assert!(args.iter().any(|arg| arg == "hls"));
            assert!(args.iter().any(|arg| arg.ends_with("segment-%05d.ts")));
        }
    }

    #[test]
    fn rendition_paths_are_closed_over_the_typed_profile_set() {
        let root = Path::new("/cache/transcode");
        let media_id = Uuid::nil();
        for profile in TranscodeQualityProfile::ALL {
            let path = rendition_root(root, media_id, profile);
            assert!(path.starts_with(root));
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(profile.as_str())
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires an FFmpeg binary with libx264 and AAC encoders"]
    async fn real_ffmpeg_job_generates_and_reuses_a_complete_hls_rendition() {
        let tempdir = tempfile::tempdir().expect("temporary transcode root");
        let source_path = tempdir.path().join("source.mkv");
        let generated = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=640x360:d=1:r=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                source_path.to_str().expect("UTF-8 test path"),
            ])
            .status()
            .expect("launch fixture FFmpeg");
        assert!(generated.success(), "fixture generation failed");

        let media_id = Uuid::new_v4();
        let source = PlaybackMediaSource {
            id: media_id,
            path: source_path,
            filename: "source.mkv".to_string(),
            size: 1,
            is_available: true,
        };
        let manager = TranscodeManager::new(
            "ffmpeg".to_string(),
            tempdir.path().join("cache"),
        );
        let owner = Uuid::new_v4();
        let started = manager
            .start(owner, source.clone(), TranscodeQualityProfile::P360)
            .await;
        let job_id = Uuid::parse_str(&started.job_id).expect("job UUID");

        let completed = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let status =
                    manager.status(owner, job_id).await.expect("job status");
                match status.state {
                    TranscodeJobState::Completed => break status,
                    TranscodeJobState::Failed => {
                        panic!("real transcode failed: {:?}", status.message)
                    }
                    TranscodeJobState::Queued | TranscodeJobState::Running => {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
            }
        })
        .await
        .expect("real transcode timed out");
        assert_eq!(completed.progress, Some(1.0));
        assert!(completed.playback_path.is_some());
        assert!(
            valid_hls_output(&rendition_root(
                &manager.inner.cache_root,
                media_id,
                TranscodeQualityProfile::P360,
            ))
            .await
        );

        let cached = manager
            .start(owner, source, TranscodeQualityProfile::P360)
            .await;
        assert_eq!(cached.state, TranscodeJobState::Completed);
        assert_eq!(cached.progress, Some(1.0));
        assert_eq!(cached.playback_path, completed.playback_path);
    }
}

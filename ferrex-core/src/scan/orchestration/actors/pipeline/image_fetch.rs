use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::database::traits::ImageLookupParams;
use crate::error::{MediaError, Result};
use crate::infrastructure::media::image_service::{
    ImageService, TmdbImageSize,
};
use crate::scan::orchestration::job::{ImageFetchJob, ImageFetchSource};

use super::{ImageFetchActor, ImageFetchCommand};

#[derive(Debug)]
pub struct DefaultImageFetchActor {
    image_service: Arc<ImageService>,
}

impl DefaultImageFetchActor {
    pub fn new(image_service: Arc<ImageService>) -> Self {
        Self { image_service }
    }
}

#[async_trait]
impl ImageFetchActor for DefaultImageFetchActor {
    async fn fetch(&self, command: ImageFetchCommand) -> Result<()> {
        let ImageFetchJob {
            library_id: _,
            source,
            key,
            priority_hint: _,
        } = command.job;

        match source {
            ImageFetchSource::Tmdb { tmdb_path } => {
                let _size =
                    TmdbImageSize::from_str(&key.variant).ok_or_else(|| {
                        MediaError::InvalidMedia(format!(
                            "Unsupported TMDB variant '{}' for {}",
                            key.variant, tmdb_path
                        ))
                    })?;

                info!(
                    media_type = %key.media_type,
                    media_id = %key.media_id,
                    image_type = %key.image_type,
                    variant = %key.variant,
                    tmdb_path = %tmdb_path,
                    "Fetching TMDB image variant"
                );

                // Route through singleflight + DB-aware path so scanner and
                // HTTP handler coordinate on the same variant work.
                let params = ImageLookupParams {
                    media_type: key.media_type.clone(),
                    media_id: key.media_id.to_string(),
                    image_type: key.image_type.clone(),
                    index: key.order_index as u32,
                    variant: Some(key.variant.clone()),
                };

                match self
                    .image_service
                    .get_or_download_variant(&params)
                    .await?
                {
                    Some(_) => Ok(()),
                    None => Err(MediaError::Internal(
                        "get_or_download_variant returned None during scan"
                            .into(),
                    )),
                }
            }
            ImageFetchSource::EpisodeThumbnail {
                media_file_id,
                image_key,
            } => {
                info!(
                    media_type = %key.media_type,
                    media_id = %key.media_id,
                    image_type = %key.image_type,
                    variant = %key.variant,
                    media_file_id = %media_file_id,
                    "Generating local episode thumbnail variant"
                );

                self.image_service
                    .generate_episode_thumbnail(&image_key, media_file_id, key)
                    .await
                    .map(|_| ())
            }
        }
    }
}

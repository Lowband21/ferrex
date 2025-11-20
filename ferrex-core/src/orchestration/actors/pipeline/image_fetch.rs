use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::image_service::{ImageService, TmdbImageSize};
use crate::orchestration::job::ImageFetchJob;
use crate::{MediaError, Result};

use super::{ImageFetchActor, ImageFetchCommand};

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
            tmdb_path,
            key,
            priority_hint: _,
        } = command.job;

        let size = TmdbImageSize::from_str(&key.variant).ok_or_else(|| {
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

        self.image_service
            .download_variant(&tmdb_path, size, Some(key))
            .await
            .map(|_| ())
    }
}

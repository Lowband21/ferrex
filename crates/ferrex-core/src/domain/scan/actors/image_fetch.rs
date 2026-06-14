use crate::{
    error::Result,
    infra::{image_service::CachePolicy, media::image_service::ImageService},
};

use crate::domain::scan::ImageFetchJob;
use async_trait::async_trait;
use std::sync::Arc;

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
    async fn fetch(&self, job: &ImageFetchJob) -> Result<()> {
        let iid = job.iid;
        let imz = job.imz;
        // Use the unified cache path for all image variants, including thumbnails.
        self.image_service
            .cached_image(iid, imz, CachePolicy::Ensure)
            .await?;

        Ok(())
    }
}

#[async_trait]
pub trait ImageFetchActor: Send + Sync {
    async fn fetch(&self, job: &ImageFetchJob) -> Result<()>;
}

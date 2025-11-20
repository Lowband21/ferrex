//! Metadata fetching domain
//!
//! Contains all metadata-related state and logic moved from the monolithic State

pub mod batch_fetcher;
pub mod batch_fetch_helper;
pub mod image_pipeline;
pub mod image_service;
pub mod image_types;
pub mod messages;
pub mod service;
pub mod update;
pub mod update_handlers;

use self::batch_fetcher::BatchMetadataFetcher;
use self::image_service::UnifiedImageService;
use self::image_types::ImageRequest;
use self::messages::Message as MetadataMessage;
use self::service::MetadataFetchService;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::domains::media::store::MediaStore;
use crate::infrastructure::{
    adapters::api_client_adapter::ApiClientAdapter, api_types::MediaReference,
};
use ferrex_core::permissions::SERVER_MANAGE_TASKS;
use ferrex_core::MediaId;
use iced::Task;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock as StdRwLock};
use std::time::Instant;
use uuid::Uuid;

/// Metadata domain state - moved from monolithic State
#[derive(Debug)]
pub struct MetadataDomainState {
    // From State struct:
    pub server_url: String,
    pub metadata_service: Option<Arc<MetadataFetchService>>,
    pub loading_posters: HashSet<String>,
    pub tmdb_poster_urls: HashMap<String, String>,
    pub metadata_fetch_attempts: HashMap<String, Instant>,
    pub image_service: UnifiedImageService,
    pub image_receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,

    // Shared references needed by metadata domain
    pub media_store: Arc<StdRwLock<MediaStore>>,
    pub api_service: Option<Arc<ApiClientAdapter>>,
}

impl MetadataDomainState {
    pub fn new(
        server_url: String,
        media_store: Arc<StdRwLock<MediaStore>>,
        api_service: Option<Arc<ApiClientAdapter>>,
        image_service: UnifiedImageService,
    ) -> Self {
        Self {
            server_url,
            metadata_service: None,
            loading_posters: HashSet::new(),
            tmdb_poster_urls: HashMap::new(),
            metadata_fetch_attempts: HashMap::new(),
            image_service,
            image_receiver: Arc::new(Mutex::new(None)),
            media_store,
            api_service,
        }
    }
    pub fn fetch_media_details_on_demand(
        &self,
        library_id: Uuid,
        media_id: MediaId,
    ) -> Task<DomainMessage> {
        #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
        profiling::scope!(crate::infrastructure::profiling_scopes::scopes::METADATA_FETCH);
        
        // Check if the media already has details in MediaStore
        if let Ok(store) = self.media_store.read() {
            if let Some(media_ref) = store.get(&media_id) {
                // Check if we need to fetch details using the api_types helper
                let needs_fetch = match media_ref {
                    MediaReference::Movie(m) => {
                        crate::infrastructure::api_types::needs_details_fetch(&m.details)
                    }
                    MediaReference::Series(s) => {
                        crate::infrastructure::api_types::needs_details_fetch(&s.details)
                    }
                    MediaReference::Season(s) => {
                        crate::infrastructure::api_types::needs_details_fetch(&s.details)
                    }
                    MediaReference::Episode(e) => {
                        crate::infrastructure::api_types::needs_details_fetch(&e.details)
                    }
                };
                if !needs_fetch {
                    // Details already exist, no need to fetch
                    return Task::none();
                }
            }
        }

        // Details needed, fetch from server
        let server_url = self.server_url.clone();
        Task::perform(
            crate::domains::media::library::fetch_media_details(
                server_url,
                library_id,
                media_id.clone(),
            ),
            move |result| match result {
                Ok(media_ref) => {
                    DomainMessage::Metadata(MetadataMessage::MediaDetailsUpdated(media_ref))
                }
                Err(e) => {
                    log::error!("Failed to fetch media details for {:?}: {}", media_id, e);
                    DomainMessage::Metadata(MetadataMessage::NoOp)
                }
            },
        )
    }
}

#[derive(Debug)]
pub struct MetadataDomain {
    pub state: MetadataDomainState,
}

impl MetadataDomain {
    pub fn new(state: MetadataDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_metadata logic
    pub fn update(&mut self, message: MetadataMessage) -> Task<DomainMessage> {
        // This will call the existing update_metadata function
        // For now, we return Task::none() to make it compile
        Task::none()
    }

    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::MediaLoaded => {
                // Could trigger metadata fetching for new media
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

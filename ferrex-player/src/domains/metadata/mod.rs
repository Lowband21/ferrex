//! Metadata fetching domain
//!
//! Contains all metadata-related state and logic moved from the monolithic State

//pub mod image_pipeline;
pub mod image_service;
pub mod messages;
pub mod update;
pub mod update_handlers;

use self::image_service::UnifiedImageService;
use self::messages::Message as MetadataMessage;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infrastructure::services::api::ApiService;
use iced::Task;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Metadata domain state - moved from monolithic State
#[derive(Debug)]
pub struct MetadataDomainState {
    // From State struct:
    pub server_url: String,
    //pub metadata_service: Option<Arc<MetadataFetchService>>,
    pub loading_posters: HashSet<String>,
    pub tmdb_poster_urls: HashMap<String, String>,
    pub metadata_fetch_attempts: HashMap<String, Instant>,
    pub image_service: UnifiedImageService,
    pub image_receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,

    // Shared references needed by metadata domain
    //pub media_store: Arc<StdRwLock<MediaStore>>,
    //pub repo_accessor: Arc<StdRwLock<MetadataRepoAccessor>>,
    pub api_service: Option<Arc<dyn ApiService>>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl MetadataDomainState {
    pub fn new(
        server_url: String,
        //media_store: Arc<StdRwLock<MediaStore>>,
        api_service: Option<Arc<dyn ApiService>>,
        image_service: UnifiedImageService,
    ) -> Self {
        Self {
            server_url,
            //metadata_service: None,
            loading_posters: HashSet::new(),
            tmdb_poster_urls: HashMap::new(),
            metadata_fetch_attempts: HashMap::new(),
            image_service,
            image_receiver: Arc::new(Mutex::new(None)),
            //media_store,
            api_service,
        }
    }
}

#[derive(Debug)]
pub struct MetadataDomain {
    pub state: MetadataDomainState,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
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

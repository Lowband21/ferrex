//! Metadata and image presentation state for the desktop player.
//!
//! This module keeps UI-bound image loading, Iced image handles, demand planning,
//! and metadata update plumbing in `ferrex-player-ui` so lower player domain
//! crates remain free of Iced dependencies.

pub mod demand_planner;
pub mod image_service;
pub mod messages;
pub mod update;
pub mod update_handlers;

use self::{demand_planner::PlannerHandle, image_service::UnifiedImageService};
use ferrex_player_api::services::api::ApiService;
use iced::Task;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Instant,
};

/// Cross-domain events relevant to metadata/image state.
pub trait MetadataExternalEvent {
    fn is_media_loaded(&self) -> bool {
        false
    }
}

/// Metadata/image state owned by the UI presentation crate.
#[derive(Debug)]
pub struct MetadataDomainState {
    pub server_url: String,
    pub loading_posters: HashSet<String>,
    pub tmdb_poster_urls: HashMap<String, String>,
    pub metadata_fetch_attempts: HashMap<String, Instant>,
    pub image_service: Arc<UnifiedImageService>,
    pub image_receiver:
        Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    pub planner_handle: Option<PlannerHandle>,
    pub planner_join: Option<tokio::task::JoinHandle<()>>,
    pub api_service: Option<Arc<dyn ApiService>>,
}

impl MetadataDomainState {
    pub fn new(
        server_url: String,
        api_service: Option<Arc<dyn ApiService>>,
        image_service: Arc<UnifiedImageService>,
    ) -> Self {
        let (planner_handle, planner_join) =
            self::demand_planner::start_planner(image_service.clone());
        Self {
            server_url,
            loading_posters: HashSet::new(),
            tmdb_poster_urls: HashMap::new(),
            metadata_fetch_attempts: HashMap::new(),
            image_service,
            image_receiver: Arc::new(Mutex::new(None)),
            planner_handle: Some(planner_handle),
            planner_join: Some(planner_join),
            api_service,
        }
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

    pub fn handle_event<E>(
        &mut self,
        _event: &E,
    ) -> Task<messages::MetadataMessage>
    where
        E: MetadataExternalEvent,
    {
        Task::none()
    }
}

impl MetadataExternalEvent for crate::common::messages::CrossDomainEvent {
    fn is_media_loaded(&self) -> bool {
        matches!(self, Self::MediaLoaded)
    }
}

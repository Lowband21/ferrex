use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use ferrex_core::player_prelude::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedMedia, ArchivedModel,
    EpisodeReference, Library, LibraryId, Media, MediaIDLike, MediaLike,
    MovieBatchId, MovieID, MovieReference, MovieReferenceBatchResponse,
    SeasonID, SeasonReference, SeriesID, SortBy, SortOrder,
};
use ferrex_model::VideoMediaType;
use parking_lot::Mutex;
use rkyv::{rancor::Error, to_bytes, util::AlignedVec, vec::ArchivedVec};
use uuid::Uuid;
use yoke::Yoke;

use crate::infra::repository::{RepositoryError, RepositoryResult};

use super::{
    EpisodeYoke, LibraryYoke, MediaYoke, MovieYoke, SeasonYoke, SeriesYoke,
    movie_batches::{
        MovieBatchInstallOutcome, MovieBatchKey, MovieBatchOverlay,
    },
    series_bundles::{
        SeriesBundleInstallOutcome, SeriesBundleKey, SeriesBundleOverlay,
    },
};

/// Archived runtime media entry backed by an `AlignedVec` to enable zero-copy access.
#[derive(Debug, Clone)]
pub(super) struct RuntimeMediaEntry {
    buffer: Arc<AlignedVec>,
}

impl RuntimeMediaEntry {
    fn new(buffer: AlignedVec) -> Self {
        Self {
            buffer: Arc::new(buffer),
        }
    }

    pub(super) fn from_media(media: &Media) -> Result<Self, RepositoryError> {
        let bytes = to_bytes::<Error>(media)
            .map_err(|e| RepositoryError::SerializationError(e.to_string()))?;
        Ok(Self::new(bytes))
    }

    #[inline]
    fn cart(&self) -> Arc<AlignedVec> {
        Arc::clone(&self.buffer)
    }

    #[inline]
    fn with_archived<T>(&self, f: impl FnOnce(&ArchivedMedia) -> T) -> T {
        // SAFETY: The aligned buffer is owned by an Arc, so it lives for the duration of the closure
        let archived =
            unsafe { rkyv::access_unchecked::<ArchivedMedia>(&self.buffer) };
        f(archived)
    }

    pub(super) fn deserialize(&self) -> RepositoryResult<Media> {
        self.with_archived(|arch| arch.try_to_model())
            .map_err(|e| RepositoryError::DeserializationError(e.to_string()))
    }
}

/// Runtime modifications layer for managing changes during application runtime
/// Resets on application restart
#[derive(Default, Debug)]
pub(super) struct RuntimeModifications {
    /// Added media items during runtime (uuid -> archived reference)
    pub(super) added: HashMap<Uuid, RuntimeMediaEntry>,
    /// Added items, mapped by owning library UUID (library_uuid -> set of media uuids)
    pub(super) added_by_library: HashMap<Uuid, HashSet<Uuid>>,
    /// Deleted media IDs during runtime
    pub(super) deleted: HashSet<Uuid>,
    /// Modified media items during runtime (for archived items)
    pub(super) modified: HashMap<Uuid, RuntimeMediaEntry>,
    /// IDs that only exist in the runtime overlay (not in the archived snapshot)
    runtime_only_ids: HashSet<Uuid>,
}

impl RuntimeModifications {
    pub(super) fn clear(&mut self) -> HashSet<Uuid> {
        let runtime_ids = std::mem::take(&mut self.runtime_only_ids);
        self.added.clear();
        self.added_by_library.clear();
        self.deleted.clear();
        self.modified.clear();
        runtime_ids
    }

    pub(super) fn is_deleted(&self, id: &Uuid) -> bool {
        self.deleted.contains(id)
    }

    pub(super) fn get_entry(&self, id: &Uuid) -> Option<&RuntimeMediaEntry> {
        self.modified.get(id).or_else(|| self.added.get(id))
    }

    pub(super) fn mark_runtime_only(&mut self, id: Uuid) {
        self.runtime_only_ids.insert(id);
    }

    pub(super) fn unmark_runtime_only(&mut self, id: &Uuid) {
        self.runtime_only_ids.remove(id);
    }

    pub(super) fn is_runtime_only(&self, id: &Uuid) -> bool {
        self.runtime_only_ids.contains(id)
    }
}

/// Single source of truth for all media data
#[derive(Debug)]
pub struct MediaRepo {
    /// Raw data buffer for libraries that must not be dropped
    libraries_buffer: Arc<AlignedVec>,

    /// Currently stored libraries index
    pub(super) libraries_index: Vec<Uuid>, // Vec of library IDs

    /// ID index for O(1) lookups: UUID -> library_id
    pub(super) media_id_index: HashMap<Uuid, Uuid>, // key: media_id, value: library_id

    /// Runtime modifications layer (cleared on restart)
    pub(super) modifications: RuntimeModifications,

    /// Zero-copy movie batches layered alongside the archived libraries buffer.
    pub(super) movie_batches: MovieBatchOverlay,

    /// Zero-copy per-series bundles layered alongside the archived libraries buffer.
    pub(super) series_bundles: SeriesBundleOverlay,

    /// Cache of `Media` values materialized from batch/bundle sources into standalone rkyv blobs.
    ///
    /// The canonical storage (movie batches / series bundles) stores concrete item types
    /// (`MovieReference`, `Series`, `SeasonReference`, `EpisodeReference`) rather than the `Media`
    /// enum. Callers that rely on `get_media_yoke` expect an `ArchivedMedia` reference, so we
    /// materialize on-demand and keep a small bounded cache.
    materialized_media: Mutex<HashMap<Uuid, RuntimeMediaEntry>>,
    materialized_media_lru: Mutex<VecDeque<Uuid>>,
    materialized_media_cap: usize,

    pub(super) library_episode_lens: Arc<Mutex<HashMap<Uuid, usize>>>,
    pub(super) library_episode_lens_dirty: Arc<Mutex<HashMap<Uuid, bool>>>,

    // Cached sorted indices, should be fetched from server
    //
    // NOTE: This is intentionally retained while server-side sorting is
    // temporarily offline in the player. Reintegrating server sorting should
    // wire these fields back into the library view pipeline.
    #[allow(dead_code)]
    pub(super) sorted_indices: Option<HashMap<Uuid, Vec<Uuid>>>, // Hashmap of library IDs to Vec of media IDs

    // Current sort criteria
    #[allow(dead_code)]
    pub(super) current_library_sort_states:
        Option<HashMap<Uuid, (SortBy, SortOrder)>>, // Hashmap of library IDs to sort criteria

                                                    //pending_events: Vec<MediaChangeEvent>,
}

impl MediaRepo {
    pub fn new_empty() -> Self {
        Self {
            libraries_buffer: Arc::new(AlignedVec::new()),
            libraries_index: Vec::new(),
            media_id_index: HashMap::new(),
            modifications: RuntimeModifications::default(),
            movie_batches: MovieBatchOverlay::default(),
            series_bundles: SeriesBundleOverlay::default(),
            materialized_media: Mutex::new(HashMap::new()),
            materialized_media_lru: Mutex::new(VecDeque::new()),
            materialized_media_cap: 2048,
            library_episode_lens: Mutex::new(HashMap::new()).into(),
            library_episode_lens_dirty: Mutex::new(HashMap::new()).into(),
            sorted_indices: None,
            current_library_sort_states: None,
        }
    }

    pub fn new(bytes: AlignedVec) -> Result<Self, Error> {
        let buffer = Arc::new(bytes);
        let mut media_id_index = HashMap::new();
        let mut libraries_index = Vec::new();

        // Access the archived data directly without unsafe transmute
        let archived_libraries =
            rkyv::access::<ArchivedVec<ArchivedLibrary>, Error>(&buffer)?;

        // Build indices
        for library in archived_libraries.iter() {
            let library_id = library.get_id().as_uuid();
            libraries_index.push(library_id);

            if let Some(media_list) = library.media() {
                for media in media_list.iter() {
                    let media_id = media.archived_media_id().to_uuid();
                    media_id_index.insert(media_id, library_id);
                }
            }
        }

        Ok(Self {
            libraries_buffer: buffer,
            libraries_index,
            media_id_index,
            modifications: RuntimeModifications::default(),
            movie_batches: MovieBatchOverlay::default(),
            series_bundles: SeriesBundleOverlay::default(),
            materialized_media: Mutex::new(HashMap::new()),
            materialized_media_lru: Mutex::new(VecDeque::new()),
            materialized_media_cap: 2048,
            sorted_indices: None,
            current_library_sort_states: None,
            library_episode_lens: Mutex::new(HashMap::new()).into(),
            library_episode_lens_dirty: Mutex::new(HashMap::new()).into(),
        })
    }

    /// Locate media by library + uuid and optional media type, returning
    /// (library_index, media_index) within the archived snapshot. Returns None
    /// if not found or if the library has no media slice.
    fn find_media_position(
        &self,
        library_id: Uuid,
        media_uuid: Uuid,
        media_type: Option<VideoMediaType>,
    ) -> Option<(usize, usize)> {
        let Ok(archived_libraries) = rkyv::access::<
            ArchivedVec<ArchivedLibrary>,
            Error,
        >(&self.libraries_buffer) else {
            return None;
        };

        for (lib_idx, library) in archived_libraries.iter().enumerate() {
            if library.get_id().as_uuid() != library_id {
                continue;
            }
            let media_list = library.media()?;
            for (pos, media_ref) in media_list.iter().enumerate() {
                if media_ref.archived_media_id().to_uuid() == media_uuid
                    && media_type.is_none_or(|t| media_ref.media_type() == t)
                {
                    return Some((lib_idx, pos));
                }
            }
            return None;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.libraries_index.len()
    }

    pub fn episode_len(&self, lib_id: &LibraryId) -> usize {
        let lib_uuid = lib_id.to_uuid();
        if let Some(len) = self.library_episode_lens.lock().get(&lib_uuid)
            && !self
                .library_episode_lens_dirty
                .lock()
                .get(&lib_uuid)
                .copied()
                .unwrap_or(false)
        {
            *len
        } else {
            let len = self.series_bundles.episodes_len(lib_id);
            self.library_episode_lens.lock().insert(lib_uuid, len);
            self.library_episode_lens_dirty
                .lock()
                .insert(lib_uuid, false);
            len
        }
    }

    pub fn is_empty(&self) -> bool {
        self.libraries_index.is_empty()
    }

    pub fn clear(&mut self) {
        self.libraries_index.clear();
        self.media_id_index.clear();
        let _ = self.modifications.clear();
        self.movie_batches.clear();
        self.series_bundles.clear();
        self.materialized_media.lock().clear();
        self.materialized_media_lru.lock().clear();
        self.library_episode_lens.lock().clear();
        self.library_episode_lens_dirty.lock().clear();
    }

    fn materialized_media_get(&self, id: &Uuid) -> Option<RuntimeMediaEntry> {
        let mut lru = self.materialized_media_lru.lock();
        let entry = self.materialized_media.lock().get(id).cloned();
        if entry.is_some() {
            if let Some(pos) = lru.iter().position(|x| x == id) {
                lru.remove(pos);
            }
            lru.push_front(*id);
        }
        entry
    }

    fn materialized_media_insert(&self, id: Uuid, entry: RuntimeMediaEntry) {
        let mut map = self.materialized_media.lock();
        let mut lru = self.materialized_media_lru.lock();

        if let std::collections::hash_map::Entry::Occupied(mut e) =
            map.entry(id)
        {
            e.insert(entry);
            if let Some(pos) = lru.iter().position(|x| x == &id) {
                lru.remove(pos);
            }
            lru.push_front(id);
            return;
        }

        if map.len() >= self.materialized_media_cap
            && let Some(evicted) = lru.pop_back()
        {
            map.remove(&evicted);
        }

        map.insert(id, entry);
        lru.push_front(id);
    }

    fn materialized_media_remove(&self, id: &Uuid) {
        self.materialized_media.lock().remove(id);
        let mut lru = self.materialized_media_lru.lock();
        if let Some(pos) = lru.iter().position(|x| x == id) {
            lru.remove(pos);
        }
    }

    pub(super) fn is_movie_backed_by_batch_internal(
        &self,
        movie_id: &MovieID,
    ) -> bool {
        self.movie_batches.is_movie_indexed(movie_id)
    }

    pub(super) fn is_series_backed_by_bundle_internal(
        &self,
        series_id: &ferrex_core::player_prelude::SeriesID,
    ) -> bool {
        self.series_bundles
            .get_series_cart(series_id.as_uuid())
            .is_some()
    }

    pub(super) fn install_movie_reference_batch_internal(
        &mut self,
        library_id: LibraryId,
        batch_id: MovieBatchId,
        bytes: AlignedVec,
    ) -> RepositoryResult<MovieBatchInstallOutcome> {
        let key = MovieBatchKey::new(library_id, batch_id);
        let ids = self.movie_batches.upsert_batch(key, bytes)?;

        let library_uuid = library_id.to_uuid();

        let mut outcome = MovieBatchInstallOutcome {
            movies_indexed: ids.len(),
            ..Default::default()
        };

        for movie_uuid in ids.iter().copied() {
            self.media_id_index.insert(movie_uuid, library_uuid);
            self.materialized_media_remove(&movie_uuid);

            if self.modifications.is_runtime_only(&movie_uuid) {
                let removed = self.modifications.added.remove(&movie_uuid);
                if removed.is_some() {
                    outcome.movies_replaced_from_runtime_overlay += 1;
                }

                if let Some(set) =
                    self.modifications.added_by_library.get_mut(&library_uuid)
                {
                    set.remove(&movie_uuid);
                    if set.is_empty() {
                        self.modifications
                            .added_by_library
                            .remove(&library_uuid);
                    }
                }

                self.modifications.unmark_runtime_only(&movie_uuid);
            }
        }

        outcome.movie_ids = ids;
        Ok(outcome)
    }

    pub(super) fn install_series_bundle_internal(
        &mut self,
        library_id: LibraryId,
        series_id: ferrex_core::player_prelude::SeriesID,
        bytes: AlignedVec,
    ) -> RepositoryResult<SeriesBundleInstallOutcome> {
        let key = SeriesBundleKey::new(library_id, series_id);
        let (series_uuid, season_ids, episode_ids) =
            self.series_bundles.upsert_bundle(key, bytes)?;

        let library_uuid = library_id.to_uuid();
        let mut outcome = SeriesBundleInstallOutcome {
            series_indexed: 1,
            seasons_indexed: season_ids.len(),
            episodes_indexed: episode_ids.len(),
            series_id: series_uuid,
            season_ids: season_ids.clone(),
            episode_ids: episode_ids.clone(),

            ..Default::default()
        };

        let mut maybe_replace_runtime = |id: Uuid, repo: &mut MediaRepo| {
            repo.media_id_index.insert(id, library_uuid);
            repo.materialized_media_remove(&id);
            if repo.modifications.is_runtime_only(&id) {
                let removed = repo.modifications.added.remove(&id);
                if removed.is_some() {
                    outcome.items_replaced_from_runtime_overlay += 1;
                }

                if let Some(set) =
                    repo.modifications.added_by_library.get_mut(&library_uuid)
                {
                    set.remove(&id);
                    if set.is_empty() {
                        repo.modifications
                            .added_by_library
                            .remove(&library_uuid);
                    }
                }

                repo.modifications.unmark_runtime_only(&id);
            }
        };

        maybe_replace_runtime(series_uuid, self);
        for id in season_ids {
            maybe_replace_runtime(id, self);
        }
        for id in episode_ids {
            maybe_replace_runtime(id, self);
        }

        Ok(outcome)
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<Media> {
        let uuid = id.as_uuid();
        let mut buf = Uuid::encode_buffer();

        // Check if deleted in runtime
        if self.modifications.is_deleted(uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: id.to_string_buf(&mut buf),
            });
        }

        // Check runtime modifications first
        if let Some(modified) = self.modifications.get_entry(uuid) {
            return modified.deserialize();
        }

        // Movies can be backed by a batch overlay (zero-copy yoke path).
        if id.media_type() == VideoMediaType::Movie
            && let Some((cart, index)) =
                self.movie_batches.get_movie_locator(uuid)
        {
            let archived = unsafe {
                rkyv::access_unchecked::<
                    rkyv::Archived<MovieReferenceBatchResponse>,
                >(&cart)
            };
            let movie =
                archived.movies.get(index as usize).ok_or_else(|| {
                    RepositoryError::NotFound {
                        entity_type: "Movie".to_string(),
                        id: id.to_string_buf(&mut buf),
                    }
                })?;

            let owned: MovieReference = movie.try_to_model().map_err(|e| {
                RepositoryError::DeserializationError(e.to_string())
            })?;
            return Ok(Media::Movie(Box::new(owned)));
        }

        // TV media can be backed by a per-series bundle overlay.
        match id.media_type() {
            VideoMediaType::Series => {
                if let Some(cart) = self.series_bundles.get_series_cart(uuid) {
                    let archived = unsafe {
                        rkyv::access_unchecked::<rkyv::Archived<ferrex_core::player_prelude::SeriesBundleResponse>>(
                            &cart,
                        )
                    };
                    let owned =
                        archived.series.try_to_model().map_err(|e| {
                            RepositoryError::DeserializationError(e.to_string())
                        })?;
                    return Ok(Media::Series(Box::new(owned)));
                }
            }
            VideoMediaType::Season => {
                if let Some((cart, index)) =
                    self.series_bundles.get_season_locator(uuid)
                {
                    let archived = unsafe {
                        rkyv::access_unchecked::<rkyv::Archived<ferrex_core::player_prelude::SeriesBundleResponse>>(
                            &cart,
                        )
                    };
                    let season = archived
                        .seasons
                        .get(index as usize)
                        .ok_or_else(|| RepositoryError::NotFound {
                            entity_type: "Season".to_string(),
                            id: id.to_string_buf(&mut buf),
                        })?;
                    let owned = season.try_to_model().map_err(|e| {
                        RepositoryError::DeserializationError(e.to_string())
                    })?;
                    return Ok(Media::Season(Box::new(owned)));
                }
            }
            VideoMediaType::Episode => {
                if let Some((cart, index)) =
                    self.series_bundles.get_episode_locator(uuid)
                {
                    let archived = unsafe {
                        rkyv::access_unchecked::<rkyv::Archived<ferrex_core::player_prelude::SeriesBundleResponse>>(
                            &cart,
                        )
                    };
                    let episode = archived
                        .episodes
                        .get(index as usize)
                        .ok_or_else(|| RepositoryError::NotFound {
                            entity_type: "Episode".to_string(),
                            id: id.to_string_buf(&mut buf),
                        })?;
                    let owned = episode.try_to_model().map_err(|e| {
                        RepositoryError::DeserializationError(e.to_string())
                    })?;
                    return Ok(Media::Episode(Box::new(owned)));
                }
            }
            _ => {}
        }

        // // Look up in archived data using index
        // if let Some(&library_id) = self.media_id_index.get(uuid) {
        //     // Access archived data on demand
        //     let archived_libraries = unsafe {
        //         // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
        //         rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(
        //             &self.libraries_buffer,
        //         )
        //     };

        //     // Find the library
        //     for library in archived_libraries.iter() {
        //         if library.get_id().as_uuid() == library_id {
        //             if let Some(media_list) = library.media() {
        //                 for media_ref in media_list.iter() {
        //                     if media_ref.archived_media_id().as_uuid() == uuid {
        //                         // Deserialize to owned
        //                         let owned =
        //                             media_ref.try_to_model().map_err(|e| {
        //                                 RepositoryError::DeserializationError(
        //                                     e.to_string(),
        //                                 )
        //                             })?;
        //                         return Ok(owned);
        //                     }
        //                 }
        //             }
        //             break;
        //         }
        //     }
        // }

        Err(RepositoryError::NotFound {
            entity_type: "media".to_string(),
            id: id.to_string_buf(&mut buf),
        })
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_media_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<MediaYoke> {
        let uuid = id.as_uuid();
        let expected_type = id.media_type();

        // Check if deleted in runtime
        if self.modifications.is_deleted(uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        // Check runtime overlay first
        if let Some(entry) = self.modifications.get_entry(uuid) {
            let cart = entry.cart();
            return Ok(MediaYoke::attach_to_cart(
                cart,
                move |data: &AlignedVec| unsafe {
                    let archived =
                        rkyv::access_unchecked::<ArchivedMedia>(data);
                    debug_assert_eq!(archived.media_type(), expected_type);
                    archived
                },
            ));
        }

        if let Some(entry) = self.materialized_media_get(uuid) {
            let cart = entry.cart();
            return Ok(MediaYoke::attach_to_cart(
                cart,
                move |data: &AlignedVec| unsafe {
                    let archived =
                        rkyv::access_unchecked::<ArchivedMedia>(data);
                    debug_assert_eq!(archived.media_type(), expected_type);
                    archived
                },
            ));
        }

        // Prefer batch / bundle overlays (sources of truth) over the legacy library snapshot.
        match expected_type {
            VideoMediaType::Movie => {
                if let Some((cart, index)) =
                    self.movie_batches.get_movie_locator(uuid)
                {
                    let archived = unsafe {
                        rkyv::access_unchecked::<
                            rkyv::Archived<MovieReferenceBatchResponse>,
                        >(&cart)
                    };
                    let movie = archived
                        .movies
                        .get(index as usize)
                        .ok_or_else(|| RepositoryError::NotFound {
                            entity_type: "Movie".to_string(),
                            id: uuid.to_string(),
                        })?;

                    let owned: MovieReference =
                        movie.try_to_model().map_err(|e| {
                            RepositoryError::DeserializationError(e.to_string())
                        })?;
                    let materialized = RuntimeMediaEntry::from_media(
                        &Media::Movie(Box::new(owned)),
                    )?;
                    self.materialized_media_insert(*uuid, materialized.clone());

                    let cart = materialized.cart();
                    return Ok(MediaYoke::attach_to_cart(
                        cart,
                        move |data: &AlignedVec| unsafe {
                            let archived =
                                rkyv::access_unchecked::<ArchivedMedia>(data);
                            debug_assert_eq!(
                                archived.media_type(),
                                expected_type
                            );
                            archived
                        },
                    ));
                }
            }
            VideoMediaType::Series => {
                if let Some(cart) = self.series_bundles.get_series_cart(uuid) {
                    let archived = unsafe {
                        rkyv::access_unchecked::<
                            rkyv::Archived<
                                ferrex_core::player_prelude::SeriesBundleResponse,
                            >,
                        >(&cart)
                    };
                    let owned =
                        archived.series.try_to_model().map_err(|e| {
                            RepositoryError::DeserializationError(e.to_string())
                        })?;
                    let materialized = RuntimeMediaEntry::from_media(
                        &Media::Series(Box::new(owned)),
                    )?;
                    self.materialized_media_insert(*uuid, materialized.clone());

                    let cart = materialized.cart();
                    return Ok(MediaYoke::attach_to_cart(
                        cart,
                        move |data: &AlignedVec| unsafe {
                            let archived =
                                rkyv::access_unchecked::<ArchivedMedia>(data);
                            debug_assert_eq!(
                                archived.media_type(),
                                expected_type
                            );
                            archived
                        },
                    ));
                }
            }
            VideoMediaType::Season => {
                if let Some((cart, index)) =
                    self.series_bundles.get_season_locator(uuid)
                {
                    let archived = unsafe {
                        rkyv::access_unchecked::<
                            rkyv::Archived<
                                ferrex_core::player_prelude::SeriesBundleResponse,
                            >,
                        >(&cart)
                    };
                    let season = archived
                        .seasons
                        .get(index as usize)
                        .ok_or_else(|| RepositoryError::NotFound {
                            entity_type: "Season".to_string(),
                            id: uuid.to_string(),
                        })?;
                    let owned = season.try_to_model().map_err(|e| {
                        RepositoryError::DeserializationError(e.to_string())
                    })?;
                    let materialized = RuntimeMediaEntry::from_media(
                        &Media::Season(Box::new(owned)),
                    )?;
                    self.materialized_media_insert(*uuid, materialized.clone());

                    let cart = materialized.cart();
                    return Ok(MediaYoke::attach_to_cart(
                        cart,
                        move |data: &AlignedVec| unsafe {
                            let archived =
                                rkyv::access_unchecked::<ArchivedMedia>(data);
                            debug_assert_eq!(
                                archived.media_type(),
                                expected_type
                            );
                            archived
                        },
                    ));
                }
            }
            VideoMediaType::Episode => {
                if let Some((cart, index)) =
                    self.series_bundles.get_episode_locator(uuid)
                {
                    let archived = unsafe {
                        rkyv::access_unchecked::<
                            rkyv::Archived<
                                ferrex_core::player_prelude::SeriesBundleResponse,
                            >,
                        >(&cart)
                    };
                    let episode = archived
                        .episodes
                        .get(index as usize)
                        .ok_or_else(|| RepositoryError::NotFound {
                            entity_type: "Episode".to_string(),
                            id: uuid.to_string(),
                        })?;
                    let owned = episode.try_to_model().map_err(|e| {
                        RepositoryError::DeserializationError(e.to_string())
                    })?;
                    let materialized = RuntimeMediaEntry::from_media(
                        &Media::Episode(Box::new(owned)),
                    )?;
                    self.materialized_media_insert(*uuid, materialized.clone());

                    let cart = materialized.cart();
                    return Ok(MediaYoke::attach_to_cart(
                        cart,
                        move |data: &AlignedVec| unsafe {
                            let archived =
                                rkyv::access_unchecked::<ArchivedMedia>(data);
                            debug_assert_eq!(
                                archived.media_type(),
                                expected_type
                            );
                            archived
                        },
                    ));
                }
            }
        }

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            // Validate positions to avoid panics during yoke access
            let (lib_idx, media_idx) = self
                .find_media_position(library_id, *uuid, Some(expected_type))
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "media".to_string(),
                    id: uuid.to_string(),
                })?;

            Ok(MediaYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                move |data: &AlignedVec| unsafe {
                    let archived_libraries = rkyv::access_unchecked::<
                        ArchivedVec<ArchivedLibrary>,
                    >(data);
                    let library = archived_libraries
                        .get(lib_idx)
                        .expect("validated library index");
                    let media_list =
                        library.media().expect("validated media list");
                    media_list.get(media_idx).expect("validated media index")
                },
            ))
        } else {
            Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            })
        }
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_movie_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<MovieYoke> {
        let uuid = id.as_uuid();
        // Check if deleted in runtime
        if self.modifications.is_deleted(uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        if let Some(entry) = self.modifications.get_entry(uuid) {
            let cart = entry.cart();
            return Ok(MovieYoke::attach_to_cart(
                cart,
                |data: &AlignedVec| unsafe {
                    match rkyv::access_unchecked::<ArchivedMedia>(data) {
                        ArchivedMedia::Movie(movie) => movie,
                        _ => unreachable!("Overlay entry variant mismatch"),
                    }
                },
            ));
        }

        if let Some((cart, index)) = self.movie_batches.get_movie_locator(uuid)
        {
            return Ok(MovieYoke::attach_to_cart(
                cart,
                move |data: &AlignedVec| unsafe {
                    let archived = rkyv::access_unchecked::<
                        rkyv::Archived<MovieReferenceBatchResponse>,
                    >(data);
                    archived
                        .movies
                        .get(index as usize)
                        .expect("validated batch movie index")
                },
            ));
        }

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let (lib_idx, media_idx) = self
                .find_media_position(
                    library_id,
                    *uuid,
                    Some(VideoMediaType::Movie),
                )
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "Movie".to_string(),
                    id: uuid.to_string(),
                })?;

            Ok(MovieYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                move |data: &AlignedVec| unsafe {
                    let archived_libraries = rkyv::access_unchecked::<
                        ArchivedVec<ArchivedLibrary>,
                    >(data);
                    let library = archived_libraries
                        .get(lib_idx)
                        .expect("validated library index");
                    let media_list =
                        library.media().expect("validated media list");
                    match media_list
                        .get(media_idx)
                        .expect("validated media index")
                    {
                        ArchivedMedia::Movie(movie) => movie,
                        _ => unreachable!("validated movie type"),
                    }
                },
            ))
        } else {
            Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            })
        }
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_series_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<SeriesYoke> {
        let uuid = id.as_uuid();
        // Check if deleted in runtime
        if self.modifications.is_deleted(uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        if let Some(entry) = self.modifications.get_entry(uuid) {
            let cart = entry.cart();
            return Ok(SeriesYoke::attach_to_cart(
                cart,
                |data: &AlignedVec| unsafe {
                    match rkyv::access_unchecked::<ArchivedMedia>(data) {
                        ArchivedMedia::Series(series) => series,
                        _ => unreachable!("Overlay entry variant mismatch"),
                    }
                },
            ));
        }

        if let Some(cart) = self.series_bundles.get_series_cart(uuid) {
            return Ok(SeriesYoke::attach_to_cart(
                cart,
                |data: &AlignedVec| unsafe {
                    let archived = rkyv::access_unchecked::<
                        rkyv::Archived<
                            ferrex_core::player_prelude::SeriesBundleResponse,
                        >,
                    >(data);
                    &archived.series
                },
            ));
        }

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let (lib_idx, media_idx) = self
                .find_media_position(
                    library_id,
                    *uuid,
                    Some(VideoMediaType::Series),
                )
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "Series".to_string(),
                    id: uuid.to_string(),
                })?;

            Ok(SeriesYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                move |data: &AlignedVec| unsafe {
                    let archived_libraries = rkyv::access_unchecked::<
                        ArchivedVec<ArchivedLibrary>,
                    >(data);
                    let library = archived_libraries
                        .get(lib_idx)
                        .expect("validated library index");
                    let media_list =
                        library.media().expect("validated media list");
                    match media_list
                        .get(media_idx)
                        .expect("validated media index")
                    {
                        ArchivedMedia::Series(series) => series,
                        _ => unreachable!("validated series type"),
                    }
                },
            ))
        } else {
            Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            })
        }
    }

    pub(super) fn get_season_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<SeasonYoke> {
        let uuid = id.as_uuid();
        if self.modifications.is_deleted(uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        if let Some(entry) = self.modifications.get_entry(uuid) {
            let cart = entry.cart();
            return Ok(SeasonYoke::attach_to_cart(
                cart,
                |data: &AlignedVec| unsafe {
                    match rkyv::access_unchecked::<ArchivedMedia>(data) {
                        ArchivedMedia::Season(season) => season,
                        _ => unreachable!("Overlay entry variant mismatch"),
                    }
                },
            ));
        }

        if let Some((cart, index)) =
            self.series_bundles.get_season_locator(uuid)
        {
            return Ok(SeasonYoke::attach_to_cart(
                cart,
                move |data: &AlignedVec| unsafe {
                    let archived = rkyv::access_unchecked::<
                        rkyv::Archived<
                            ferrex_core::player_prelude::SeriesBundleResponse,
                        >,
                    >(data);
                    archived
                        .seasons
                        .get(index as usize)
                        .expect("validated bundle season index")
                },
            ));
        }

        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let (lib_idx, media_idx) = self
                .find_media_position(
                    library_id,
                    *uuid,
                    Some(VideoMediaType::Season),
                )
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "Season".to_string(),
                    id: uuid.to_string(),
                })?;
            return Ok(SeasonYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                move |data: &AlignedVec| unsafe {
                    let archived_libraries = rkyv::access_unchecked::<
                        ArchivedVec<ArchivedLibrary>,
                    >(data);
                    let library = archived_libraries
                        .get(lib_idx)
                        .expect("validated library index");
                    let media_list =
                        library.media().expect("validated media list");
                    match media_list
                        .get(media_idx)
                        .expect("validated media index")
                    {
                        ArchivedMedia::Season(season) => season,
                        _ => unreachable!("validated season type"),
                    }
                },
            ));
        }

        Err(RepositoryError::NotFound {
            entity_type: "media".to_string(),
            id: uuid.to_string(),
        })
    }

    pub(super) fn get_episode_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<EpisodeYoke> {
        let uuid = id.as_uuid();
        if self.modifications.is_deleted(uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        if let Some(entry) = self.modifications.get_entry(uuid) {
            let cart = entry.cart();
            return Ok(EpisodeYoke::attach_to_cart(
                cart,
                |data: &AlignedVec| unsafe {
                    match rkyv::access_unchecked::<ArchivedMedia>(data) {
                        ArchivedMedia::Episode(episode) => episode,
                        _ => unreachable!("Overlay entry variant mismatch"),
                    }
                },
            ));
        }

        if let Some((cart, index)) =
            self.series_bundles.get_episode_locator(uuid)
        {
            return Ok(EpisodeYoke::attach_to_cart(
                cart,
                move |data: &AlignedVec| unsafe {
                    let archived = rkyv::access_unchecked::<
                        rkyv::Archived<
                            ferrex_core::player_prelude::SeriesBundleResponse,
                        >,
                    >(data);
                    archived
                        .episodes
                        .get(index as usize)
                        .expect("validated bundle episode index")
                },
            ));
        }

        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let (lib_idx, media_idx) = self
                .find_media_position(
                    library_id,
                    *uuid,
                    Some(VideoMediaType::Episode),
                )
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "Episode".to_string(),
                    id: uuid.to_string(),
                })?;
            return Ok(EpisodeYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                move |data: &AlignedVec| unsafe {
                    let archived_libraries = rkyv::access_unchecked::<
                        ArchivedVec<ArchivedLibrary>,
                    >(data);
                    let library = archived_libraries
                        .get(lib_idx)
                        .expect("validated library index");
                    let media_list =
                        library.media().expect("validated media list");
                    match media_list
                        .get(media_idx)
                        .expect("validated media index")
                    {
                        ArchivedMedia::Episode(ep) => ep,
                        _ => unreachable!("validated episode type"),
                    }
                },
            ));
        }

        Err(RepositoryError::NotFound {
            entity_type: "media".to_string(),
            id: uuid.to_string(),
        })
    }

    /// Internal method to get all media from a library
    pub(super) fn get_library_media_internal(
        &self,
        library_id: &LibraryId,
    ) -> RepositoryResult<Vec<Media>> {
        let mut results = Vec::new();
        let mut seen = HashSet::new();

        // // Find the library and collect its media
        // for library in archived_libraries.iter() {
        //     if library.get_id().as_uuid() == library_id.to_uuid() {
        //         if let Some(media_list) = library.media() {
        //             for media_ref in media_list.iter() {
        //                 let media_id = media_ref.archived_media_id().to_uuid();

        //                 // Skip if deleted
        //                 if self.modifications.is_deleted(&media_id) {
        //                     continue;
        //                 }

        //                 // Use modified version if available
        //                 if let Some(modified) =
        //                     self.modifications.get_entry(&media_id)
        //                 {
        //                     results.push(modified.deserialize()?);
        //                     seen.insert(media_id);
        //                 } else {
        //                     // Deserialize archived version
        //                     let owned =
        //                         media_ref.try_to_model().map_err(|e| {
        //                             RepositoryError::DeserializationError(
        //                                 e.to_string(),
        //                             )
        //                         })?;
        //                     seen.insert(media_id);
        //                     results.push(owned);
        //                 }
        //             }
        //         }
        //         break;
        //     }
        // }

        // Add any new items added at runtime for this library
        let lib_uuid = library_id.as_uuid();
        if let Some(ids) = self.modifications.added_by_library.get(lib_uuid) {
            for media_id in ids {
                if let Some(media) = self.modifications.added.get(media_id) {
                    let owned = media.deserialize()?;
                    seen.insert(*media_id);
                    results.push(owned);
                }
            }
        }

        for movie_uuid in self.movie_batches.movie_ids_for_library(library_id) {
            if seen.contains(&movie_uuid) {
                continue;
            }

            if self.modifications.is_deleted(&movie_uuid) {
                continue;
            }

            if let Some(modified) = self.modifications.get_entry(&movie_uuid) {
                results.push(modified.deserialize()?);
                seen.insert(movie_uuid);
                continue;
            }

            let Some((cart, index)) =
                self.movie_batches.get_movie_locator(&movie_uuid)
            else {
                continue;
            };

            let archived = unsafe {
                rkyv::access_unchecked::<
                    rkyv::Archived<MovieReferenceBatchResponse>,
                >(&cart)
            };
            let Some(movie) = archived.movies.get(index as usize) else {
                continue;
            };
            let owned: MovieReference = movie.try_to_model().map_err(|e| {
                RepositoryError::DeserializationError(e.to_string())
            })?;
            results.push(Media::Movie(Box::new(owned)));
            seen.insert(movie_uuid);
        }

        for series_uuid in
            self.series_bundles.series_ids_for_library(library_id)
        {
            if seen.contains(&series_uuid) {
                continue;
            }

            if self.modifications.is_deleted(&series_uuid) {
                continue;
            }

            if let Some(modified) = self.modifications.get_entry(&series_uuid) {
                results.push(modified.deserialize()?);
                seen.insert(series_uuid);
                continue;
            }

            if let Some(cart) =
                self.series_bundles.get_series_cart(&series_uuid)
            {
                let archived = unsafe {
                    rkyv::access_unchecked::<
                        rkyv::Archived<
                            ferrex_core::player_prelude::SeriesBundleResponse,
                        >,
                    >(&cart)
                };
                let owned = archived.series.try_to_model().map_err(|e| {
                    RepositoryError::DeserializationError(e.to_string())
                })?;
                results.push(Media::Series(Box::new(owned)));
                seen.insert(series_uuid);
            }
        }

        Ok(results)
    }

    /*
    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_movie_yokes_by_library_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<Vec<MovieYoke>> {
        let uuid = id.as_uuid();

        // Check if deleted in runtime
        if self.modifications.is_deleted(&uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        let num_media = self.libraries_index.len();
        let mut yokes = Vec::with_capacity(num_libraries);

        // Access the archived data directly without unsafe transmute
        let archived_libraries = rkyv::access::<ArchivedVec<ArchivedLibrary>, Error>(&buffer)?;

        // Build indices
        for library in archived_libraries.iter() {
            let library_id = library.get_id().as_uuid();
            libraries_index.push(library_id);

            if let Some(media_list) = library.media() {
                for media in media_list.iter() {
                    let media_id = media.archived_media_id().as_uuid();
                    media_id_index.insert(media_id, library_id);
                }
            }


        // TODO: Fix this unwrap and make sure that we get the correct libraries
        for (index, _) in self.libraries_index.iter().enumerate() {
            yokes.push(
                Yoke::<&'static ArchivedLibrary, Arc<AlignedVec>>::attach_to_cart(
                    Arc::clone(&self.libraries_buffer),
                    |data: &AlignedVec| unsafe {
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data)
                            .get(index)
                            .unwrap()
                    },
                ),
            );
        }
        yokes
        if let Some(&library_id) = self.media_id_index.get(&uuid) {
            let media_type = id.media_type();

            return Ok(MediaYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data);
                    archived_libraries
                        .iter()
                        .find(|l| l.get_id().as_uuid() == library_id)
                        .unwrap()
                        .media()
                        .unwrap()
                        .iter()
                },
            ));
        } else {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }
    }*/

    /// Get all libraries
    pub(super) fn get_libraries_internal(&self) -> Vec<Library> {
        let Ok(archived_libraries) = rkyv::access::<
            ArchivedVec<ArchivedLibrary>,
            Error,
        >(&self.libraries_buffer) else {
            return Vec::new();
        };

        archived_libraries
            .iter()
            .filter_map(|archived| archived.try_to_model().ok())
            .collect()
    }

    /*
    /// Get all archived libraries
    pub(super) fn get_archived_libraries_yoke_internal(
        &self,
    ) -> Yoke<&'static ArchivedVec<ArchivedLibrary>, Arc<AlignedVec>> {
        Yoke::<&'static ArchivedVec<ArchivedLibrary>, Arc<AlignedVec>>::attach_to_cart(
            Arc::clone(&self.libraries_buffer),
            |data: &AlignedVec| unsafe {
                rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data)
            },
        )
    } */

    /// Get all archived libraries
    pub(super) fn get_archived_libraries_yoke_internal(
        &self,
    ) -> Vec<Yoke<&'static ArchivedLibrary, Arc<AlignedVec>>> {
        let num_libraries = self.libraries_index.len();
        let mut yokes = Vec::with_capacity(num_libraries);

        // TODO: Fix this unwrap and make sure that we get the correct libraries
        for (index, _) in self.libraries_index.iter().enumerate() {
            yokes.push(
                Yoke::<&'static ArchivedLibrary, Arc<AlignedVec>>::attach_to_cart(
                    Arc::clone(&self.libraries_buffer),
                    |data: &AlignedVec| unsafe {
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data)
                            .get(index)
                            .unwrap()
                    },
                ),
            );
        }
        yokes
    }

    /// Get all archived libraries
    pub(super) fn get_archived_libraries_internal(
        &self,
    ) -> &ArchivedVec<ArchivedLibrary> {
        // If the repo is initialized without a library snapshot (i.e. overlay-only),
        // this is not meaningful; prefer the safe `get_libraries_internal` path.
        //
        // This method is used by some legacy call sites; keep it but ensure the
        // buffer is actually a valid archive before returning a reference.
        rkyv::access::<ArchivedVec<ArchivedLibrary>, Error>(&self.libraries_buffer)
            .expect("get_archived_libraries_internal called without a valid library snapshot")
    }

    /// Get a specific library by ID
    pub(super) fn get_library_internal(
        &self,
        library_id: &LibraryId,
    ) -> Option<Library> {
        let Ok(archived_libraries) = rkyv::access::<
            ArchivedVec<ArchivedLibrary>,
            Error,
        >(&self.libraries_buffer) else {
            return None;
        };

        for library in archived_libraries.iter() {
            if library.get_id().as_uuid() == library_id.to_uuid() {
                return library.try_to_model().ok();
            }
        }
        None
    }

    /// Get a specific archived library by ID
    pub(super) fn get_archived_library_yoke_internal(
        &self,
        library_id: &Uuid,
    ) -> Option<LibraryYoke> {
        if rkyv::access::<ArchivedVec<ArchivedLibrary>, Error>(
            &self.libraries_buffer,
        )
        .is_err()
        {
            return None;
        }
        if !self.libraries_index.contains(library_id) {
            return None;
        }
        Some(LibraryYoke::attach_to_cart(
            Arc::clone(&self.libraries_buffer),
            |data: &AlignedVec| unsafe {
                rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data)
                    .iter()
                    .find(|library| &library.get_id().as_uuid() == library_id)
                    .unwrap()
            },
        ))
    }

    /// Get all media from all libraries
    pub(super) fn get_all_media_internal(&self) -> Vec<Media> {
        let mut results = Vec::new();
        let mut seen = HashSet::new();

        let archived_libraries = unsafe {
            // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(
                &self.libraries_buffer,
            )
        };

        for library in archived_libraries.iter() {
            if let Some(media_list) = library.media() {
                for media_ref in media_list.iter() {
                    let media_id = media_ref.archived_media_id().to_uuid();

                    // Skip if deleted
                    if self.modifications.is_deleted(&media_id) {
                        continue;
                    }

                    // Use modified version if available
                    if let Some(modified) =
                        self.modifications.get_entry(&media_id)
                    {
                        if let Ok(deser) = modified.deserialize() {
                            results.push(deser);
                            seen.insert(media_id);
                        }
                    } else {
                        // Deserialize archived version
                        if let Ok(owned) = media_ref.try_to_model() {
                            results.push(owned);
                            seen.insert(media_id);
                        }
                    }
                }
            }
        }

        // Add runtime additions (from all libraries)
        for (id, entry) in self.modifications.added.iter() {
            if let Ok(deser) = entry.deserialize() {
                results.push(deser);
                seen.insert(*id);
            }
        }

        // Add batch-backed movies not already included.
        for movie_uuid in self.movie_batches.all_movie_ids() {
            if seen.contains(&movie_uuid) {
                continue;
            }
            if self.modifications.is_deleted(&movie_uuid) {
                continue;
            }
            if let Some(modified) = self.modifications.get_entry(&movie_uuid)
                && let Ok(deser) = modified.deserialize()
            {
                results.push(deser);
                seen.insert(movie_uuid);
                continue;
            }

            let Some((cart, index)) =
                self.movie_batches.get_movie_locator(&movie_uuid)
            else {
                continue;
            };
            let archived = unsafe {
                rkyv::access_unchecked::<
                    rkyv::Archived<MovieReferenceBatchResponse>,
                >(&cart)
            };
            let Some(movie) = archived.movies.get(index as usize) else {
                continue;
            };
            if let Ok(owned) = movie.try_to_model() {
                results.push(Media::Movie(Box::new(owned)));
                seen.insert(movie_uuid);
            }
        }

        results
    }

    /// Get all seasons for a given series
    pub(super) fn get_series_seasons_internal(
        &self,
        series_id: &SeriesID,
    ) -> RepositoryResult<Vec<SeasonReference>> {
        let series_uuid = series_id.as_uuid();

        if let Some(cart) = self.series_bundles.get_series_cart(series_uuid) {
            let archived = unsafe {
                rkyv::access_unchecked::<
                    rkyv::Archived<
                        ferrex_core::player_prelude::SeriesBundleResponse,
                    >,
                >(&cart)
            };

            let mut out = Vec::with_capacity(archived.seasons.len());
            for season in archived.seasons.iter() {
                if let Ok(model) = season.try_to_model() {
                    out.push(model);
                }
            }
            out.sort_by_key(|s| s.season_number.value());
            return Ok(out);
        }

        // Determine which library this series belongs to
        let &library_id =
            self.media_id_index.get(series_uuid).ok_or_else(|| {
                RepositoryError::NotFound {
                    entity_type: "Series".to_string(),
                    id: series_uuid.to_string(),
                }
            })?;

        let mut results: Vec<SeasonReference> = Vec::new();

        // Access archived data for the library
        let archived_libraries = unsafe {
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(
                &self.libraries_buffer,
            )
        };

        if let Some(library) = archived_libraries
            .iter()
            .find(|l| l.get_id().as_uuid() == library_id)
            && let Some(media_list) = library.media()
        {
            for media_ref in media_list.iter() {
                // Skip if deleted via runtime overlay
                let media_uuid = media_ref.archived_media_id().to_uuid();
                if self.modifications.is_deleted(&media_uuid) {
                    continue;
                }

                if let ArchivedMedia::Season(season) = media_ref {
                    // Match parent series
                    if season.series_id.as_uuid() == series_uuid {
                        // Prefer runtime modified version if present
                        if let Some(modified) =
                            self.modifications.get_entry(&media_uuid)
                        {
                            if let Some(season) =
                                modified.deserialize()?.to_season()
                            {
                                results.push(*season);
                            } else if let Ok(Media::Season(s)) =
                                media_ref.try_to_model()
                            {
                                results.push(*s);
                            }
                        } else if let Ok(Media::Season(s)) =
                            media_ref.try_to_model()
                        {
                            results.push(*s);
                        }
                    }
                }
            }
        }

        // Include runtime-added media in this library that match the series
        if let Some(ids) = self.modifications.added_by_library.get(&library_id)
        {
            for id in ids {
                if let Some(media) = self.modifications.added.get(id)
                    && let Some(season) = media.deserialize()?.to_season()
                    && &season.series_id == series_id
                {
                    results.push(*season);
                }
            }
        }

        // Sort by season number ascending
        results.sort_by_key(|s| s.season_number.value());

        Ok(results)
    }

    /// Get all episodes for a given season
    pub(super) fn get_season_episodes_internal(
        &self,
        season_id: &SeasonID,
    ) -> RepositoryResult<Vec<EpisodeReference>> {
        let season_uuid = season_id.as_uuid();

        if let Some((cart, _)) =
            self.series_bundles.get_season_locator(season_uuid)
        {
            let archived = unsafe {
                rkyv::access_unchecked::<
                    rkyv::Archived<
                        ferrex_core::player_prelude::SeriesBundleResponse,
                    >,
                >(&cart)
            };

            let mut out = Vec::new();
            for episode in archived.episodes.iter() {
                if episode.season_id.as_uuid() != season_uuid {
                    continue;
                }
                if let Ok(model) = episode.try_to_model() {
                    out.push(model);
                }
            }
            out.sort_by_key(|e| e.episode_number.value());
            return Ok(out);
        }

        // Determine which library this season belongs to
        let &library_id =
            self.media_id_index.get(season_uuid).ok_or_else(|| {
                RepositoryError::NotFound {
                    entity_type: "Season".to_string(),
                    id: season_uuid.to_string(),
                }
            })?;

        let mut results: Vec<EpisodeReference> = Vec::new();

        // Access archived data for the library
        let archived_libraries = unsafe {
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(
                &self.libraries_buffer,
            )
        };

        if let Some(library) = archived_libraries
            .iter()
            .find(|l| l.get_id().as_uuid() == library_id)
            && let Some(media_list) = library.media()
        {
            for media_ref in media_list.iter() {
                // Skip if deleted via runtime overlay
                let media_uuid = media_ref.archived_media_id().to_uuid();
                if self.modifications.is_deleted(&media_uuid) {
                    continue;
                }

                if let ArchivedMedia::Episode(ep) = media_ref
                    && ep.season_id.as_uuid() == season_uuid
                {
                    if let Some(modified) =
                        self.modifications.get_entry(&media_uuid)
                    {
                        if let Some(ep) = modified.deserialize()?.to_episode() {
                            results.push(*ep);
                        } else if let Ok(Media::Episode(e)) =
                            media_ref.try_to_model()
                        {
                            results.push(*e);
                        }
                    } else if let Ok(Media::Episode(e)) =
                        media_ref.try_to_model()
                    {
                        results.push(*e);
                    }
                }
            }
        }

        // Include runtime-added media in this library that match the season
        if let Some(ids) = self.modifications.added_by_library.get(&library_id)
        {
            for id in ids {
                if let Some(media) = self.modifications.added.get(id)
                    && let Some(ep) = media.deserialize()?.to_episode()
                    && &ep.season_id == season_id
                {
                    results.push(*ep);
                }
            }
        }

        // Sort by episode number
        results.sort_by_key(|e| e.episode_number.value());

        Ok(results)
    }
}

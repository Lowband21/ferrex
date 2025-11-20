use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ferrex_core::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedMedia, EpisodeReference, LibraryID, Media,
    MediaIDLike, MediaLike, MediaType, SeasonID, SeasonReference, SeriesID, SortBy, SortOrder,
};
use rkyv::{deserialize, rancor::Error, to_bytes, util::AlignedVec, vec::ArchivedVec};
use uuid::Uuid;
use yoke::Yoke;

use crate::infrastructure::repository::{RepositoryError, RepositoryResult};

use super::{EpisodeYoke, LibraryYoke, MediaYoke, MovieYoke, SeasonYoke, SeriesYoke};

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
        let archived = unsafe { rkyv::access_unchecked::<ArchivedMedia>(&self.buffer) };
        f(archived)
    }

    pub(super) fn media_type(&self) -> MediaType {
        self.with_archived(|arch| arch.media_type())
    }

    pub(super) fn deserialize(&self) -> RepositoryResult<Media> {
        self.with_archived(deserialize::<Media, Error>)
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

    // Cached sorted indices, should be fetched from server
    pub(super) sorted_indices: Option<HashMap<Uuid, Vec<Uuid>>>, // Hashmap of library IDs to Vec of media IDs

    // Current sort criteria
    pub(super) current_library_sort_states: Option<HashMap<Uuid, (SortBy, SortOrder)>>, // Hashmap of library IDs to sort criteria

                                                                                        //pending_events: Vec<MediaChangeEvent>,
}

impl MediaRepo {
    pub fn new(bytes: AlignedVec) -> Result<Self, Error> {
        let buffer = Arc::new(bytes);
        let mut media_id_index = HashMap::new();
        let mut libraries_index = Vec::new();

        // Access the archived data directly without unsafe transmute
        let archived_libraries = rkyv::access::<ArchivedVec<ArchivedLibrary>, Error>(&buffer)?;

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
            sorted_indices: None,
            current_library_sort_states: None,
            //pending_events: Vec::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.libraries_index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.libraries_index.is_empty()
    }

    pub fn clear(&mut self) {
        self.libraries_index.clear();
        self.media_id_index.clear();
        let _ = self.modifications.clear();
        //self.pending_events.clear();
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_internal(&self, id: &impl MediaIDLike) -> RepositoryResult<Media> {
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

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            // Access archived data on demand
            let archived_libraries = unsafe {
                // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
                rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
            };

            // Find the library
            for library in archived_libraries.iter() {
                if library.get_id().as_uuid() == library_id {
                    if let Some(media_list) = library.media() {
                        for media_ref in media_list.iter() {
                            if media_ref.archived_media_id().as_uuid() == uuid {
                                // Deserialize to owned
                                let owned =
                                    deserialize::<Media, Error>(media_ref).map_err(|e| {
                                        RepositoryError::DeserializationError(e.to_string())
                                    })?;
                                return Ok(owned);
                            }
                        }
                    }
                    break;
                }
            }
        }

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
            let expected_type = id.media_type();
            return Ok(MediaYoke::attach_to_cart(
                cart,
                move |data: &AlignedVec| unsafe {
                    let archived = rkyv::access_unchecked::<ArchivedMedia>(data);
                    debug_assert_eq!(archived.media_type(), expected_type);
                    archived
                },
            ));
        }

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let media_type = id.media_type();

            Ok(MediaYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data);
                    archived_libraries
                        .iter()
                        .find(|l| l.get_id().as_uuid() == library_id)
                        .unwrap()
                        .media()
                        .unwrap()
                        .iter()
                        .filter(|m| m.media_type() == media_type)
                        .find(|m| m.archived_media_id().as_uuid() == uuid)
                        .unwrap()
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

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let media_type = id.media_type();

            Ok(MovieYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data);
                    match archived_libraries
                        .iter()
                        .find(|l| l.get_id().as_uuid() == library_id)
                        .unwrap()
                        .media()
                        .unwrap()
                        .iter()
                        .filter(|m| m.media_type() == media_type)
                        .find(|m| m.archived_media_id().as_uuid() == uuid)
                        .unwrap()
                    {
                        ArchivedMedia::Movie(movie) => movie,
                        ArchivedMedia::Series(_)
                        | ArchivedMedia::Season(_)
                        | ArchivedMedia::Episode(_) => {
                            unreachable!("We just checked the media type")
                        }
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

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let media_type = id.media_type();

            Ok(SeriesYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data);
                    match archived_libraries
                        .iter()
                        .find(|l| l.get_id().as_uuid() == library_id)
                        .unwrap()
                        .media()
                        .unwrap()
                        .iter()
                        .filter(|m| m.media_type() == media_type)
                        .find(|m| m.archived_media_id().as_uuid() == uuid)
                        .unwrap()
                    {
                        ArchivedMedia::Series(series) => series,
                        ArchivedMedia::Movie(_)
                        | ArchivedMedia::Season(_)
                        | ArchivedMedia::Episode(_) => {
                            unreachable!("We just checked the media type")
                        }
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

        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let media_type = id.media_type();
            return Ok(SeasonYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data);
                    match archived_libraries
                        .iter()
                        .find(|l| l.get_id().as_uuid() == library_id)
                        .unwrap()
                        .media()
                        .unwrap()
                        .iter()
                        .filter(|m| m.media_type() == media_type)
                        .find(|m| m.archived_media_id().as_uuid() == uuid)
                        .unwrap()
                    {
                        ArchivedMedia::Season(season) => season,
                        _ => unreachable!("We just filtered by media type Season"),
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

        if let Some(&library_id) = self.media_id_index.get(uuid) {
            let media_type = id.media_type();
            return Ok(EpisodeYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(data);
                    match archived_libraries
                        .iter()
                        .find(|l| l.get_id().as_uuid() == library_id)
                        .unwrap()
                        .media()
                        .unwrap()
                        .iter()
                        .filter(|m| m.media_type() == media_type)
                        .find(|m| m.archived_media_id().as_uuid() == uuid)
                        .unwrap()
                    {
                        ArchivedMedia::Episode(ep) => ep,
                        _ => unreachable!("We just filtered by media type Episode"),
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
        library_id: &LibraryID,
    ) -> RepositoryResult<Vec<Media>> {
        let mut results = Vec::new();

        // Access archived data
        let archived_libraries = unsafe {
            // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
        };

        // Find the library and collect its media
        for library in archived_libraries.iter() {
            if library.get_id().as_uuid() == library_id.as_uuid() {
                if let Some(media_list) = library.media() {
                    for media_ref in media_list.iter() {
                        let media_id = media_ref.archived_media_id().to_uuid();

                        // Skip if deleted
                        if self.modifications.is_deleted(&media_id) {
                            continue;
                        }

                        // Use modified version if available
                        if let Some(modified) = self.modifications.get_entry(&media_id) {
                            results.push(modified.deserialize()?);
                        } else {
                            // Deserialize archived version
                            let owned = deserialize::<Media, Error>(media_ref).map_err(|e| {
                                RepositoryError::DeserializationError(e.to_string())
                            })?;
                            results.push(owned);
                        }
                    }
                }
                break;
            }
        }

        // Add any new items added at runtime for this library
        let lib_uuid = library_id.as_uuid();
        if let Some(ids) = self.modifications.added_by_library.get(&lib_uuid) {
            for media_id in ids {
                if let Some(media) = self.modifications.added.get(media_id) {
                    results.push(media.deserialize()?);
                }
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
    pub(super) fn get_libraries_internal(&self) -> Vec<ferrex_core::Library> {
        let archived_libraries = unsafe {
            // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
        };

        archived_libraries
            .iter()
            .map(|archived| {
                // Deserialize to owned Library
                deserialize::<ferrex_core::Library, Error>(archived)
                    .expect("Failed to deserialize library")
            })
            .collect()
    }

    /*
    /// Get all archived libraries
    pub(super) fn get_archived_libraries_yoke_internal(
        &self,
    ) -> Yoke<&'static ArchivedVec<ferrex_core::ArchivedLibrary>, Arc<AlignedVec>> {
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
    ) -> Vec<Yoke<&'static ferrex_core::ArchivedLibrary, Arc<AlignedVec>>> {
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
    pub(super) fn get_archived_libraries_internal(&self) -> &ArchivedVec<ArchivedLibrary> {
        unsafe {
            // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
        }
    }

    /// Get a specific library by ID
    pub(super) fn get_library_internal(
        &self,
        library_id: &LibraryID,
    ) -> Option<ferrex_core::Library> {
        let archived_libraries = unsafe {
            // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
        };

        for library in archived_libraries.iter() {
            if library.get_id().as_uuid() == library_id.as_uuid() {
                return deserialize::<ferrex_core::Library, Error>(library).ok();
            }
        }
        None
    }

    /// Get a specific archived library by ID
    pub(super) fn get_archived_library_yoke_internal(
        &self,
        library_id: &Uuid,
    ) -> Option<LibraryYoke> {
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

        let archived_libraries = unsafe {
            // SAFETY: We hold the buffer through Arc, it won't be dropped while we're using it
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
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
                    if let Some(modified) = self.modifications.get_entry(&media_id) {
                        if let Ok(deser) = modified.deserialize() {
                            results.push(deser);
                        }
                    } else {
                        // Deserialize archived version
                        if let Ok(owned) = deserialize::<Media, Error>(media_ref) {
                            results.push(owned);
                        }
                    }
                }
            }
        }

        // Add runtime additions (from all libraries)
        for media in self.modifications.added.values() {
            if let Ok(deser) = media.deserialize() {
                results.push(deser);
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

        // Determine which library this series belongs to
        let &library_id =
            self.media_id_index
                .get(series_uuid)
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "Series".to_string(),
                    id: series_uuid.to_string(),
                })?;

        let mut results: Vec<SeasonReference> = Vec::new();

        // Access archived data for the library
        let archived_libraries = unsafe {
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
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
                        if let Some(modified) = self.modifications.get_entry(&media_uuid) {
                            if let Some(season) = modified.deserialize()?.to_season() {
                                results.push(season);
                            } else if let Ok(Media::Season(s)) =
                                deserialize::<Media, Error>(media_ref)
                            {
                                results.push(s);
                            }
                        } else if let Ok(Media::Season(s)) = deserialize::<Media, Error>(media_ref)
                        {
                            results.push(s);
                        }
                    }
                }
            }
        }

        // Include runtime-added media in this library that match the series
        if let Some(ids) = self.modifications.added_by_library.get(&library_id) {
            for id in ids {
                if let Some(media) = self.modifications.added.get(id)
                    && let Some(season) = media.deserialize()?.to_season()
                    && &season.series_id == series_id
                {
                    results.push(season);
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

        // Determine which library this season belongs to
        let &library_id =
            self.media_id_index
                .get(season_uuid)
                .ok_or_else(|| RepositoryError::NotFound {
                    entity_type: "Season".to_string(),
                    id: season_uuid.to_string(),
                })?;

        let mut results: Vec<EpisodeReference> = Vec::new();

        // Access archived data for the library
        let archived_libraries = unsafe {
            rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&self.libraries_buffer)
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
                    if let Some(modified) = self.modifications.get_entry(&media_uuid) {
                        if let Some(ep) = modified.deserialize()?.to_episode() {
                            results.push(ep);
                        } else if let Ok(Media::Episode(e)) = deserialize::<Media, Error>(media_ref)
                        {
                            results.push(e);
                        }
                    } else if let Ok(Media::Episode(e)) = deserialize::<Media, Error>(media_ref) {
                        results.push(e);
                    }
                }
            }
        }

        // Include runtime-added media in this library that match the season
        if let Some(ids) = self.modifications.added_by_library.get(&library_id) {
            for id in ids {
                if let Some(media) = self.modifications.added.get(id)
                    && let Some(ep) = media.deserialize()?.to_episode()
                    && &ep.season_id == season_id
                {
                    results.push(ep);
                }
            }
        }

        // Sort by episode number
        results.sort_by_key(|e| e.episode_number.value());

        Ok(results)
    }
}

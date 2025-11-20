use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ferrex_core::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedLibraryID, ArchivedMedia, ArchivedMediaID,
    LibraryID, Media, MediaID, MediaIDLike, MediaLike, MediaType, SeriesID,
};
use rkyv::{Archived, deserialize, rancor::Error, util::AlignedVec, vec::ArchivedVec};
use uuid::Uuid;
use yoke::Yoke;

use crate::{
    domains::{
        //media::store::MediaChangeEvent,
        ui::{SortBy, SortOrder},
    },
    infrastructure::repository::{RepositoryError, RepositoryResult},
};

use super::{LibraryYoke, MediaYoke, MovieYoke, SeriesYoke};

/// Runtime modifications layer for managing changes during application runtime
/// Resets on application restart
#[derive(Default, Debug)]
pub(super) struct RuntimeModifications {
    /// Added media items during runtime (uuid -> reference)
    pub(super) added: HashMap<Uuid, Media>,
    /// Added items, mapped by owning library UUID (library_uuid -> set of media uuids)
    pub(super) added_by_library: HashMap<Uuid, HashSet<Uuid>>,
    /// Deleted media IDs during runtime
    pub(super) deleted: HashSet<Uuid>,
    /// Modified media items during runtime (for archived items)
    pub(super) modified: HashMap<Uuid, Media>,
}

impl RuntimeModifications {
    pub(super) fn clear(&mut self) {
        self.added.clear();
        self.added_by_library.clear();
        self.deleted.clear();
        self.modified.clear();
    }

    pub(super) fn is_deleted(&self, id: &Uuid) -> bool {
        self.deleted.contains(id)
    }

    pub(super) fn get_modified(&self, id: &Uuid) -> Option<&Media> {
        self.modified.get(id).or_else(|| self.added.get(id))
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
        self.modifications.clear();
        //self.pending_events.clear();
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_internal(&self, id: &impl MediaIDLike) -> RepositoryResult<Media> {
        let uuid = id.as_uuid();
        let mut buf = Uuid::encode_buffer();

        // Check if deleted in runtime
        if self.modifications.is_deleted(&uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: id.to_string_buf(&mut buf),
            });
        }

        // Check runtime modifications first
        if let Some(modified) = self.modifications.get_modified(&uuid) {
            return Ok(modified.clone());
        }

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(&uuid) {
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
        if self.modifications.is_deleted(&uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        // Check runtime modifications first
        /* TODO: Probably need to change the MaybeArchived type to hold a MediaYoke
        if let Some(modified) = self.modifications.get_modified(&uuid) {
            return MediaYoke::attach_to_cart(Arc::clone(&self), |_| {

            })
        } */
        // Look up in archived data using index
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
                        .filter(|m| m.media_type() == media_type)
                        .find(|m| m.archived_media_id().as_uuid() == uuid)
                        .unwrap()
                },
            ));
        } else {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_movie_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<MovieYoke> {
        let uuid = id.as_uuid();
        // Check if deleted in runtime
        if self.modifications.is_deleted(&uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(&uuid) {
            let media_type = id.media_type();

            return Ok(MovieYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data);
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
            ));
        } else {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }
    }

    /// Internal method to get media by ID, checking modifications first
    pub(super) fn get_series_yoke_internal(
        &self,
        id: &impl MediaIDLike,
    ) -> RepositoryResult<SeriesYoke> {
        let uuid = id.as_uuid();
        // Check if deleted in runtime
        if self.modifications.is_deleted(&uuid) {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }

        // Check runtime modifications first
        /* TODO: Probably need to change the MaybeArchived type to hold a MediaYoke
        if let Some(modified) = self.modifications.get_modified(&uuid) {
            return MediaYoke::attach_to_cart(Arc::clone(&self), |_| {

            })
        } */
        // Look up in archived data using index
        if let Some(&library_id) = self.media_id_index.get(&uuid) {
            let media_type = id.media_type();

            return Ok(SeriesYoke::attach_to_cart(
                Arc::clone(&self.libraries_buffer),
                |data: &AlignedVec| unsafe {
                    let archived_libraries =
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data);
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
            ));
        } else {
            return Err(RepositoryError::NotFound {
                entity_type: "media".to_string(),
                id: uuid.to_string(),
            });
        }
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
                        if let Some(modified) = self.modifications.get_modified(&media_id) {
                            results.push(modified.clone());
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
                    results.push(media.clone());
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
                        rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data)
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
        if !self.libraries_index.contains(&library_id) {
            return None;
        }
        Some(LibraryYoke::attach_to_cart(
            Arc::clone(&self.libraries_buffer),
            |data: &AlignedVec| unsafe {
                rkyv::access_unchecked::<ArchivedVec<ArchivedLibrary>>(&data)
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
                    if let Some(modified) = self.modifications.get_modified(&media_id) {
                        results.push(modified.clone());
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
            results.push(media.clone());
        }

        results
    }
}

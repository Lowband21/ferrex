use std::marker::PhantomData;
use std::sync::Arc;

use ferrex_core::{
    ArchivedLibrary, ArchivedLibraryExt, ArchivedMedia, Library, LibraryID, Media,
    MediaDetailsOption, MediaID, MediaIDLike, MediaLike, MediaOps, SeasonID, SeasonLike,
    SeasonReference, SeriesID,
};
use parking_lot::RwLock;
use rkyv::{util::AlignedVec, vec::ArchivedVec};
use uuid::Uuid;
use yoke::Yoke;

use crate::infrastructure::repository::{RepositoryError, RepositoryResult};

use super::{
    EpisodeYoke, LibraryYoke, MediaYoke, MovieYoke, SeasonYoke, SeriesYoke, repository::MediaRepo,
};

/// Marker types for capability roles
#[derive(Debug, Clone, Copy)]
pub struct ReadOnly;
#[derive(Debug, Clone, Copy)]
pub struct ReadWrite;

/// Role traits for capability gating
pub trait ReadCap {}
pub trait WriteCap: ReadCap {}

impl ReadCap for ReadOnly {}
impl ReadCap for ReadWrite {}
impl WriteCap for ReadWrite {}

/// Generic accessor with capability-gated inherent methods
#[derive(Clone, Debug)]
pub struct Accessor<R> {
    repo: Arc<RwLock<Option<MediaRepo>>>,
    _role: PhantomData<R>,
}

impl<R> Accessor<R> {
    pub fn new(repo: Arc<RwLock<Option<MediaRepo>>>) -> Self {
        Self {
            repo,
            _role: PhantomData,
        }
    }

    /// Returns true when the underlying repository has been set
    pub fn is_initialized(&self) -> bool {
        self.repo.read().is_some()
    }

    #[inline]
    fn infallible_with_repo<T>(&self, f: impl FnOnce(&MediaRepo) -> T) -> RepositoryResult<T> {
        let guard = self.repo.read();
        match &*guard {
            Some(repo) => Ok(f(repo)),
            None => Err(RepositoryError::StorageError(
                "Repository not initialized".into(),
            )),
        }
    }

    #[inline]
    fn with_repo<T>(
        &self,
        f: impl FnOnce(&MediaRepo) -> RepositoryResult<T>,
    ) -> RepositoryResult<T> {
        let guard = self.repo.read();
        match &*guard {
            Some(repo) => f(repo),
            None => Err(RepositoryError::StorageError(
                "Repository not initialized".into(),
            )),
        }
    }

    #[inline]
    fn with_repo_mut<T>(
        &self,
        f: impl FnOnce(&mut MediaRepo) -> RepositoryResult<T>,
    ) -> RepositoryResult<T> {
        let mut guard = self.repo.write();
        match &mut *guard {
            Some(repo) => f(repo),
            None => Err(RepositoryError::StorageError(
                "Repository not initialized".into(),
            )),
        }
    }
}

// -------------------------
// Read-only API
// -------------------------
impl<R: ReadCap> Accessor<R> {
    #[inline]
    pub fn with_archived_libraries<T>(
        &self,
        f: impl FnOnce(&ArchivedVec<ArchivedLibrary>) -> T,
    ) -> RepositoryResult<T> {
        let guard = self.repo.read();
        match &*guard {
            Some(repo) => Ok(f(repo.get_archived_libraries_internal())),
            None => Err(RepositoryError::StorageError(
                "Repository not initialized".into(),
            )),
        }
    }

    /// Get a single media item by ID (read-only)
    pub fn get(&self, id: &impl MediaIDLike) -> RepositoryResult<Media> {
        self.with_repo(|repo| repo.get_internal(id))
    }

    /// Get a single media item by ID (read-only)
    pub fn get_media_yoke(&self, id: &impl MediaIDLike) -> RepositoryResult<MediaYoke> {
        self.with_repo(|repo| repo.get_media_yoke_internal(id))
    }
    /// Get a single media item by ID (read-only)
    pub fn get_movie_yoke(&self, id: &impl MediaIDLike) -> RepositoryResult<MovieYoke> {
        self.with_repo(|repo| repo.get_movie_yoke_internal(id))
    }
    /// Get a single media item by ID (read-only)
    pub fn get_series_yoke(&self, id: &impl MediaIDLike) -> RepositoryResult<SeriesYoke> {
        self.with_repo(|repo| repo.get_series_yoke_internal(id))
    }

    pub fn get_season_yoke(&self, id: &impl MediaIDLike) -> RepositoryResult<SeasonYoke> {
        self.with_repo(|repo| repo.get_season_yoke_internal(id))
    }

    pub fn get_episode_yoke(&self, id: &impl MediaIDLike) -> RepositoryResult<EpisodeYoke> {
        self.with_repo(|repo| repo.get_episode_yoke_internal(id))
    }

    /// Get all media from a library
    pub fn get_library_media(&self, library_id: &LibraryID) -> RepositoryResult<Vec<Media>> {
        self.with_repo(|repo| repo.get_library_media_internal(library_id))
    }

    /*
    /// Get all media from a library
    pub fn get_archived_media_by_library(
        &self,
        library_id: &LibraryID,
    ) -> RepositoryResult<Vec<Media>> {
        self.with_repo(|repo| repo.get_archived_media_by_library_internal(library_id))
    } */

    /// Get multiple items by IDs
    pub fn get_batch<I: MediaIDLike>(&self, ids: &[I]) -> RepositoryResult<Vec<Media>> {
        self.with_repo(|repo| ids.iter().map(|id| repo.get_internal(id)).collect())
    }

    /// Get items by positions into the archived library media slice (index-based access)
    pub fn get_by_positions(
        &self,
        library_id: &LibraryID,
        positions: &[u32],
    ) -> RepositoryResult<Vec<Media>> {
        self.with_repo(|repo| {
            let lib_uuid = library_id.as_uuid();
            let yoke = repo.get_archived_library_yoke_internal(&lib_uuid).ok_or(
                RepositoryError::NotFound {
                    entity_type: "Library".into(),
                    id: library_id.to_string(),
                },
            )?;
            let archived = yoke.get();
            let slice = archived.media_as_slice();

            let mut out = Vec::with_capacity(positions.len());
            for &pos in positions {
                let idx = pos as usize;
                if let Some(media_ref) = slice.get(idx) {
                    let owned = rkyv::deserialize::<Media, rkyv::rancor::Error>(media_ref)
                        .map_err(|e| RepositoryError::DeserializationError(e.to_string()))?;
                    out.push(owned);
                }
            }
            Ok(out)
        })
    }

    /// Count total items (excluding deleted)
    pub fn count(&self) -> RepositoryResult<usize> {
        self.with_repo(|repo| {
            let total = repo.media_id_index.len();
            let deleted = repo.modifications.deleted.len();
            let added = repo.modifications.added.len();
            Ok(total - deleted + added)
        })
    }

    /// Get all libraries
    pub fn get_libraries(&self) -> RepositoryResult<Vec<ferrex_core::Library>> {
        self.with_repo(|repo| Ok(repo.get_libraries_internal()))
    }

    /*
    pub fn get_archived_libraries<'a>(
        &self,
    ) -> RepositoryResult<Yoke<&'static ArchivedVec<ferrex_core::ArchivedLibrary>, Arc<AlignedVec>>>
    {
        self.infallible_with_repo(|repo| repo.get_archived_libraries_yoke_internal())
    } */

    pub fn get_archived_libraries<'a>(
        &self,
    ) -> RepositoryResult<Vec<Yoke<&'static ferrex_core::ArchivedLibrary, Arc<AlignedVec>>>> {
        self.infallible_with_repo(|repo| repo.get_archived_libraries_yoke_internal())
    }

    /// Get library count
    pub fn library_count(&self) -> RepositoryResult<usize> {
        self.with_repo(|repo| Ok(repo.libraries_index.len()))
    }

    /// Get library ids
    pub fn libraries_index(&self) -> RepositoryResult<Vec<Uuid>> {
        self.infallible_with_repo(|repo| repo.libraries_index.clone())
    }

    /// Get a specific library by ID
    pub fn get_library(&self, library_id: &LibraryID) -> RepositoryResult<Option<Library>> {
        self.with_repo(|repo| Ok(repo.get_library_internal(library_id)))
    }

    /*
    /// Get a specific library by ID
    pub fn get_archived_library(
        &self,
        library_id: &LibraryID,
    ) -> RepositoryResult<Option<&ArchivedLibrary>> {
        self.infallible_with_repo(|repo| repo.get_archived_library_internal(library_id))
    } */
    /// Get a specific library by ID
    pub fn get_archived_library_yoke(
        &self,
        library_id: &Uuid,
    ) -> RepositoryResult<Option<LibraryYoke>> {
        self.infallible_with_repo(|repo| repo.get_archived_library_yoke_internal(library_id))
    }

    // TODO: Fix these clones
    pub fn get_sorted_index_by_library(
        &self,
        library_id: &LibraryID,
    ) -> RepositoryResult<Vec<Uuid>> {
        self.with_repo(|repo| {
            let lib_uuid = library_id.as_uuid();

            // Determine library type from archived snapshot
            let owned_lib =
                repo.get_library_internal(library_id)
                    .ok_or(RepositoryError::NotFound {
                        entity_type: "Library".to_string(),
                        id: library_id.to_string(),
                    })?;

            // Preferred path: server-provided sorted indices (movies only for now)
            let mut ids = if let Some(vec) = repo
                .sorted_indices
                .as_ref()
                .and_then(|map| map.get(&lib_uuid))
            {
                vec.clone()
            } else {
                // Fallback: compute indices directly from archived library
                let yoke = repo.get_archived_library_yoke_internal(&lib_uuid).ok_or(
                    RepositoryError::NotFound {
                        entity_type: "ArchivedLibrary".to_string(),
                        id: library_id.to_string(),
                    },
                )?;

                let archived = yoke.get();
                let slice = archived.media_as_slice();

                match owned_lib.library_type {
                    ferrex_core::LibraryType::Movies => slice
                        .iter()
                        .filter_map(|m| match m {
                            ArchivedMedia::Movie(movie) => Some(Uuid::from_bytes(movie.id.0)),
                            _ => None,
                        })
                        .collect(),
                    ferrex_core::LibraryType::Series => slice
                        .iter()
                        .filter_map(|m| match m {
                            ArchivedMedia::Series(series) => Some(Uuid::from_bytes(series.id.0)),
                            _ => None,
                        })
                        .collect(),
                }
            };

            // Remove any ids marked deleted in the overlay
            if !repo.modifications.deleted.is_empty() {
                ids.retain(|id| !repo.modifications.deleted.contains(id));
            }

            // Append runtime additions for this library, filtered by library type
            if let Some(additions) = repo.modifications.added_by_library.get(&lib_uuid) {
                let mut new_ids: Vec<Uuid> = additions.iter().copied().collect();
                new_ids.sort();

                for media_id in new_ids {
                    if ids.contains(&media_id) {
                        continue;
                    }

                    let Some(media) = repo.modifications.added.get(&media_id) else {
                        continue;
                    };

                    let matches_library = match (&owned_lib.library_type, media) {
                        (ferrex_core::LibraryType::Movies, Media::Movie(_)) => true,
                        (ferrex_core::LibraryType::Series, Media::Series(_)) => true,
                        _ => false,
                    };

                    if matches_library {
                        ids.push(media_id);
                    }
                }
            }

            Ok(ids)
        })
    }

    /// Get all media from a library
    pub fn get_all_media(&self) -> RepositoryResult<Vec<Media>> {
        self.with_repo(|repo| Ok(repo.get_all_media_internal()))
    }

    /// Get all seasons for a series
    pub fn get_series_seasons(
        &self,
        series_id: &SeriesID,
    ) -> RepositoryResult<Vec<SeasonReference>> {
        self.with_repo(|repo| repo.get_series_seasons_internal(series_id))
    }

    /// Get all episodes for a season
    pub fn get_season_episodes(
        &self,
        season_id: &SeasonID,
    ) -> RepositoryResult<Vec<ferrex_core::EpisodeReference>> {
        self.with_repo(|repo| repo.get_season_episodes_internal(season_id))
    }

    /// Get season episode count (kept from previous UI accessor API)
    pub fn get_season_episode_count(&self, season_id: &SeasonID) -> RepositoryResult<u32> {
        self.with_repo(|repo| {
            // Reuse existing internal get with an owned MediaID
            let media_ref = repo.get_internal(&MediaID::Season(season_id.to_owned()))?;
            let mut buffer = Uuid::encode_buffer();
            match media_ref.to_season() {
                Some(season) => Ok(season.num_episodes()),
                None => Err(RepositoryError::NotFound {
                    entity_type: "Season".into(),
                    id: season_id.to_string_buf(&mut buffer),
                }),
            }
        })
    }
}

// -------------------------
// Write API (runtime overlay)
// -------------------------
impl<R: WriteCap> Accessor<R> {
    /// Add or update a media item (runtime only, resets on restart)
    /// Requires the owning library ID for new items so we can keep the overlay library-centric.
    pub fn upsert(&self, media: Media, library_id: &LibraryID) -> RepositoryResult<()> {
        self.with_repo_mut(|repo| {
            let id = media.media_id().to_uuid();

            // Remove from deleted if it was there
            repo.modifications.deleted.remove(&id);

            if repo.media_id_index.contains_key(&id) {
                // Existing archived item: consider as modified
                repo.modifications.added.remove(&id);
                // Remove from any added_by_library set if it happened to be there
                if let Some(sets) = repo
                    .modifications
                    .added_by_library
                    .get_mut(&library_id.as_uuid())
                {
                    sets.remove(&id);
                    if sets.is_empty() {
                        repo.modifications
                            .added_by_library
                            .remove(&library_id.as_uuid());
                    }
                }
                repo.modifications.modified.insert(id, media);
            } else {
                // New runtime item: track by library
                repo.modifications.added.insert(id, media);
                let lib_uuid = library_id.as_uuid();
                repo.modifications
                    .added_by_library
                    .entry(lib_uuid)
                    .or_default()
                    .insert(id);
            }

            Ok(())
        })
    }

    /// Delete a media item (runtime only, resets on restart)
    pub fn delete(&self, id: &impl MediaIDLike) -> RepositoryResult<()> {
        self.with_repo_mut(|repo| {
            let uuid = id.as_uuid();

            // Mark as deleted and remove from modifications
            repo.modifications.deleted.insert(*uuid);
            repo.modifications.added.remove(&uuid);
            repo.modifications.modified.remove(&uuid);

            // Remove from added_by_library if present
            // If the item was archived, we can compute its library via media_id_index
            if let Some(arch_lib_id) = repo.media_id_index.get(&uuid) {
                let lib_uuid = arch_lib_id;
                if let Some(set) = repo.modifications.added_by_library.get_mut(&lib_uuid) {
                    set.remove(&uuid);
                    if set.is_empty() {
                        repo.modifications.added_by_library.remove(&lib_uuid);
                    }
                }
            } else {
                // Not in archived index, so it was a runtime-added item. Find and remove.
                let mut empty_keys = Vec::new();
                for (lib_uuid, set) in repo.modifications.added_by_library.iter_mut() {
                    if set.remove(&uuid) && set.is_empty() {
                        empty_keys.push(*lib_uuid);
                    }
                }
                for k in empty_keys {
                    repo.modifications.added_by_library.remove(&k);
                }
            }

            Ok(())
        })
    }

    /// Clear all runtime modifications
    pub fn clear_modifications(&self) -> RepositoryResult<()> {
        self.with_repo_mut(|repo| {
            repo.modifications.clear();
            Ok(())
        })
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use ferrex_core::player_prelude::{
    LibraryId, MovieBatchId, MovieID, MovieReferenceBatchResponse,
};
use rkyv::{rancor::Error, util::AlignedVec};
use uuid::Uuid;

use crate::infra::repository::{RepositoryError, RepositoryResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MovieBatchKey {
    pub library_id: LibraryId,
    pub batch_id: MovieBatchId,
}

impl MovieBatchKey {
    pub fn new(library_id: LibraryId, batch_id: MovieBatchId) -> Self {
        Self {
            library_id,
            batch_id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MovieBatchMovieLocator {
    key: MovieBatchKey,
    index: u32,
}

#[derive(Debug, Default)]
pub struct MovieBatchInstallOutcome {
    pub movies_indexed: usize,
    pub movies_replaced_from_runtime_overlay: usize,
    pub movie_ids: Vec<Uuid>,
}

#[derive(Debug, Default)]
pub struct MovieBatchOverlay {
    batches: HashMap<MovieBatchKey, Arc<AlignedVec>>,
    movies_by_id: HashMap<Uuid, MovieBatchMovieLocator>,
    batch_contents: HashMap<MovieBatchKey, Vec<Uuid>>,
}

impl MovieBatchOverlay {
    pub fn clear(&mut self) {
        self.batches.clear();
        self.movies_by_id.clear();
        self.batch_contents.clear();
    }

    pub fn is_movie_indexed(&self, movie_id: &MovieID) -> bool {
        self.movies_by_id.contains_key(movie_id.as_uuid())
    }

    pub fn movie_ids_for_library(&self, library_id: &LibraryId) -> Vec<Uuid> {
        let mut out = Vec::new();
        for (key, ids) in self.batch_contents.iter() {
            if key.library_id == *library_id {
                out.extend_from_slice(ids);
            }
        }
        out
    }

    pub fn all_movie_ids(&self) -> Vec<Uuid> {
        self.movies_by_id.keys().copied().collect()
    }

    pub fn get_movie_locator(
        &self,
        movie_uuid: &Uuid,
    ) -> Option<(Arc<AlignedVec>, u32)> {
        let locator = self.movies_by_id.get(movie_uuid)?;
        let cart = self.batches.get(&locator.key)?;
        Some((Arc::clone(cart), locator.index))
    }

    pub fn upsert_batch(
        &mut self,
        expected_key: MovieBatchKey,
        bytes: AlignedVec,
    ) -> RepositoryResult<Vec<Uuid>> {
        let buffer = Arc::new(bytes);
        let archived = rkyv::access::<
            rkyv::Archived<MovieReferenceBatchResponse>,
            Error,
        >(&buffer)
        .map_err(|e| RepositoryError::DeserializationError(e.to_string()))?;

        let actual_library_id = archived.library_id.as_uuid();
        if actual_library_id != expected_key.library_id.to_uuid() {
            return Err(RepositoryError::UpdateFailed(format!(
                "Movie batch payload library_id mismatch: expected {} got {}",
                expected_key.library_id, actual_library_id
            )));
        }

        if archived.batch_id.0 != expected_key.batch_id.0 {
            return Err(RepositoryError::UpdateFailed(format!(
                "Movie batch payload batch_id mismatch: expected {} got {}",
                expected_key.batch_id, archived.batch_id.0
            )));
        }

        if let Some(old_ids) = self.batch_contents.remove(&expected_key) {
            for id in old_ids {
                if let Some(locator) = self.movies_by_id.get(&id)
                    && locator.key == expected_key
                {
                    self.movies_by_id.remove(&id);
                }
            }
        }

        self.batches.insert(expected_key, Arc::clone(&buffer));

        let mut ids = Vec::with_capacity(archived.movies.len());
        for (idx, movie) in archived.movies.iter().enumerate() {
            let movie_uuid = movie.id.to_uuid();
            ids.push(movie_uuid);
            self.movies_by_id.insert(
                movie_uuid,
                MovieBatchMovieLocator {
                    key: expected_key,
                    index: idx as u32,
                },
            );
        }

        self.batch_contents.insert(expected_key, ids.clone());

        Ok(ids)
    }
}

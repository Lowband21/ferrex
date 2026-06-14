//! Tab state definitions for independent tab management

use crate::domains::ui::view_models::HomeViewModel;
use crate::domains::ui::views::grid::VirtualGridState;
use crate::infra::api_types::{LibraryType, Media};
// no poster-checking helpers needed; core compare_media handles poster-first
use super::HomeFocusState;
use crate::infra::repository::accessor::{Accessor, ReadOnly};
use ferrex_core::player_prelude::{
    ArchivedLibraryExt, ArchivedMedia, ArchivedMediaID, ArchivedModel,
    ArchivedMovieReference, ArchivedSeries, LibraryId, MediaID, MediaIDLike,
    MediaOps, MovieID, SeriesID, SortBy, SortOrder, compare_media,
};
use iced::widget::Id;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// State for an individual tab
#[derive(Debug)]
pub enum TabState {
    /// State for the home tab showing curated content
    Home(Box<HomeTabState>),

    /// State for a library-specific tab
    Library(Box<LibraryTabState>),
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl TabState {
    /// Create a new Home tab state
    pub fn new_all(accessor: Accessor<ReadOnly>) -> Self {
        TabState::Home(Box::new(HomeTabState::new(accessor)))
    }

    /// Create a new Library tab state
    pub fn new_library(
        library_id: LibraryId,
        library_type: LibraryType,
        accessor: Accessor<ReadOnly>,
    ) -> Self {
        TabState::Library(Box::new(LibraryTabState::new(
            library_id,
            library_type,
            accessor,
        )))
    }

    /// Get the grid state if this is a library tab
    pub fn grid_state(&self) -> Option<&VirtualGridState> {
        match self {
            TabState::Library(state) => Some(&state.grid_state),
            TabState::Home(_) => None,
        }
    }

    /// Get mutable grid state if this is a library tab
    pub fn grid_state_mut(&mut self) -> Option<&mut VirtualGridState> {
        match self {
            TabState::Library(state) => Some(&mut state.grid_state),
            TabState::Home(_) => None,
        }
    }

    /// Get the currently visible media items for this tab
    pub fn get_visible_items(&self) -> Vec<ArchivedMediaID> {
        match self {
            TabState::Library(state) => state.get_visible_items(),
            TabState::Home(_) => {
                // Home  tab uses carousel view, not virtual grid
                // Return empty for now - could be extended to return carousel visible items
                Vec::new()
            }
        }
    }

    /// Get the currently prefetch media items for this tab
    pub fn get_prefetch_items(&self) -> Vec<ArchivedMediaID> {
        match self {
            TabState::Library(state) => state.get_preload_items(),
            TabState::Home(_) => {
                // Home  tab uses carousel view, not virtual grid
                // Return empty for now - could be extended to return carousel visible items
                Vec::new()
            }
        }
    }

    /// Get the visible positions for this tab (movie libraries). Returns None if not a library tab.
    pub fn get_visible_positions(&self) -> Option<(LibraryId, Vec<u32>)> {
        match self {
            TabState::Library(state) => Some(state.get_visible_positions()),
            _ => None,
        }
    }

    pub fn set_sort(&mut self, sort_by: SortBy, sort_order: SortOrder) {
        if let TabState::Library(state) = self {
            state.set_sort(sort_by, sort_order);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LibraryTabState;
    use crate::domains::ui::views::grid::VirtualGridState;
    use crate::infra::api_types::LibraryType;
    use crate::infra::repository::accessor::{Accessor, ReadOnly};
    use crate::infra::repository::media_repo::MediaRepo;
    use ferrex_core::player_prelude::{LibraryId, SortBy, SortOrder};
    use iced::widget::Id;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Arc;
    use uuid::Uuid;

    #[test]
    fn reconcile_positions_skips_missing_entries() {
        let positions = vec![0, 1, 2];
        let uuid_a = Uuid::from_u128(1);
        let uuid_c = Uuid::from_u128(3);
        let data = [Some(uuid_a), None, Some(uuid_c)];

        let (indices, ids) = LibraryTabState::reconcile_positions(
            &positions,
            |idx| data.get(idx).and_then(|opt| *opt),
            None,
        );

        assert_eq!(indices, vec![0usize, 2usize]);
        assert_eq!(ids, vec![uuid_a, uuid_c]);
    }

    #[test]
    fn reconcile_positions_respects_allowed_set() {
        let positions = vec![2, 0, 1];
        let uuid_a = Uuid::from_u128(10);
        let uuid_b = Uuid::from_u128(11);
        let uuid_c = Uuid::from_u128(12);
        let data = [Some(uuid_a), Some(uuid_b), Some(uuid_c)];

        let allowed: HashSet<Uuid> = [uuid_c, uuid_a].into_iter().collect();

        let (indices, ids) = LibraryTabState::reconcile_positions(
            &positions,
            |idx| data.get(idx).and_then(|opt| *opt),
            Some(&allowed),
        );

        assert_eq!(indices, vec![2usize, 0usize]);
        assert_eq!(ids, vec![uuid_c, uuid_a]);
    }

    #[test]
    fn refresh_from_repo_preserves_grid_when_repo_uninitialized() {
        let repo: Arc<RwLock<Option<MediaRepo>>> = Arc::new(RwLock::new(None));
        let accessor: Accessor<ReadOnly> = Accessor::new(repo);

        let cached_ids = vec![Uuid::from_u128(1), Uuid::from_u128(2)];
        let cached_set: HashSet<Uuid> = cached_ids.iter().copied().collect();

        let mut tab = LibraryTabState {
            library_id: LibraryId(Uuid::from_u128(42)),
            library_type: LibraryType::Movies,
            grid_state: VirtualGridState::with_id(2, 5, 100.0, Id::unique()),
            cached_index_ids: cached_ids.clone(),
            cached_index_set: cached_set,
            cached_positions: Some(vec![0, 1]),
            cached_filter_positions: HashMap::new(),
            filtered_indices: Some(vec![0, 1]),
            cached_media: super::CachedMedia::Movies(Vec::new()),
            needs_refresh: true,
            navigation_history: Vec::new(),
            accessor,
            sort_by: SortBy::Title,
            sort_order: SortOrder::Ascending,
        };

        tab.refresh_from_repo();

        assert_eq!(tab.cached_index_ids, cached_ids);
        assert!(tab.needs_refresh);
        assert_eq!(tab.grid_state.total_items, 2);
        assert!(tab.cached_positions.is_some());
        assert!(tab.filtered_indices.is_some());
    }

    #[test]
    fn refresh_from_repo_preserves_grid_when_repo_returns_empty_index() {
        let repo: Arc<RwLock<Option<MediaRepo>>> =
            Arc::new(RwLock::new(Some(MediaRepo::new_empty())));
        let accessor: Accessor<ReadOnly> = Accessor::new(repo);

        let cached_ids = vec![Uuid::from_u128(10), Uuid::from_u128(11)];
        let cached_set: HashSet<Uuid> = cached_ids.iter().copied().collect();

        let mut tab = LibraryTabState {
            library_id: LibraryId(Uuid::from_u128(7)),
            library_type: LibraryType::Movies,
            grid_state: VirtualGridState::with_id(2, 5, 100.0, Id::unique()),
            cached_index_ids: cached_ids.clone(),
            cached_index_set: cached_set,
            cached_positions: None,
            cached_filter_positions: HashMap::new(),
            filtered_indices: None,
            cached_media: super::CachedMedia::Movies(Vec::new()),
            needs_refresh: true,
            navigation_history: Vec::new(),
            accessor,
            sort_by: SortBy::Title,
            sort_order: SortOrder::Ascending,
        };

        tab.refresh_from_repo();

        assert_eq!(tab.cached_index_ids, cached_ids);
        assert!(tab.needs_refresh);
        assert_eq!(tab.grid_state.total_items, 2);
    }

    fn stub_movie_details(
        id: u64,
        title: &str,
    ) -> ferrex_model::EnhancedMovieDetails {
        use ferrex_model::details::ExternalIds;
        use ferrex_model::image::metadata::MediaImages;
        ferrex_model::EnhancedMovieDetails {
            id,
            title: title.to_string(),
            original_title: None,
            overview: None,
            release_date: None,
            runtime: None,
            vote_average: None,
            vote_count: None,
            popularity: None,
            content_rating: None,
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: Vec::new(),
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: None,
            tagline: None,
            budget: None,
            revenue: None,
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            primary_poster_iid: None,
            primary_backdrop_iid: None,
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            collection: None,
            recommendations: Vec::new(),
            similar: Vec::new(),
        }
    }

    #[test]
    fn apply_sorted_positions_does_not_require_archived_media_slice() {
        use crate::infra::repository::accessor::{Accessor, ReadWrite};
        use ferrex_core::player_prelude::{
            Library, LibraryType as ModelLibraryType, MediaID, MovieBatchId,
            MovieReference, MovieReferenceBatchResponse,
        };
        use ferrex_model::MovieReferenceBatchSize;
        use ferrex_model::titles::MovieTitle;
        use ferrex_model::urls::MovieURL;
        use rkyv::rancor::Error as RkyvError;
        use std::path::PathBuf;

        let library_uuid = Uuid::from_u128(100);
        let library_id = LibraryId(library_uuid);

        // Build a repo where the archived library exists but has no `media` slice.
        // Movie references are provided via the movie-batches overlay.
        let library = Library {
            id: library_id,
            name: "Test Library".to_string(),
            library_type: ModelLibraryType::Movies,
            paths: Vec::new(),
            scan_interval_minutes: 60,
            last_scan: None,
            enabled: true,
            auto_scan: false,
            watch_for_changes: false,
            analyze_on_scan: false,
            max_retry_attempts: 3,
            movie_ref_batch_size: MovieReferenceBatchSize::default(),
            created_at: ferrex_model::chrono::Utc::now(),
            updated_at: ferrex_model::chrono::Utc::now(),
            media: None,
        };

        let libraries_bytes =
            rkyv::to_bytes::<RkyvError>(&vec![library]).expect("serialize");
        let repo = MediaRepo::new(libraries_bytes).expect("init repo");
        let repo: Arc<RwLock<Option<MediaRepo>>> =
            Arc::new(RwLock::new(Some(repo)));

        let accessor_rw: Accessor<ReadWrite> = Accessor::new(Arc::clone(&repo));

        let batch_id = MovieBatchId::new(1).expect("batch id");
        let movie_a_uuid = Uuid::from_u128(200);
        let movie_b_uuid = Uuid::from_u128(201);
        let movie_a_id = ferrex_core::player_prelude::MovieID(movie_a_uuid);
        let movie_b_id = ferrex_core::player_prelude::MovieID(movie_b_uuid);

        let movie_a = MovieReference {
            id: movie_a_id,
            library_id,
            batch_id: Some(batch_id),
            tmdb_id: 1,
            title: MovieTitle::from("A Movie"),
            details: stub_movie_details(1, "A Movie"),
            endpoint: MovieURL::from("/media/a".to_string()),
            file: ferrex_model::MediaFile {
                id: Uuid::from_u128(300),
                media_id: MediaID::Movie(movie_a_id),
                path: PathBuf::from("/tmp/a.mkv"),
                filename: "a.mkv".to_string(),
                size: 1,
                discovered_at: ferrex_model::chrono::Utc::now(),
                created_at: ferrex_model::chrono::Utc::now(),
                media_file_metadata: None,
                library_id,
            },
            theme_color: None,
        };

        let movie_b = MovieReference {
            id: movie_b_id,
            library_id,
            batch_id: Some(batch_id),
            tmdb_id: 2,
            title: MovieTitle::from("B Movie"),
            details: stub_movie_details(2, "B Movie"),
            endpoint: MovieURL::from("/media/b".to_string()),
            file: ferrex_model::MediaFile {
                id: Uuid::from_u128(301),
                media_id: MediaID::Movie(movie_b_id),
                path: PathBuf::from("/tmp/b.mkv"),
                filename: "b.mkv".to_string(),
                size: 2,
                discovered_at: ferrex_model::chrono::Utc::now(),
                created_at: ferrex_model::chrono::Utc::now(),
                media_file_metadata: None,
                library_id,
            },
            theme_color: None,
        };

        let batch = MovieReferenceBatchResponse {
            library_id,
            batch_id,
            movies: vec![movie_a, movie_b],
        };
        let batch_bytes =
            rkyv::to_bytes::<RkyvError>(&batch).expect("serialize batch");
        accessor_rw
            .install_movie_reference_batch(library_id, batch_id, batch_bytes)
            .expect("install movie batch");

        let accessor_ro: Accessor<ReadOnly> = Accessor::new(repo);
        let mut tab =
            LibraryTabState::new(library_id, LibraryType::Movies, accessor_ro);

        // Sanity check: base refresh populates canonical title ordering.
        assert_eq!(tab.cached_index_ids, vec![movie_a_uuid, movie_b_uuid]);
        assert_eq!(tab.grid_state.total_items, 2);

        // Apply server positions that reorder the canonical title index.
        tab.apply_sorted_positions(&[1, 0], Some(42));

        assert_eq!(tab.cached_index_ids, vec![movie_b_uuid, movie_a_uuid]);
        assert_eq!(tab.grid_state.total_items, 2);
    }
}

/// State for the "Home" tab showing curated content
#[derive(Debug)]
pub struct HomeTabState {
    /// The Home view model (existing implementation)
    pub view_model: HomeViewModel,

    /// Navigation history specific to this tab
    pub navigation_history: Vec<String>,

    /// Curated: combined continue watching across movies and series
    pub continue_watching: Vec<uuid::Uuid>,
    /// Curated: recently added movies (by date added desc)
    pub recent_movies: Vec<uuid::Uuid>,
    /// Curated: recently added series (by date added desc)
    pub recent_series: Vec<uuid::Uuid>,
    /// Curated: recently released movies (by release date desc)
    pub released_movies: Vec<uuid::Uuid>,
    /// Curated: recently released series (by release/first_air date desc)
    pub released_series: Vec<uuid::Uuid>,

    /// Focus and vertical scroll animation state for the Home view
    pub focus: HomeFocusState,
}

impl HomeTabState {
    /// Create a new Home tab state
    pub fn new(accessor: Accessor<ReadOnly>) -> Self {
        Self {
            view_model: HomeViewModel::new(accessor),
            navigation_history: Vec::new(),
            continue_watching: Vec::new(),
            recent_movies: Vec::new(),
            recent_series: Vec::new(),
            released_movies: Vec::new(),
            released_series: Vec::new(),
            focus: HomeFocusState::new(),
        }
    }

    // Set the repo accessor
    //pub fn set_repo_accessor(&mut self, accessor: Option<&UIMediaAccessor>) {
    //    self.view_model.set_repo_accessor(accessor);
    //}
}

/// State for a library-specific tab
#[derive(Debug)]
pub struct LibraryTabState {
    /// The library ID this tab represents
    pub library_id: LibraryId,

    /// The type of library (Movies or TvShows)
    pub library_type: LibraryType,

    /// Virtual grid state for this specific tab
    pub grid_state: VirtualGridState,

    /// Cached sorted index of visible top-level items (movie/series) for this library
    pub cached_index_ids: Vec<Uuid>,

    /// Fast membership check for `cached_index_ids` (kept in sync on refresh/insert)
    cached_index_set: HashSet<Uuid>,

    /// Cached server-provided positions into archived slice (movies only, Phase 1)
    pub cached_positions: Option<Vec<u32>>,

    /// Cache of server-provided position sets keyed by filter specification hash
    cached_filter_positions: HashMap<u64, Vec<u32>>,

    /// Cached mapping of the currently active filtered/sorted positions into archived slice indices
    filtered_indices: Option<Vec<usize>>,

    /// Cached media items for this library
    /// This is an enum to support both movie and TV libraries
    pub cached_media: CachedMedia,

    /// Whether this tab needs refresh
    pub needs_refresh: bool,

    /// Navigation history specific to this tab
    pub navigation_history: Vec<String>,

    /// Reference to the media repo accessor for data access
    accessor: Accessor<ReadOnly>,

    /// Current sort field for this tab
    sort_by: SortBy,

    /// Current sort order for this tab
    sort_order: SortOrder,
}

/// Cached media items for a library tab - using archived references for zero-copy access
#[derive(Debug)]
pub enum CachedMedia {
    /// Cached movies for a movie library (archived references)
    Movies(Vec<&'static ArchivedMovieReference>),

    /// Cached TV series for a TV library (archived references)
    TvShows(Vec<&'static ArchivedSeries>),
}

impl CachedMedia {
    /// Get the count of cached items
    pub fn len(&self) -> usize {
        match self {
            CachedMedia::Movies(movies) => movies.len(),
            CachedMedia::TvShows(shows) => shows.len(),
        }
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl LibraryTabState {
    /// Create a new library tab state
    pub fn new(
        library_id: LibraryId,
        library_type: LibraryType,
        accessor: Accessor<ReadOnly>,
    ) -> Self {
        let cached_media = match library_type {
            LibraryType::Movies => CachedMedia::Movies(Vec::new()),
            LibraryType::Series => CachedMedia::TvShows(Vec::new()),
        };

        let scrollable_id = Id::from(format!("library-{}", library_id));
        let grid_state = VirtualGridState::with_id(
            0,
            5,
            crate::infra::constants::virtual_grid::ROW_HEIGHT,
            scrollable_id,
        );

        let mut state = Self {
            library_id,
            library_type,
            grid_state,
            cached_index_ids: Vec::new(),
            cached_index_set: HashSet::new(),
            cached_positions: None,
            cached_filter_positions: HashMap::new(),
            filtered_indices: None,
            cached_media,
            needs_refresh: true,
            navigation_history: Vec::new(),
            accessor,
            sort_by: SortBy::Title,
            sort_order: SortOrder::Ascending,
        };

        state.refresh_from_repo();

        state
    }

    /// Apply server-provided sorted positions to reorder the current grid
    pub fn apply_sorted_positions(
        &mut self,
        positions: &[u32],
        cache_key: Option<u64>,
    ) {
        self.cached_positions = Some(positions.to_vec());
        if let Some(hash) = cache_key {
            self.cached_filter_positions
                .insert(hash, positions.to_vec());
        }

        if !matches!(self.library_type, LibraryType::Movies) {
            self.filtered_indices = None;
            self.grid_state.total_items = self.cached_index_ids.len();
            self.grid_state.calculate_visible_range();
            return;
        }

        if !self.accessor.is_initialized() {
            // Repo not ready; keep existing grid visible and retry when the repo
            // becomes available (the cached positions are retained above).
            log::debug!(
                "Skipping ApplyFilteredPositions for library {}: repo not initialized",
                self.library_id
            );
            return;
        }

        // Server positions are indices into a canonical client-side movie ordering
        // (currently title-sorted). Do not derive UUIDs from the archived library
        // slice: the archive can exist without a hydrated `media` slice while movie
        // batches are loaded, which would incorrectly clear the grid.
        let title_index = match self.accessor.get_sorted_index_by_library(
            &self.library_id,
            SortBy::Title,
            SortOrder::Ascending,
        ) {
            Ok(index) => index,
            Err(err) => {
                log::warn!(
                    "ApplyFilteredPositions: failed to build title index for library {}; preserving existing grid: {}",
                    self.library_id,
                    err
                );
                return;
            }
        };

        let (filtered_indices, ids) = Self::reconcile_positions(
            positions,
            |idx| title_index.get(idx).copied(),
            None,
        );

        // Treat a non-empty response that maps to zero items as transient/mismatched.
        // Preserve the existing grid rather than clearing it.
        if ids.is_empty()
            && !positions.is_empty()
            && !self.cached_index_ids.is_empty()
        {
            log::warn!(
                "ApplyFilteredPositions: positions mapped to 0 items (positions={}, title_index={}); preserving existing grid ({} items)",
                positions.len(),
                title_index.len(),
                self.cached_index_ids.len()
            );
            return;
        }

        self.filtered_indices = Some(filtered_indices);
        self.cached_index_set = ids.iter().copied().collect();
        self.cached_index_ids = ids;
        self.grid_state.total_items = self.cached_index_ids.len();
        self.constrain_scroll_after_update();
        self.grid_state.calculate_visible_range();
        self.needs_refresh = false;
    }

    fn constrain_scroll_after_update(&mut self) {
        if self.grid_state.total_items == 0 || self.grid_state.columns == 0 {
            self.grid_state.scroll_position = 0.0;
            return;
        }

        let total_rows = self
            .grid_state
            .total_items
            .div_ceil(self.grid_state.columns);
        let content_height = total_rows as f32 * self.grid_state.row_height;
        let max_scroll = if content_height > self.grid_state.viewport_height {
            content_height - self.grid_state.viewport_height
        } else {
            0.0
        };

        if self.grid_state.scroll_position > max_scroll {
            self.grid_state.scroll_position = max_scroll;
        }
    }

    fn reconcile_positions<F>(
        positions: &[u32],
        mut fetch_uuid: F,
        allowed: Option<&HashSet<Uuid>>,
    ) -> (Vec<usize>, Vec<Uuid>)
    where
        F: FnMut(usize) -> Option<Uuid>,
    {
        let mut filtered_indices = Vec::with_capacity(positions.len());
        let mut ids = Vec::with_capacity(positions.len());
        let mut seen = HashSet::new();

        for &pos in positions {
            let idx = pos as usize;
            if let Some(uuid) = fetch_uuid(idx)
                && allowed.is_none_or(|set| set.contains(&uuid))
                && seen.insert(uuid)
            {
                filtered_indices.push(idx);
                ids.push(uuid);
            }
        }

        (filtered_indices, ids)
    }

    /// Refresh cached media from the repo
    pub fn refresh_from_repo(&mut self) {
        if !self.needs_refresh {
            return;
        }

        if !self.accessor.is_initialized() {
            // The repo may be temporarily unavailable (e.g., during bootstrap).
            // Preserve the existing grid rather than clearing it.
            return;
        }

        let ids = match self.accessor.get_sorted_index_by_library(
            &self.library_id,
            self.sort_by,
            self.sort_order,
        ) {
            Ok(ids) => ids,
            Err(err) => {
                // Treat repository read failures as transient: keep the existing
                // cache so the UI doesn't "blink" empty, and retry on the next
                // refresh opportunity.
                log::warn!(
                    "Failed to refresh library {} from repo; preserving existing grid: {}",
                    self.library_id,
                    err
                );
                return;
            }
        };

        // A sudden "empty" refresh while we already have items is almost always a
        // transient state (e.g., cache swap during scans/bootstrap). Avoid
        // replacing a populated grid with an empty one; retry instead.
        if ids.is_empty() && !self.cached_index_ids.is_empty() {
            log::warn!(
                "Refresh for library {} returned 0 items while grid was populated ({}); preserving existing grid",
                self.library_id,
                self.cached_index_ids.len()
            );
            return;
        }

        // Refresh succeeded and is non-destructive: apply the new ordering.
        self.cached_index_set = ids.iter().copied().collect();
        self.cached_index_ids = ids;
        self.grid_state.total_items = self.cached_index_ids.len();
        self.constrain_scroll_after_update();
        self.grid_state.calculate_visible_range();

        // Invalidate server-provided filtered positions; they're keyed to the
        // previous cached index ordering and must be re-fetched/re-applied after
        // the base ordering changes.
        self.cached_positions = None;
        self.cached_filter_positions.clear();
        self.filtered_indices = None;

        self.cached_media = match self.library_type {
            LibraryType::Movies => CachedMedia::Movies(Vec::new()),
            LibraryType::Series => CachedMedia::TvShows(Vec::new()),
        };

        self.needs_refresh = false;
    }

    fn compare_media_with_fallback(
        &self,
        a: &Media,
        a_id: &Uuid,
        b: &Media,
        b_id: &Uuid,
    ) -> Ordering {
        match compare_media(a, b, self.sort_by, self.sort_order) {
            Some(ord) if ord != Ordering::Equal => ord,
            Some(_) | None => {
                let fallback =
                    compare_media(a, b, SortBy::Title, SortOrder::Ascending);
                match fallback {
                    Some(ord) if ord != Ordering::Equal => ord,
                    Some(_) | None => a_id.cmp(b_id),
                }
            }
        }
    }

    fn media_id_for_uuid(&self, id: Uuid) -> MediaID {
        match self.library_type {
            LibraryType::Movies => MediaID::Movie(MovieID(id)),
            LibraryType::Series => MediaID::Series(SeriesID(id)),
        }
    }

    fn lower_bound_insert_position(
        &self,
        media: &Media,
        media_uuid: Uuid,
    ) -> Option<usize> {
        let mut left = 0usize;
        let mut right = self.cached_index_ids.len();

        while left < right {
            let mid = left + ((right - left) / 2);
            let existing_id = self.cached_index_ids[mid];

            let existing_media = match self.library_type {
                LibraryType::Movies => {
                    let existing_lookup_id = MovieID(existing_id);
                    let existing_yoke = match self
                        .accessor
                        .get_movie_yoke(&MediaID::Movie(existing_lookup_id))
                    {
                        Ok(yoke) => yoke,
                        Err(err) => {
                            log::warn!(
                                "Failed to fetch movie yoke {} while inserting SSE addition: {}",
                                existing_lookup_id,
                                err
                            );
                            return None;
                        }
                    };
                    let movie = match existing_yoke.get().try_to_model() {
                        Ok(m) => m,
                        Err(err) => {
                            log::warn!(
                                "Failed to deserialize movie {} while inserting SSE addition: {}",
                                existing_lookup_id,
                                err
                            );
                            return None;
                        }
                    };
                    Media::Movie(Box::new(movie))
                }
                LibraryType::Series => {
                    let existing_lookup_id =
                        self.media_id_for_uuid(existing_id);
                    let existing_yoke = match self
                        .accessor
                        .get_media_yoke(&existing_lookup_id)
                    {
                        Ok(yoke) => yoke,
                        Err(err) => {
                            log::warn!(
                                "Failed to fetch media yoke {} while inserting SSE addition: {}",
                                existing_lookup_id,
                                err
                            );
                            return None;
                        }
                    };
                    match existing_yoke.get().try_to_model() {
                        Ok(m) => m,
                        Err(err) => {
                            log::warn!(
                                "Failed to deserialize media {} while inserting SSE addition: {}",
                                existing_lookup_id,
                                err
                            );
                            return None;
                        }
                    }
                }
            };

            let ord = self.compare_media_with_fallback(
                &existing_media,
                &existing_id,
                media,
                &media_uuid,
            );

            if ord == Ordering::Less {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        Some(left)
    }

    /// Insert a newly discovered media item into the cached ordering based on the
    /// current sort configuration. Returns true if the media was inserted.
    pub fn insert_media_reference(&mut self, media: &Media) -> bool {
        if !self.matches_library_media(media) {
            return false;
        }

        if self.needs_refresh {
            // If the cached ordering is stale, prefer the full refresh path rather than
            // attempting to maintain a potentially inconsistent local ordering.
            return false;
        }

        let media_uuid = media.media_id().to_uuid();

        if self.cached_index_set.contains(&media_uuid) {
            return false;
        }

        let Some(position) =
            self.lower_bound_insert_position(media, media_uuid)
        else {
            self.mark_needs_refresh();
            return false;
        };

        self.cached_index_ids.insert(position, media_uuid);
        self.cached_index_set.insert(media_uuid);

        self.grid_state.total_items = self.cached_index_ids.len();
        self.grid_state.calculate_visible_range();

        true
    }

    fn matches_library_media(&self, media: &Media) -> bool {
        matches!(
            (self.library_type, media),
            (LibraryType::Movies, Media::Movie(_))
                | (LibraryType::Series, Media::Series(_))
        )
    }

    pub fn set_sort(&mut self, sort_by: SortBy, sort_order: SortOrder) {
        if self.sort_by != sort_by || self.sort_order != sort_order {
            self.sort_by = sort_by;
            self.sort_order = sort_order;
            self.mark_needs_refresh();
        }
    }

    /// Mark this tab as needing refresh
    pub fn mark_needs_refresh(&mut self) {
        self.needs_refresh = true;
    }

    pub fn cached_positions_for_hash(&self, hash: u64) -> Option<&Vec<u32>> {
        self.cached_filter_positions.get(&hash)
    }

    /// Update the grid state's scroll position
    pub fn update_scroll(
        &mut self,
        viewport: iced::widget::scrollable::Viewport,
    ) {
        self.grid_state.update_scroll(viewport);
    }

    /// Get movies if this is a movie library (archived references)
    pub fn movies(&self) -> Option<&[&'static ArchivedMovieReference]> {
        match &self.cached_media {
            CachedMedia::Movies(movies) => Some(movies),
            _ => None,
        }
    }

    /// Get TV shows if this is a TV library (archived references)
    pub fn tv_shows(&self) -> Option<&[&'static ArchivedSeries]> {
        match &self.cached_media {
            CachedMedia::TvShows(shows) => Some(shows),
            _ => None,
        }
    }

    /// Compute visible positions for the archived slice (Phase 1, movies: filter top-level by library type)
    pub fn get_visible_positions(&self) -> (LibraryId, Vec<u32>) {
        let range = self.grid_state.visible_range.clone();
        // Positions are simply the indices inside the filtered top-level slice range
        // Align with how get_visible_items filters by media type
        let lib_uuid = self.library_id;
        (lib_uuid, (range.start as u32..range.end as u32).collect())
    }

    /// Get the currently visible media items based on the grid's visible range
    pub fn get_visible_items(&self) -> Vec<ArchivedMediaID> {
        let range = self.grid_state.visible_range.clone();

        if !self.accessor.is_initialized() {
            return Vec::new();
        }

        let lib_uuid = *self.library_id.as_uuid();
        let yoke_opt = self
            .accessor
            .get_archived_library_yoke(&lib_uuid)
            .ok()
            .flatten();
        let Some(yoke) = yoke_opt else {
            return Vec::new();
        };
        let lib = yoke.get();
        let slice = lib.media_as_slice();

        // If we have server-provided positions (movies Phase 1), use them to compute visible IDs
        if matches!(self.library_type, LibraryType::Movies)
            && let Some(indices) = &self.filtered_indices
        {
            let visible = indices.get(range.clone()).unwrap_or(&[]);
            return visible
                .iter()
                .filter_map(|idx| slice.get(*idx))
                .map(|m| m.id())
                .collect();
        }

        // Fallback: filter top-level media according to library type and slice by visible range
        let filtered: Vec<&ArchivedMedia> = match self.library_type {
            LibraryType::Movies => slice
                .iter()
                .filter(|m| matches!(m, ArchivedMedia::Movie(_)))
                .collect(),
            LibraryType::Series => slice
                .iter()
                .filter(|m| matches!(m, ArchivedMedia::Series(_)))
                .collect(),
        };

        filtered
            .get(range)
            .unwrap_or(&[])
            .iter()
            .map(|m| m.id())
            .collect()
    }

    /// Get the currently visible media items based on the grid's visible range
    pub fn get_preload_items(&self) -> Vec<ArchivedMediaID> {
        let start_bound = self.grid_state.visible_range.start;
        let end_bound = self.grid_state.visible_range.end;
        let preload_above_range =
            self.grid_state.overscan_rows_above..start_bound;
        let preload_below_range =
            end_bound..self.grid_state.overscan_rows_below;

        if !self.accessor.is_initialized() {
            return Vec::new();
        }

        let lib_uuid = *self.library_id.as_uuid();
        let yoke_opt = self
            .accessor
            .get_archived_library_yoke(&lib_uuid)
            .ok()
            .flatten();
        let Some(yoke) = yoke_opt else {
            return Vec::new();
        };
        let lib = yoke.get();
        let slice = lib.media_as_slice();

        let filtered: Vec<&ArchivedMedia> = match self.library_type {
            LibraryType::Movies => slice
                .iter()
                .filter(|m| matches!(m, ArchivedMedia::Movie(_)))
                .collect(),
            LibraryType::Series => slice
                .iter()
                .filter(|m| matches!(m, ArchivedMedia::Series(_)))
                .collect(),
        };

        let mut preload: Vec<ArchivedMediaID> = filtered
            .get(preload_above_range)
            .unwrap_or(&[])
            .iter()
            .map(|m| m.id())
            .collect();

        preload.extend(
            filtered
                .get(preload_below_range)
                .unwrap_or(&[])
                .iter()
                .map(|m| m.id())
                .collect::<Vec<ArchivedMediaID>>(),
        );

        preload
    }
}

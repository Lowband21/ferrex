//! Tab state definitions for independent tab management

use crate::domains::ui::view_models::AllViewModel;
use crate::domains::ui::views::grid::VirtualGridState;
use crate::infrastructure::api_types::{LibraryType, Media};
use crate::infrastructure::repository::accessor::{Accessor, ReadOnly};
use ferrex_core::player_prelude::{
    ArchivedLibraryExt, ArchivedMedia, ArchivedMediaID, ArchivedMovieReference,
    ArchivedSeriesReference, LibraryID, MediaID, MediaIDLike, MediaOps,
    MovieID, SeriesID, SortBy, SortOrder, compare_media,
};
use iced::widget::Id;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// State for an individual tab
#[derive(Debug)]
pub enum TabState {
    /// State for the "All" tab showing curated content
    All(AllTabState),

    /// State for a library-specific tab
    Library(LibraryTabState),
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
    /// Create a new All tab state
    pub fn new_all(accessor: Accessor<ReadOnly>) -> Self {
        TabState::All(AllTabState::new(accessor))
    }

    /// Create a new Library tab state
    pub fn new_library(
        library_id: LibraryID,
        library_type: LibraryType,
        accessor: Accessor<ReadOnly>,
    ) -> Self {
        TabState::Library(LibraryTabState::new(
            library_id,
            library_type,
            accessor,
        ))
    }

    /// Get the grid state if this is a library tab
    pub fn grid_state(&self) -> Option<&VirtualGridState> {
        match self {
            TabState::Library(state) => Some(&state.grid_state),
            TabState::All(_) => None,
        }
    }

    /// Get mutable grid state if this is a library tab
    pub fn grid_state_mut(&mut self) -> Option<&mut VirtualGridState> {
        match self {
            TabState::Library(state) => Some(&mut state.grid_state),
            TabState::All(_) => None,
        }
    }

    /// Get the currently visible media items for this tab
    pub fn get_visible_items(&self) -> Vec<ArchivedMediaID> {
        match self {
            TabState::Library(state) => state.get_visible_items(),
            TabState::All(_) => {
                // All tab uses carousel view, not virtual grid
                // Return empty for now - could be extended to return carousel visible items
                Vec::new()
            }
        }
    }

    /// Get the visible positions for this tab (movie libraries). Returns None if not a library tab.
    pub fn get_visible_positions(&self) -> Option<(LibraryID, Vec<u32>)> {
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
    use std::collections::HashSet;
    use uuid::Uuid;

    #[test]
    fn reconcile_positions_skips_missing_entries() {
        let positions = vec![0, 1, 2];
        let uuid_a = Uuid::from_u128(1);
        let uuid_c = Uuid::from_u128(3);
        let data = [Some(uuid_a), None, Some(uuid_c)];

        let (indices, ids) = LibraryTabState::reconcile_positions(
            &positions,
            |idx| data.get(idx).and_then(|opt| opt.clone()),
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
            |idx| data.get(idx).and_then(|opt| opt.clone()),
            Some(&allowed),
        );

        assert_eq!(indices, vec![2usize, 0usize]);
        assert_eq!(ids, vec![uuid_c, uuid_a]);
    }
}

impl LibraryTabState {
    fn extract_media_uuid(media: &ArchivedMedia) -> Option<Uuid> {
        match media {
            ArchivedMedia::Movie(movie) => Some(movie.id.0),
            ArchivedMedia::Series(series) => Some(series.id.0),
            _ => None,
        }
    }
}

/// State for the "All" tab showing curated content
#[derive(Debug)]
pub struct AllTabState {
    /// The All view model (existing implementation)
    pub view_model: AllViewModel,

    /// Navigation history specific to this tab
    pub navigation_history: Vec<String>,
}

impl AllTabState {
    /// Create a new All tab state
    pub fn new(accessor: Accessor<ReadOnly>) -> Self {
        Self {
            view_model: AllViewModel::new(accessor),
            navigation_history: Vec::new(),
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
    pub library_id: LibraryID,

    /// The type of library (Movies or TvShows)
    pub library_type: LibraryType,

    /// Virtual grid state for this specific tab
    pub grid_state: VirtualGridState,

    /// Cached sorted index of visible top-level items (movie/series) for this library
    pub cached_index_ids: Vec<Uuid>,

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
    TvShows(Vec<&'static ArchivedSeriesReference>),
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
        library_id: LibraryID,
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
            crate::infrastructure::constants::virtual_grid::ROW_HEIGHT,
            scrollable_id,
        );

        let mut state = Self {
            library_id,
            library_type,
            grid_state,
            cached_index_ids: Vec::new(),
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
            self.filtered_indices = Some(Vec::new());
            self.cached_index_ids.clear();
            self.grid_state.total_items = 0;
            self.grid_state.calculate_visible_range();
            return;
        }

        let lib_uuid = self.library_id.as_uuid();
        let yoke_opt = self
            .accessor
            .get_archived_library_yoke(&lib_uuid)
            .ok()
            .flatten();
        let Some(yoke) = yoke_opt else {
            self.filtered_indices = Some(Vec::new());
            self.cached_index_ids.clear();
            self.grid_state.total_items = 0;
            self.grid_state.calculate_visible_range();
            return;
        };

        let slice = yoke.get().media_as_slice();

        let authoritative_set = self
            .accessor
            .get_sorted_index_by_library(
                &self.library_id,
                self.sort_by,
                self.sort_order,
            )
            .ok()
            .and_then(|ids| {
                if ids.is_empty() {
                    None
                } else {
                    Some(ids.into_iter().collect::<HashSet<Uuid>>())
                }
            });

        let (filtered_indices, ids) = Self::reconcile_positions(
            positions,
            |idx| slice.get(idx).and_then(Self::extract_media_uuid),
            authoritative_set.as_ref(),
        );

        self.filtered_indices = Some(filtered_indices);
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

        for &pos in positions {
            let idx = pos as usize;
            if let Some(uuid) = fetch_uuid(idx)
                && allowed.is_none_or(|set| set.contains(&uuid))
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

        self.cached_positions = None;
        self.cached_filter_positions.clear();
        self.filtered_indices = None;

        if self.accessor.is_initialized() {
            match self.accessor.get_sorted_index_by_library(
                &self.library_id,
                self.sort_by,
                self.sort_order,
            ) {
                Ok(ids) => {
                    self.cached_index_ids = ids;
                    self.grid_state.total_items = self.cached_index_ids.len();
                    self.grid_state.calculate_visible_range();
                }
                Err(err) => {
                    log::warn!(
                        "Failed to refresh library {} from repo: {}",
                        self.library_id,
                        err
                    );
                    self.cached_index_ids.clear();
                    self.grid_state.total_items = 0;
                    self.grid_state.calculate_visible_range();
                }
            }

            self.cached_media = match self.library_type {
                LibraryType::Movies => CachedMedia::Movies(Vec::new()),
                LibraryType::Series => CachedMedia::TvShows(Vec::new()),
            };
        }

        self.needs_refresh = false;
    }

    /// Insert a newly discovered media item into the cached ordering based on the
    /// current sort configuration. Returns true if the media was inserted.
    pub fn insert_media_reference(&mut self, media: &Media) -> bool {
        if !self.matches_library_media(media) {
            return false;
        }

        let media_uuid = media.media_id().to_uuid();

        if self.cached_index_ids.contains(&media_uuid) {
            return false;
        }

        let compare_with_fallback =
            |a: &Media, a_id: &Uuid, b: &Media, b_id: &Uuid| {
                compare_media(a, b, self.sort_by, self.sort_order)
                    .unwrap_or_else(|| {
                        compare_media(a, b, SortBy::Title, SortOrder::Ascending)
                            .unwrap_or_else(|| a_id.cmp(b_id))
                    })
                    .then_with(|| a_id.cmp(b_id))
            };

        let mut insert_at = None;

        for (idx, existing_id) in self.cached_index_ids.iter().enumerate() {
            let Some(existing_media) = self.fetch_media_by_uuid(*existing_id)
            else {
                continue;
            };

            let ordering = compare_with_fallback(
                media,
                &media_uuid,
                &existing_media,
                existing_id,
            );
            if ordering != Ordering::Greater {
                insert_at = Some(idx);
                break;
            }
        }

        match insert_at {
            Some(position) => {
                self.cached_index_ids.insert(position, media_uuid)
            }
            None => self.cached_index_ids.push(media_uuid),
        }

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

    fn fetch_media_by_uuid(&self, id: Uuid) -> Option<Media> {
        let lookup_id = match self.library_type {
            LibraryType::Movies => MediaID::Movie(MovieID(id)),
            LibraryType::Series => MediaID::Series(SeriesID(id)),
        };

        match self.accessor.get(&lookup_id) {
            Ok(media) => Some(media),
            Err(err) => {
                log::warn!(
                    "Failed to fetch media {} while inserting SSE addition: {}",
                    lookup_id,
                    err
                );
                None
            }
        }
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
    pub fn tv_shows(&self) -> Option<&[&'static ArchivedSeriesReference]> {
        match &self.cached_media {
            CachedMedia::TvShows(shows) => Some(shows),
            _ => None,
        }
    }

    /// Compute visible positions for the archived slice (Phase 1, movies: filter top-level by library type)
    pub fn get_visible_positions(&self) -> (LibraryID, Vec<u32>) {
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

        let lib_uuid = self.library_id.as_uuid();
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
}

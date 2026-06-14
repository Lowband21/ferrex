//! Folder delta reconciliation for incremental scans.
//!
//! The dispatcher uses these helpers to turn a changed folder listing into the
//! smallest safe set of media pipeline work: new files, fingerprint-changing
//! modifications, fingerprint-preserving moves, and tombstones for removed
//! paths.

use async_trait::async_trait;
use ferrex_model::MediaID;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::domain::scan::actors::messages::MediaFileDiscovered;
use crate::domain::scan::orchestration::job::MediaFingerprint;
use crate::error::Result;
use crate::types::ids::LibraryId;

/// Database snapshot for one media file known to the scanner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredMediaFile {
    pub id: Uuid,
    pub media_id: MediaID,
    pub path_norm: String,
    pub fingerprint: MediaFingerprint,
    pub is_available: bool,
}

/// A fingerprint-preserving rename/move inside the same library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaMoveDelta {
    pub file_id: Uuid,
    pub media_id: MediaID,
    pub old_path_norm: String,
    pub new_path_norm: String,
    pub fingerprint: MediaFingerprint,
}

/// Direct-file reconciliation result for a single folder scan.
#[derive(Clone, Debug, Default)]
pub struct DirectMediaDelta {
    /// Files with no matching stored path/fingerprint. These need normal
    /// analyze/metadata/index work.
    pub additions: Vec<MediaFileDiscovered>,
    /// Files at a known path whose fingerprint changed. These keep the logical
    /// media identity but need refreshed analysis/metadata.
    pub modifications: Vec<MediaFileDiscovered>,
    /// Fingerprint-preserving path changes. These update repository paths and do
    /// not need media analysis.
    pub moves: Vec<MediaMoveDelta>,
    /// Stored files missing from the current folder listing and not consumed by
    /// a move. These should be tombstoned by default.
    pub removals: Vec<StoredMediaFile>,
    /// Existing files whose relevant fingerprint fields are unchanged.
    pub unchanged: Vec<StoredMediaFile>,
}

impl DirectMediaDelta {
    pub fn media_requiring_pipeline(&self) -> Vec<MediaFileDiscovered> {
        self.additions
            .iter()
            .chain(self.modifications.iter())
            .cloned()
            .collect()
    }
}

/// Repository operations needed by folder delta reconciliation.
#[async_trait]
pub trait FolderDeltaRepository: Send + Sync {
    async fn list_media_directly_under(
        &self,
        library_id: LibraryId,
        folder_path_norm: &str,
    ) -> Result<Vec<StoredMediaFile>>;

    async fn find_available_media_by_fingerprint(
        &self,
        library_id: LibraryId,
        fingerprint: &MediaFingerprint,
        excluding_path_norm: &str,
    ) -> Result<Vec<StoredMediaFile>>;

    async fn move_media_by_path(
        &self,
        library_id: LibraryId,
        old_path_norm: &str,
        new_path_norm: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<Uuid>;

    async fn mark_unavailable_by_paths(
        &self,
        library_id: LibraryId,
        paths: Vec<String>,
        reason: &str,
    ) -> Result<u64>;

    async fn mark_unavailable_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
        reason: &str,
    ) -> Result<u64>;

    async fn delete_folder_inventory_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<u64>;
}

/// No-op implementation used by focused dispatcher tests that do not exercise
/// DB-backed delta reconciliation.
#[derive(Debug, Default)]
pub struct NoopFolderDeltaRepository;

#[async_trait]
impl FolderDeltaRepository for NoopFolderDeltaRepository {
    async fn list_media_directly_under(
        &self,
        _library_id: LibraryId,
        _folder_path_norm: &str,
    ) -> Result<Vec<StoredMediaFile>> {
        Ok(Vec::new())
    }

    async fn find_available_media_by_fingerprint(
        &self,
        _library_id: LibraryId,
        _fingerprint: &MediaFingerprint,
        _excluding_path_norm: &str,
    ) -> Result<Vec<StoredMediaFile>> {
        Ok(Vec::new())
    }

    async fn move_media_by_path(
        &self,
        _library_id: LibraryId,
        _old_path_norm: &str,
        _new_path_norm: &str,
        _fingerprint: &MediaFingerprint,
    ) -> Result<Uuid> {
        Ok(Uuid::nil())
    }

    async fn mark_unavailable_by_paths(
        &self,
        _library_id: LibraryId,
        _paths: Vec<String>,
        _reason: &str,
    ) -> Result<u64> {
        Ok(0)
    }

    async fn mark_unavailable_by_prefixes(
        &self,
        _library_id: LibraryId,
        _prefixes: Vec<String>,
        _reason: &str,
    ) -> Result<u64> {
        Ok(0)
    }

    async fn delete_folder_inventory_by_prefixes(
        &self,
        _library_id: LibraryId,
        _prefixes: Vec<String>,
    ) -> Result<u64> {
        Ok(0)
    }
}

/// Compare current media discoveries with stored direct children for the same
/// folder. The result deliberately separates moves from modifications so moves
/// can update repository paths without refreshing metadata.
pub fn reconcile_direct_media(
    stored: Vec<StoredMediaFile>,
    current: Vec<MediaFileDiscovered>,
) -> DirectMediaDelta {
    let mut stored_by_path: HashMap<String, StoredMediaFile> = stored
        .into_iter()
        .map(|stored| (stored.path_norm.clone(), stored))
        .collect();

    let mut delta = DirectMediaDelta::default();
    let mut unmatched_current = Vec::new();

    for mut media in current {
        if let Some(stored) = stored_by_path.remove(&media.path_norm) {
            media.media_id = stored.media_id;
            if fingerprints_equivalent(&stored.fingerprint, &media.fingerprint)
                && stored.is_available
            {
                delta.unchanged.push(stored);
            } else {
                delta.modifications.push(media);
            }
        } else {
            unmatched_current.push(media);
        }
    }

    let mut missing: Vec<StoredMediaFile> =
        stored_by_path.into_values().collect();
    let mut consumed_missing = HashSet::new();
    let mut consumed_current = HashSet::new();

    for (current_idx, media) in unmatched_current.iter().enumerate() {
        let matches: Vec<usize> = missing
            .iter()
            .enumerate()
            .filter_map(|(missing_idx, stored)| {
                if consumed_missing.contains(&missing_idx) {
                    return None;
                }
                fingerprints_equivalent(&stored.fingerprint, &media.fingerprint)
                    .then_some(missing_idx)
            })
            .collect();

        if let [missing_idx] = matches.as_slice() {
            let stored = &missing[*missing_idx];
            delta.moves.push(MediaMoveDelta {
                file_id: stored.id,
                media_id: stored.media_id,
                old_path_norm: stored.path_norm.clone(),
                new_path_norm: media.path_norm.clone(),
                fingerprint: media.fingerprint.clone(),
            });
            consumed_missing.insert(*missing_idx);
            consumed_current.insert(current_idx);
        }
    }

    for (idx, media) in unmatched_current.into_iter().enumerate() {
        if !consumed_current.contains(&idx) {
            delta.additions.push(media);
        }
    }

    for (idx, stored) in missing.drain(..).enumerate() {
        if !consumed_missing.contains(&idx) {
            delta.removals.push(stored);
        }
    }

    delta
}

/// Returns true when two fingerprints identify the same file content/location
/// for incremental reconciliation purposes.
pub fn fingerprints_equivalent(
    stored: &MediaFingerprint,
    current: &MediaFingerprint,
) -> bool {
    if let (Some(left), Some(right)) = (&stored.weak_hash, &current.weak_hash)
        && left == right
    {
        return true;
    }

    if let (
        Some(left_device),
        Some(right_device),
        Some(left_inode),
        Some(right_inode),
    ) = (
        &stored.device_id,
        &current.device_id,
        stored.inode,
        current.inode,
    ) && left_device == right_device
        && left_inode == right_inode
        && stored.size == current.size
    {
        return true;
    }

    stored.size == current.size
        && stored.size > 0
        && stored.mtime > 0
        && current.mtime > 0
        && stored.mtime == current.mtime
}

/// Find immediate child cursor/inventory paths that disappeared from the latest
/// folder listing. The caller can tombstone media under these prefixes and then
/// remove stale scan read-model rows for the same prefixes.
pub fn removed_child_prefixes(
    parent_path_norm: &str,
    current_child_paths: impl IntoIterator<Item = String>,
    known_paths: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let current: HashSet<String> = current_child_paths.into_iter().collect();
    let mut removed = Vec::new();

    for path in known_paths {
        if path == parent_path_norm {
            continue;
        }
        if !is_immediate_child(parent_path_norm, &path) {
            continue;
        }
        if !current.contains(&path) {
            removed.push(path);
        }
    }

    removed.sort();
    removed.dedup();
    removed
}

pub fn is_immediate_child(
    parent_path_norm: &str,
    child_path_norm: &str,
) -> bool {
    let parent = Path::new(parent_path_norm);
    let child = Path::new(child_path_norm);
    match child.parent() {
        Some(child_parent) => same_path(child_parent, parent),
        None => false,
    }
}

pub fn is_direct_child_file(
    parent_path_norm: &str,
    child_path_norm: &str,
) -> bool {
    let parent = Path::new(parent_path_norm);
    let child = Path::new(child_path_norm);
    match child.parent() {
        Some(child_parent) => same_path(child_parent, parent),
        None => false,
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_lexical(left) == normalize_lexical(right)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    let mut anchored = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                anchored = true;
                out.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !anchored {
                    out.push(component.as_os_str());
                }
            }
            Component::Normal(_) => out.push(component.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::actors::messages::MediaKindHint;
    use crate::domain::scan::context::{
        MovieRootPath, MovieScanHierarchy, ScanNodeKind,
    };
    use crate::domain::scan::{AnalyzeScanHierarchy, ScanReason};
    use ferrex_model::VideoMediaType;

    fn fp(size: u64, mtime: i64) -> MediaFingerprint {
        MediaFingerprint {
            device_id: None,
            inode: None,
            size,
            mtime,
            weak_hash: None,
        }
    }

    fn stored(
        path: &str,
        media_id: MediaID,
        fingerprint: MediaFingerprint,
    ) -> StoredMediaFile {
        StoredMediaFile {
            id: Uuid::now_v7(),
            media_id,
            path_norm: path.to_string(),
            fingerprint,
            is_available: true,
        }
    }

    fn discovered(
        path: &str,
        fingerprint: MediaFingerprint,
    ) -> MediaFileDiscovered {
        let library_id = LibraryId::new();
        let root = MovieRootPath::try_new("/library/movie").unwrap();
        MediaFileDiscovered {
            library_id,
            path_norm: path.to_string(),
            fingerprint,
            classified_as: MediaKindHint::Movie,
            media_id: MediaID::new(VideoMediaType::Movie),
            variant: VideoMediaType::Movie,
            node: ScanNodeKind::MovieFolder,
            hierarchy: AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
                movie_root_path: root,
                movie_id: None,
                extra_tag: None,
            }),
            context: crate::domain::scan::context::FolderScanContext::Movie(
                crate::domain::scan::context::MovieFolderScanContext {
                    library_id,
                    movie_root_path: MovieRootPath::try_new("/library/movie")
                        .unwrap(),
                },
            ),
            scan_reason: ScanReason::MaintenanceSweep,
        }
    }

    #[test]
    fn classifies_added_media() {
        let current = vec![discovered("/library/movie/a.mkv", fp(10, 20))];
        let delta = reconcile_direct_media(Vec::new(), current);
        assert_eq!(delta.additions.len(), 1);
        assert!(delta.modifications.is_empty());
        assert!(delta.moves.is_empty());
        assert!(delta.removals.is_empty());
    }

    #[test]
    fn classifies_modified_media_when_fingerprint_changes() {
        let media_id = MediaID::new(VideoMediaType::Movie);
        let stored = vec![stored("/library/movie/a.mkv", media_id, fp(10, 20))];
        let current = vec![discovered("/library/movie/a.mkv", fp(12, 30))];
        let delta = reconcile_direct_media(stored, current);
        assert_eq!(delta.modifications.len(), 1);
        assert_eq!(delta.modifications[0].media_id, media_id);
        assert!(delta.additions.is_empty());
    }

    #[test]
    fn classifies_move_and_preserves_media_identity() {
        let media_id = MediaID::new(VideoMediaType::Movie);
        let stored =
            vec![stored("/library/movie/old.mkv", media_id, fp(10, 20))];
        let current = vec![discovered("/library/movie/new.mkv", fp(10, 20))];
        let delta = reconcile_direct_media(stored, current);
        assert_eq!(delta.moves.len(), 1);
        assert_eq!(delta.moves[0].media_id, media_id);
        assert_eq!(delta.moves[0].old_path_norm, "/library/movie/old.mkv");
        assert_eq!(delta.moves[0].new_path_norm, "/library/movie/new.mkv");
        assert!(delta.additions.is_empty());
        assert!(delta.removals.is_empty());
    }

    #[test]
    fn classifies_file_deletion() {
        let media_id = MediaID::new(VideoMediaType::Movie);
        let stored = vec![stored("/library/movie/a.mkv", media_id, fp(10, 20))];
        let delta = reconcile_direct_media(stored, Vec::new());
        assert_eq!(delta.removals.len(), 1);
        assert_eq!(delta.removals[0].media_id, media_id);
    }

    #[test]
    fn removed_child_prefixes_cover_folder_delete_and_read_model_cleanup() {
        let removed = removed_child_prefixes(
            "/library/Series",
            vec!["/library/Series/Season 02".to_string()],
            vec![
                "/library/Series".to_string(),
                "/library/Series/Season 01".to_string(),
                "/library/Series/Season 02".to_string(),
                "/library/Series/Season 01/deeper".to_string(),
            ],
        );

        assert_eq!(removed, vec!["/library/Series/Season 01".to_string()]);
    }

    #[test]
    fn unavailable_same_path_refreshes_instead_of_staying_hidden() {
        let media_id = MediaID::new(VideoMediaType::Movie);
        let mut stored_file =
            stored("/library/movie/a.mkv", media_id, fp(10, 20));
        stored_file.is_available = false;
        let current = vec![discovered("/library/movie/a.mkv", fp(10, 20))];
        let delta = reconcile_direct_media(vec![stored_file], current);
        assert_eq!(delta.modifications.len(), 1);
        assert_eq!(delta.modifications[0].media_id, media_id);
    }
}

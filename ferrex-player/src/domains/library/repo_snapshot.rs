use ferrex_core::player_prelude::{LibraryId, MovieBatchId, SeriesID};
use rkyv::util::AlignedVec;
use uuid::Uuid;

use crate::domains::library::types::{
    MovieBatchInstallCart, SeriesBundleInstallCart,
};

const SNAPSHOT_MAGIC: &[u8; 8] = b"FRXREPO\0";
const SNAPSHOT_VERSION_V1: u32 = 1;

#[derive(Debug)]
pub enum RepoSnapshotDecodeError {
    InvalidHeader,
    UnsupportedVersion(u32),
    Truncated,
    InvalidLength,
    InvalidUuid,
}

impl std::fmt::Display for RepoSnapshotDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoSnapshotDecodeError::InvalidHeader => {
                write!(f, "invalid snapshot header")
            }
            RepoSnapshotDecodeError::UnsupportedVersion(v) => {
                write!(f, "unsupported snapshot version {v}")
            }
            RepoSnapshotDecodeError::Truncated => {
                write!(f, "truncated snapshot")
            }
            RepoSnapshotDecodeError::InvalidLength => {
                write!(f, "invalid snapshot length")
            }
            RepoSnapshotDecodeError::InvalidUuid => {
                write!(f, "invalid uuid in snapshot")
            }
        }
    }
}

impl std::error::Error for RepoSnapshotDecodeError {}

#[derive(Debug)]
pub struct DecodedRepoSnapshot {
    pub movie_batches: Vec<MovieBatchInstallCart>,
    pub series_bundles: Vec<SeriesBundleInstallCart>,
}

pub fn encode_repo_snapshot(
    movie_batches: &[MovieBatchInstallCart],
    series_bundles: &[SeriesBundleInstallCart],
) -> Vec<u8> {
    let mut total_len: usize = 0;
    total_len = total_len.saturating_add(8 + 4);
    total_len = total_len.saturating_add(4);
    for batch in movie_batches {
        total_len = total_len.saturating_add(16 + 4 + 8 + 4);
        total_len = total_len.saturating_add(batch.cart.len());
    }
    total_len = total_len.saturating_add(4);
    for bundle in series_bundles {
        total_len = total_len.saturating_add(16 + 16 + 8 + 4);
        total_len = total_len.saturating_add(bundle.cart.len());
    }

    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(SNAPSHOT_MAGIC);
    out.extend_from_slice(&SNAPSHOT_VERSION_V1.to_le_bytes());

    out.extend_from_slice(&(movie_batches.len() as u32).to_le_bytes());
    for batch in movie_batches {
        out.extend_from_slice(batch.library_id.0.as_bytes());
        out.extend_from_slice(&batch.batch_id.0.to_le_bytes());
        out.extend_from_slice(&batch.version.to_le_bytes());
        out.extend_from_slice(&(batch.cart.len() as u32).to_le_bytes());
        out.extend_from_slice(batch.cart.as_slice());
    }

    out.extend_from_slice(&(series_bundles.len() as u32).to_le_bytes());
    for bundle in series_bundles {
        out.extend_from_slice(bundle.library_id.0.as_bytes());
        out.extend_from_slice(bundle.series_id.0.as_bytes());
        out.extend_from_slice(&bundle.version.to_le_bytes());
        out.extend_from_slice(&(bundle.cart.len() as u32).to_le_bytes());
        out.extend_from_slice(bundle.cart.as_slice());
    }

    out
}

pub fn decode_repo_snapshot(
    bytes: &[u8],
) -> Result<DecodedRepoSnapshot, RepoSnapshotDecodeError> {
    let mut cursor = 0usize;

    let magic = take(bytes, &mut cursor, 8)?;
    if magic != SNAPSHOT_MAGIC {
        return Err(RepoSnapshotDecodeError::InvalidHeader);
    }

    let _version = read_u32_le(bytes, &mut cursor)?;

    let movie_batch_count = read_u32_le(bytes, &mut cursor)? as usize;
    let mut movie_batches = Vec::with_capacity(movie_batch_count);
    for _ in 0..movie_batch_count {
        let library_id = read_uuid(bytes, &mut cursor)?;
        let batch_id = read_u32_le(bytes, &mut cursor)?;
        let version = read_u64_le(bytes, &mut cursor)?;
        let cart_len = read_u32_le(bytes, &mut cursor)? as usize;
        let cart_bytes = take(bytes, &mut cursor, cart_len)?;
        movie_batches.push(MovieBatchInstallCart {
            library_id: LibraryId(library_id),
            batch_id: MovieBatchId(batch_id),
            version,
            cart: aligned_from_slice(cart_bytes),
        });
    }

    let series_bundle_count = read_u32_le(bytes, &mut cursor)? as usize;
    let mut series_bundles = Vec::with_capacity(series_bundle_count);
    for _ in 0..series_bundle_count {
        let library_id = read_uuid(bytes, &mut cursor)?;
        let series_id = read_uuid(bytes, &mut cursor)?;
        let version = read_u64_le(bytes, &mut cursor)?;
        let cart_len = read_u32_le(bytes, &mut cursor)? as usize;
        let cart_bytes = take(bytes, &mut cursor, cart_len)?;
        series_bundles.push(SeriesBundleInstallCart {
            library_id: LibraryId(library_id),
            series_id: SeriesID(series_id),
            version,
            cart: aligned_from_slice(cart_bytes),
        });
    }

    if cursor != bytes.len() {
        return Err(RepoSnapshotDecodeError::InvalidLength);
    }

    Ok(DecodedRepoSnapshot {
        movie_batches,
        series_bundles,
    })
}

fn read_u32_le(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<u32, RepoSnapshotDecodeError> {
    let raw = take(bytes, cursor, 4)?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()))
}

fn read_u64_le(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<u64, RepoSnapshotDecodeError> {
    let raw = take(bytes, cursor, 8)?;
    Ok(u64::from_le_bytes(raw.try_into().unwrap()))
}

fn read_uuid(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Uuid, RepoSnapshotDecodeError> {
    let raw = take(bytes, cursor, 16)?;
    Uuid::from_slice(raw).map_err(|_| RepoSnapshotDecodeError::InvalidUuid)
}

fn take<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], RepoSnapshotDecodeError> {
    let start = *cursor;
    let end = start
        .checked_add(len)
        .ok_or(RepoSnapshotDecodeError::InvalidLength)?;
    let out = bytes
        .get(start..end)
        .ok_or(RepoSnapshotDecodeError::Truncated)?;
    *cursor = end;
    Ok(out)
}

fn aligned_from_slice(bytes: &[u8]) -> AlignedVec {
    let mut aligned = AlignedVec::with_capacity(bytes.len());
    aligned.extend_from_slice(bytes);
    if aligned.capacity() > aligned.len() * 2 {
        aligned.shrink_to_fit();
    }
    aligned
}

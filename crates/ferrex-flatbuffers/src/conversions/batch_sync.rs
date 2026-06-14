//! Conversions for mobile movie-batch and series-bundle sync requests.

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use uuid::Uuid;

use crate::fb::library as fb;
use crate::uuid_helpers::{fb_to_uuid, uuid_to_fb};

/// Version entry for a movie-reference batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchVersion {
    pub batch_id: u32,
    pub version: u64,
}

/// Version entry for a per-series bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesBundleVersion {
    pub series_id: Uuid,
    pub version: u64,
}

/// Build a FlatBuffers `BatchVersion` table.
pub fn build_batch_version<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    version: &BatchVersion,
) -> WIPOffset<fb::BatchVersion<'a>> {
    fb::BatchVersion::create(
        builder,
        &fb::BatchVersionArgs {
            batch_id: version.batch_id,
            version: version.version,
        },
    )
}

/// Parse a movie-batch sync request.
pub fn parse_batch_sync_request(
    bytes: &[u8],
) -> Result<Vec<BatchVersion>, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::BatchSyncRequest>(bytes)?;
    let versions = request
        .cached_versions()
        .map(|items| {
            items
                .iter()
                .map(|version| BatchVersion {
                    batch_id: version.batch_id(),
                    version: version.version(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(versions)
}

/// Serialize a movie-batch sync request.
pub fn serialize_batch_sync_request(
    cached_versions: &[BatchVersion],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(64 + 16 * cached_versions.len());
    let cached_versions = cached_versions
        .iter()
        .map(|version| build_batch_version(&mut builder, version))
        .collect::<Vec<_>>();
    let cached_versions = builder.create_vector(&cached_versions);
    let request = fb::BatchSyncRequest::create(
        &mut builder,
        &fb::BatchSyncRequestArgs {
            cached_versions: Some(cached_versions),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Serialize a movie-batch sync response.
pub fn serialize_batch_sync_response(
    stale_batch_ids: &[u32],
    deleted_batch_ids: &[u32],
    server_versions: &[BatchVersion],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(
        96 + 4 * stale_batch_ids.len()
            + 4 * deleted_batch_ids.len()
            + 16 * server_versions.len(),
    );
    let server_versions = server_versions
        .iter()
        .map(|version| build_batch_version(&mut builder, version))
        .collect::<Vec<_>>();
    let stale_batch_ids = builder.create_vector(stale_batch_ids);
    let deleted_batch_ids = builder.create_vector(deleted_batch_ids);
    let server_versions = builder.create_vector(&server_versions);

    let response = fb::BatchSyncResponse::create(
        &mut builder,
        &fb::BatchSyncResponseArgs {
            stale_batch_ids: Some(stale_batch_ids),
            deleted_batch_ids: Some(deleted_batch_ids),
            server_versions: Some(server_versions),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Parse a movie-batch fetch request.
pub fn parse_batch_fetch_request(
    bytes: &[u8],
) -> Result<Vec<u32>, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::BatchFetchRequest>(bytes)?;
    Ok(request
        .batch_ids()
        .map(|ids| ids.iter().collect())
        .unwrap_or_default())
}

/// Serialize a movie-batch fetch request.
pub fn serialize_batch_fetch_request(batch_ids: &[u32]) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(64 + 4 * batch_ids.len());
    let batch_ids = builder.create_vector(batch_ids);
    let request = fb::BatchFetchRequest::create(
        &mut builder,
        &fb::BatchFetchRequestArgs {
            batch_ids: Some(batch_ids),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `SeriesBundleVersion` table.
pub fn build_series_bundle_version<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    version: &SeriesBundleVersion,
) -> WIPOffset<fb::SeriesBundleVersion<'a>> {
    let series_id = uuid_to_fb(&version.series_id);
    fb::SeriesBundleVersion::create(
        builder,
        &fb::SeriesBundleVersionArgs {
            series_id: Some(&series_id),
            version: version.version,
        },
    )
}

/// Parse a per-series bundle sync request.
pub fn parse_series_bundle_sync_request(
    bytes: &[u8],
) -> Result<Vec<SeriesBundleVersion>, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::SeriesBundleSyncRequest>(bytes)?;
    let versions = request
        .cached_versions()
        .map(|items| {
            items
                .iter()
                .map(|version| SeriesBundleVersion {
                    series_id: fb_to_uuid(version.series_id()),
                    version: version.version(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(versions)
}

/// Serialize a per-series bundle sync request.
pub fn serialize_series_bundle_sync_request(
    cached_versions: &[SeriesBundleVersion],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(64 + 24 * cached_versions.len());
    let cached_versions = cached_versions
        .iter()
        .map(|version| build_series_bundle_version(&mut builder, version))
        .collect::<Vec<_>>();
    let cached_versions = builder.create_vector(&cached_versions);
    let request = fb::SeriesBundleSyncRequest::create(
        &mut builder,
        &fb::SeriesBundleSyncRequestArgs {
            cached_versions: Some(cached_versions),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

/// Serialize a per-series bundle sync response.
pub fn serialize_series_bundle_sync_response(
    stale_series_ids: &[Uuid],
    deleted_series_ids: &[Uuid],
    server_versions: &[SeriesBundleVersion],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(
        96 + 16 * stale_series_ids.len()
            + 16 * deleted_series_ids.len()
            + 24 * server_versions.len(),
    );
    let stale_series_ids =
        stale_series_ids.iter().map(uuid_to_fb).collect::<Vec<_>>();
    let deleted_series_ids = deleted_series_ids
        .iter()
        .map(uuid_to_fb)
        .collect::<Vec<_>>();
    let server_versions = server_versions
        .iter()
        .map(|version| build_series_bundle_version(&mut builder, version))
        .collect::<Vec<_>>();

    let stale_series_ids = builder.create_vector(&stale_series_ids);
    let deleted_series_ids = builder.create_vector(&deleted_series_ids);
    let server_versions = builder.create_vector(&server_versions);
    let response = fb::SeriesBundleSyncResponse::create(
        &mut builder,
        &fb::SeriesBundleSyncResponseArgs {
            stale_series_ids: Some(stale_series_ids),
            deleted_series_ids: Some(deleted_series_ids),
            server_versions: Some(server_versions),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Parse a per-series bundle fetch request.
pub fn parse_series_bundle_fetch_request(
    bytes: &[u8],
) -> Result<Vec<Uuid>, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::SeriesBundleFetchRequest>(bytes)?;
    Ok(request
        .series_ids()
        .map(|ids| ids.iter().map(|id| fb_to_uuid(&id)).collect())
        .unwrap_or_default())
}

/// Serialize a per-series bundle fetch request.
pub fn serialize_series_bundle_fetch_request(series_ids: &[Uuid]) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(64 + 16 * series_ids.len());
    let series_ids = series_ids.iter().map(uuid_to_fb).collect::<Vec<_>>();
    let series_ids = builder.create_vector(&series_ids);
    let request = fb::SeriesBundleFetchRequest::create(
        &mut builder,
        &fb::SeriesBundleFetchRequestArgs {
            series_ids: Some(series_ids),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

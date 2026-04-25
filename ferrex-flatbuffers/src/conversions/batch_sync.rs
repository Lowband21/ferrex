//! Conversions for movie-batch sync types → FlatBuffers.

use flatbuffers::FlatBufferBuilder;

use crate::fb::library as fb;

/// Entry describing a batch that needs updating.
pub struct BatchUpdateEntry {
    pub batch_id: u32,
    pub version: u64,
}

/// Serialize a `MovieBatchSyncResponse` into a FlatBuffers `BatchSyncResponse` buffer.
///
/// Maps the server's sync protocol response into the FlatBuffers schema:
/// - `stale_batch_ids`: batch IDs from `updates` (batches the client should fetch)
/// - `deleted_batch_ids`: batch IDs from `removals` (batches the client should delete)
/// - `server_versions`: version info for each stale batch
pub fn serialize_batch_sync_response(
    updates: &[BatchUpdateEntry],
    removals: &[u32],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(
        64 + 12 * updates.len() + 4 * removals.len(),
    );

    // Build server_versions vector
    let versions: Vec<_> = updates
        .iter()
        .map(|entry| {
            fb::BatchVersion::create(
                &mut builder,
                &fb::BatchVersionArgs {
                    batch_id: entry.batch_id,
                    version: entry.version,
                },
            )
        })
        .collect();
    let server_versions = builder.create_vector(&versions);

    // Build stale_batch_ids (same IDs as updates)
    let stale_ids: Vec<u32> = updates.iter().map(|e| e.batch_id).collect();
    let stale_batch_ids = builder.create_vector(&stale_ids);

    // Build deleted_batch_ids
    let deleted_batch_ids = builder.create_vector(removals);

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

//! FlatBuffers serialization for `POST /media/query` results.
//!
//! The query endpoint returns `Vec<MediaWithStatus>` — a media identifier
//! paired with optional watch progress.  We flatten these into a
//! `WatchState { items: [WatchStateEntry] }` buffer because:
//!
//! 1. The mobile client already has full media details cached locally (from
//!    batch sync), so repeating them is wasteful.
//! 2. `WatchStateEntry` carries all the fields the client needs to display
//!    a search result: media_id, position, duration, completed.
//! 3. The client resolves the media type (movie / series / etc.) from its
//!    local cache using the UUID.

use flatbuffers::FlatBufferBuilder;

use crate::fb::common::Timestamp as FbTimestamp;
use crate::fb::watch as fb;
use crate::uuid_helpers::uuid_to_fb;

/// Lightweight description of one search-result item, passed in by the server
/// handler.  Avoids depending on `ferrex-core` types in this crate.
pub struct MediaQueryHit {
    /// Raw UUID extracted from the MediaID enum.
    pub media_uuid: uuid::Uuid,
    /// Current playback position in seconds (0 if not in progress).
    pub position: f64,
    /// Total duration in seconds (0 if unknown / not in progress).
    pub duration: f64,
    /// Whether the item is marked completed.
    pub completed: bool,
    /// Last-watched unix-timestamp in seconds (0 if no watch data).
    pub last_watched_secs: i64,
}

/// Serialize a list of media-query hits into a `WatchState` buffer.
///
/// Items that have no watch status at all are still included (with zeroed
/// fields and `completed = false`) so the client knows the search matched.
pub fn serialize_media_query_results(hits: &[MediaQueryHit]) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(64 * hits.len().max(1));

    let entries: Vec<_> = hits
        .iter()
        .map(|hit| {
            let id = uuid_to_fb(&hit.media_uuid);
            let updated_at = FbTimestamp::new(hit.last_watched_secs * 1000);

            fb::WatchStateEntry::create(
                &mut builder,
                &fb::WatchStateEntryArgs {
                    media_id: Some(&id),
                    position: hit.position,
                    duration: hit.duration,
                    completed: hit.completed,
                    updated_at: Some(&updated_at),
                },
            )
        })
        .collect();

    let items = builder.create_vector(&entries);
    let state = fb::WatchState::create(
        &mut builder,
        &fb::WatchStateArgs { items: Some(items) },
    );

    builder.finish(state, None);
    builder.finished_data().to_vec()
}

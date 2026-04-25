//! Conversions for watch state types → FlatBuffers.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::fb::common::{
    Timestamp as FbTimestamp, VideoMediaType as FbVideoMediaType,
};
use crate::fb::watch as fb;
use crate::uuid_helpers::uuid_to_fb;

/// Build a single `WatchStateEntry` table from an in-progress item.
fn build_watch_state_entry<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    media_id: &uuid::Uuid,
    position: f64,
    duration: f64,
    completed: bool,
    last_watched: i64,
) -> WIPOffset<fb::WatchStateEntry<'a>> {
    let id = uuid_to_fb(media_id);
    let updated_at = FbTimestamp::new(last_watched * 1000); // seconds → millis

    fb::WatchStateEntry::create(
        builder,
        &fb::WatchStateEntryArgs {
            media_id: Some(&id),
            position,
            duration,
            completed,
            updated_at: Some(&updated_at),
        },
    )
}

/// Serialize a `UserWatchState` into a complete FlatBuffers `WatchState` buffer.
///
/// Flattens in-progress items and completed items into a single vector of
/// `WatchStateEntry`. Completed items get position=0, duration=0, completed=true.
pub fn serialize_watch_state(
    in_progress: &std::collections::HashMap<uuid::Uuid, InProgressItemRef<'_>>,
    completed: &std::collections::HashSet<uuid::Uuid>,
) -> Vec<u8> {
    let entry_count = in_progress.len() + completed.len();
    let mut builder =
        FlatBufferBuilder::with_capacity(128 * entry_count.max(1));

    let mut entries: Vec<WIPOffset<fb::WatchStateEntry>> =
        Vec::with_capacity(entry_count);

    // In-progress items
    for (media_id, item) in in_progress {
        entries.push(build_watch_state_entry(
            &mut builder,
            media_id,
            item.position as f64,
            item.duration as f64,
            false,
            item.last_watched,
        ));
    }

    // Completed items (not already in in_progress)
    for media_id in completed {
        if !in_progress.contains_key(media_id) {
            entries.push(build_watch_state_entry(
                &mut builder,
                media_id,
                0.0,
                0.0,
                true,
                0,
            ));
        }
    }

    let items = builder.create_vector(&entries);
    let state = fb::WatchState::create(
        &mut builder,
        &fb::WatchStateArgs { items: Some(items) },
    );

    builder.finish(state, None);
    builder.finished_data().to_vec()
}

/// Reference to in-progress item fields (avoids importing ferrex-core in this crate).
pub struct InProgressItemRef<'a> {
    pub media_id: &'a uuid::Uuid,
    pub position: f32,
    pub duration: f32,
    pub last_watched: i64,
}

/// Reference to presentation-ready continue-watching item fields.
pub struct ContinueWatchingItemRef<'a> {
    pub media_id: &'a uuid::Uuid,
    pub media_type: FbVideoMediaType,
    pub position: f32,
    pub duration: f32,
    pub last_watched: i64,
    pub title: Option<&'a str>,
    pub poster_iid: Option<&'a uuid::Uuid>,
}

/// Build a single `ContinueWatchingEntry` table.
fn build_continue_watching_entry<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    media_id: &uuid::Uuid,
    media_type: FbVideoMediaType,
    position: f64,
    duration: f64,
    last_watched: i64,
    title: Option<&str>,
    poster_iid: Option<&uuid::Uuid>,
) -> WIPOffset<fb::ContinueWatchingEntry<'a>> {
    let id = uuid_to_fb(media_id);
    let updated_at = FbTimestamp::new(last_watched * 1000);
    let title_offset = title.map(|t| builder.create_string(t));
    let poster = poster_iid.map(|p| uuid_to_fb(p));

    fb::ContinueWatchingEntry::create(
        builder,
        &fb::ContinueWatchingEntryArgs {
            media_id: Some(&id),
            media_type,
            position,
            duration,
            updated_at: Some(&updated_at),
            title: title_offset,
            poster_iid: poster.as_ref(),
        },
    )
}

/// Serialize a presentation-ready continue-watching list into FlatBuffers.
pub fn serialize_continue_watching_list(
    items: &[ContinueWatchingItemRef<'_>],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(128 * items.len().max(1));

    let entries: Vec<_> = items
        .iter()
        .map(|item| {
            build_continue_watching_entry(
                &mut builder,
                item.media_id,
                item.media_type,
                item.position as f64,
                item.duration as f64,
                item.last_watched,
                item.title,
                item.poster_iid,
            )
        })
        .collect();

    let items_vec = builder.create_vector(&entries);
    let list = fb::ContinueWatchingList::create(
        &mut builder,
        &fb::ContinueWatchingListArgs {
            items: Some(items_vec),
        },
    );

    builder.finish(list, None);
    builder.finished_data().to_vec()
}

//! FlatBuffers serialization for movie-batch fetch responses.
//!
//! These functions produce the wire format consumed by mobile clients for the
//! `POST /movie-batches:fetch` and `GET /movie-batches/bundle` endpoints.
//! The server cache stores source `MovieReference` structs per batch so that
//! FlatBuffers can be built without re-querying the database.

use flatbuffers::FlatBufferBuilder;

use crate::conversions::media::{
    build_movie_reference, build_series_reference, build_season_reference,
    build_episode_reference,
};
use crate::fb::library as fb_lib;
use crate::fb::media as fb_media;

/// A batch of movie references with its ID and version, ready for
/// FlatBuffers serialization.
pub struct BatchInput<'a> {
    pub batch_id: u32,
    pub version: u64,
    pub movies: &'a [ferrex_model::MovieReference],
}

/// Serialize a single batch into a FlatBuffers `MediaBatchData` table within
/// the given builder.  Returns the WIPOffset for embedding in a parent table.
fn build_media_batch_data<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    batch: &BatchInput<'_>,
) -> flatbuffers::WIPOffset<fb_lib::MediaBatchData<'bldr>> {
    // Build each MovieReference and wrap it in a Media union.
    let media_offsets: Vec<_> = batch
        .movies
        .iter()
        .map(|movie| {
            let movie_off = build_movie_reference(builder, movie);
            fb_media::Media::create(builder, &fb_media::MediaArgs {
                variant_type: fb_media::MediaVariant::MovieReference,
                variant: Some(movie_off.as_union_value()),
            })
        })
        .collect();

    let items = builder.create_vector(&media_offsets);

    fb_lib::MediaBatchData::create(builder, &fb_lib::MediaBatchDataArgs {
        batch_id: batch.batch_id,
        version: batch.version,
        items: Some(items),
    })
}

/// Serialize a complete batch-fetch response (one or more batches) into a
/// finished FlatBuffers `BatchFetchResponse` buffer.
///
/// This is the FlatBuffers equivalent of the rkyv
/// `MovieReferenceBatchBundleResponse`.
pub fn serialize_batch_fetch_response(batches: &[BatchInput<'_>]) -> Vec<u8> {
    // Rough capacity estimate: ~1KB per movie reference.
    let movie_count: usize = batches.iter().map(|b| b.movies.len()).sum();
    let mut builder = FlatBufferBuilder::with_capacity(1024 * movie_count.max(1));

    let batch_offsets: Vec<_> = batches
        .iter()
        .map(|batch| build_media_batch_data(&mut builder, batch))
        .collect();

    let batches_vec = builder.create_vector(&batch_offsets);

    let response = fb_lib::BatchFetchResponse::create(
        &mut builder,
        &fb_lib::BatchFetchResponseArgs {
            batches: Some(batches_vec),
        },
    );

    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Serialize a single batch as a standalone FlatBuffers buffer.
///
/// Used by the single-batch endpoint `GET /movie-batches/:batch_id`.
pub fn serialize_single_batch(batch: &BatchInput<'_>) -> Vec<u8> {
    serialize_batch_fetch_response(&[BatchInput {
        batch_id: batch.batch_id,
        version: batch.version,
        movies: batch.movies,
    }])
}

/// Serialize a series bundle (series + seasons + episodes) as a FlatBuffers
/// `BatchFetchResponse`.
///
/// All items are packed into a single `MediaBatchData` with `batch_id = 0`
/// and `version = 0`. The series is first, then seasons, then episodes —
/// each wrapped in a `Media` union with the appropriate `MediaVariant`.
pub fn serialize_series_bundle(
    series: &ferrex_model::Series,
    seasons: &[ferrex_model::SeasonReference],
    episodes: &[ferrex_model::EpisodeReference],
) -> Vec<u8> {
    let item_count = 1 + seasons.len() + episodes.len();
    let mut builder = FlatBufferBuilder::with_capacity(1024 * item_count.max(1));

    let mut media_offsets = Vec::with_capacity(item_count);

    // Series reference (always exactly one)
    let series_off = build_series_reference(&mut builder, series);
    media_offsets.push(fb_media::Media::create(
        &mut builder,
        &fb_media::MediaArgs {
            variant_type: fb_media::MediaVariant::SeriesReference,
            variant: Some(series_off.as_union_value()),
        },
    ));

    // Season references
    for season in seasons {
        let season_off = build_season_reference(&mut builder, season);
        media_offsets.push(fb_media::Media::create(
            &mut builder,
            &fb_media::MediaArgs {
                variant_type: fb_media::MediaVariant::SeasonReference,
                variant: Some(season_off.as_union_value()),
            },
        ));
    }

    // Episode references
    for episode in episodes {
        let episode_off = build_episode_reference(&mut builder, episode);
        media_offsets.push(fb_media::Media::create(
            &mut builder,
            &fb_media::MediaArgs {
                variant_type: fb_media::MediaVariant::EpisodeReference,
                variant: Some(episode_off.as_union_value()),
            },
        ));
    }

    let items = builder.create_vector(&media_offsets);

    let batch = fb_lib::MediaBatchData::create(
        &mut builder,
        &fb_lib::MediaBatchDataArgs {
            batch_id: 0,
            version: 0,
            items: Some(items),
        },
    );

    let batches_vec = builder.create_vector(&[batch]);

    let response = fb_lib::BatchFetchResponse::create(
        &mut builder,
        &fb_lib::BatchFetchResponseArgs {
            batches: Some(batches_vec),
        },
    );

    builder.finish(response, None);
    builder.finished_data().to_vec()
}

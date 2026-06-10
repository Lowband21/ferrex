//! Conversions for movie-batch and per-series bundle data payloads.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::media::{
    build_episode_reference, build_movie_reference, build_season_reference,
    build_series_reference,
};
use crate::fb::library as fb;
use crate::fb::media as media_fb;
use crate::uuid_helpers::uuid_to_fb;

/// Borrowed movie-batch payload.
#[derive(Debug, Clone, Copy)]
pub struct MovieBatch<'a> {
    pub batch_id: u32,
    pub version: u64,
    pub movies: &'a [ferrex_model::MovieReference],
}

/// Borrowed per-series bundle payload.
#[derive(Debug, Clone, Copy)]
pub struct SeriesBundle<'a> {
    pub version: u64,
    pub series: &'a ferrex_model::Series,
    pub seasons: &'a [ferrex_model::SeasonReference],
    pub episodes: &'a [ferrex_model::EpisodeReference],
}

fn wrap_movie_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    movie: &ferrex_model::MovieReference,
) -> WIPOffset<media_fb::Media<'a>> {
    let movie = build_movie_reference(builder, movie);
    media_fb::Media::create(
        builder,
        &media_fb::MediaArgs {
            variant_type: media_fb::MediaVariant::MovieReference,
            variant: Some(movie.as_union_value()),
        },
    )
}

fn wrap_series_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    series: &ferrex_model::Series,
) -> WIPOffset<media_fb::Media<'a>> {
    let series = build_series_reference(builder, series);
    media_fb::Media::create(
        builder,
        &media_fb::MediaArgs {
            variant_type: media_fb::MediaVariant::SeriesReference,
            variant: Some(series.as_union_value()),
        },
    )
}

fn wrap_season_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    season: &ferrex_model::SeasonReference,
) -> WIPOffset<media_fb::Media<'a>> {
    let season = build_season_reference(builder, season);
    media_fb::Media::create(
        builder,
        &media_fb::MediaArgs {
            variant_type: media_fb::MediaVariant::SeasonReference,
            variant: Some(season.as_union_value()),
        },
    )
}

fn wrap_episode_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    episode: &ferrex_model::EpisodeReference,
) -> WIPOffset<media_fb::Media<'a>> {
    let episode = build_episode_reference(builder, episode);
    media_fb::Media::create(
        builder,
        &media_fb::MediaArgs {
            variant_type: media_fb::MediaVariant::EpisodeReference,
            variant: Some(episode.as_union_value()),
        },
    )
}

/// Build a FlatBuffers `MediaBatchData` table from movie references.
pub fn build_movie_batch_data<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    batch: &MovieBatch<'_>,
) -> WIPOffset<fb::MediaBatchData<'a>> {
    let items = batch
        .movies
        .iter()
        .map(|movie| wrap_movie_reference(builder, movie))
        .collect::<Vec<_>>();
    let items = builder.create_vector(&items);

    fb::MediaBatchData::create(
        builder,
        &fb::MediaBatchDataArgs {
            batch_id: batch.batch_id,
            version: batch.version,
            items: Some(items),
        },
    )
}

/// Serialize one movie batch as root `MediaBatchData` bytes.
pub fn serialize_movie_batch_data(batch: &MovieBatch<'_>) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(1024 * batch.movies.len().max(1));
    let batch = build_movie_batch_data(&mut builder, batch);
    builder.finish(batch, None);
    builder.finished_data().to_vec()
}

/// Serialize multiple movie batches as a root `BatchFetchResponse`.
pub fn serialize_batch_fetch_response(batches: &[MovieBatch<'_>]) -> Vec<u8> {
    let total_items = batches
        .iter()
        .map(|batch| batch.movies.len())
        .sum::<usize>();
    let mut builder =
        FlatBufferBuilder::with_capacity(1024 * total_items.max(1));
    let batches = batches
        .iter()
        .map(|batch| build_movie_batch_data(&mut builder, batch))
        .collect::<Vec<_>>();
    let batches = builder.create_vector(&batches);
    let response = fb::BatchFetchResponse::create(
        &mut builder,
        &fb::BatchFetchResponseArgs {
            batches: Some(batches),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

/// Build a FlatBuffers `SeriesBundleData` table from one series and children.
pub fn build_series_bundle_data<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    bundle: &SeriesBundle<'_>,
) -> WIPOffset<fb::SeriesBundleData<'a>> {
    let series_id = uuid_to_fb(bundle.series.id.as_uuid());
    let mut items =
        Vec::with_capacity(1 + bundle.seasons.len() + bundle.episodes.len());
    items.push(wrap_series_reference(builder, bundle.series));
    items.extend(
        bundle
            .seasons
            .iter()
            .map(|season| wrap_season_reference(builder, season)),
    );
    items.extend(
        bundle
            .episodes
            .iter()
            .map(|episode| wrap_episode_reference(builder, episode)),
    );
    let items = builder.create_vector(&items);

    fb::SeriesBundleData::create(
        builder,
        &fb::SeriesBundleDataArgs {
            series_id: Some(&series_id),
            version: bundle.version,
            items: Some(items),
        },
    )
}

/// Serialize one series bundle as root `SeriesBundleData` bytes.
pub fn serialize_series_bundle_data(bundle: &SeriesBundle<'_>) -> Vec<u8> {
    let item_count = 1 + bundle.seasons.len() + bundle.episodes.len();
    let mut builder =
        FlatBufferBuilder::with_capacity(1024 * item_count.max(1));
    let bundle = build_series_bundle_data(&mut builder, bundle);
    builder.finish(bundle, None);
    builder.finished_data().to_vec()
}

/// Serialize multiple series bundles as a root `SeriesBundleFetchResponse`.
pub fn serialize_series_bundle_fetch_response(
    bundles: &[SeriesBundle<'_>],
) -> Vec<u8> {
    let total_items = bundles
        .iter()
        .map(|bundle| 1 + bundle.seasons.len() + bundle.episodes.len())
        .sum::<usize>();
    let mut builder =
        FlatBufferBuilder::with_capacity(1024 * total_items.max(1));
    let bundles = bundles
        .iter()
        .map(|bundle| build_series_bundle_data(&mut builder, bundle))
        .collect::<Vec<_>>();
    let bundles = builder.create_vector(&bundles);
    let response = fb::SeriesBundleFetchResponse::create(
        &mut builder,
        &fb::SeriesBundleFetchResponseArgs {
            bundles: Some(bundles),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}

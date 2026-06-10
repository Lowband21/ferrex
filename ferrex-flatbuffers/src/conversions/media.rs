//! Conversions for Ferrex media references.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::common::timestamp_to_fb;
use crate::conversions::details::{
    build_enhanced_movie_details, build_enhanced_series_details,
    build_episode_details, build_season_details,
};
use crate::conversions::files::build_media_file;
use crate::fb::media as fb;
use crate::uuid_helpers::uuid_to_fb;

fn string_opt<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    value: Option<&str>,
) -> Option<flatbuffers::WIPOffset<&'a str>> {
    value.map(|value| builder.create_string(value))
}

/// Build a FlatBuffers `Media` union wrapper.
pub fn build_media<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    media: &ferrex_model::Media,
) -> WIPOffset<fb::Media<'a>> {
    match media {
        ferrex_model::Media::Movie(movie) => {
            let movie = build_movie_reference(builder, movie);
            fb::Media::create(
                builder,
                &fb::MediaArgs {
                    variant_type: fb::MediaVariant::MovieReference,
                    variant: Some(movie.as_union_value()),
                },
            )
        }
        ferrex_model::Media::Series(series) => {
            let series = build_series_reference(builder, series);
            fb::Media::create(
                builder,
                &fb::MediaArgs {
                    variant_type: fb::MediaVariant::SeriesReference,
                    variant: Some(series.as_union_value()),
                },
            )
        }
        ferrex_model::Media::Season(season) => {
            let season = build_season_reference(builder, season);
            fb::Media::create(
                builder,
                &fb::MediaArgs {
                    variant_type: fb::MediaVariant::SeasonReference,
                    variant: Some(season.as_union_value()),
                },
            )
        }
        ferrex_model::Media::Episode(episode) => {
            let episode = build_episode_reference(builder, episode);
            fb::Media::create(
                builder,
                &fb::MediaArgs {
                    variant_type: fb::MediaVariant::EpisodeReference,
                    variant: Some(episode.as_union_value()),
                },
            )
        }
    }
}

/// Build a FlatBuffers `MovieReference` table.
pub fn build_movie_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    movie: &ferrex_model::MovieReference,
) -> WIPOffset<fb::MovieReference<'a>> {
    let id = uuid_to_fb(movie.id.as_uuid());
    let library_id = uuid_to_fb(movie.library_id.as_uuid());
    let title = builder.create_string(movie.title.as_str());
    let details = build_enhanced_movie_details(builder, &movie.details);
    let endpoint = builder.create_string(movie.endpoint.as_ref());
    let file = build_media_file(builder, &movie.file);
    let theme_color = string_opt(builder, movie.theme_color.as_deref());

    fb::MovieReference::create(
        builder,
        &fb::MovieReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            batch_id: movie.batch_id.map_or(0, |batch| batch.as_u32()),
            tmdb_id: movie.tmdb_id,
            title: Some(title),
            details: Some(details),
            endpoint: Some(endpoint),
            file: Some(file),
            theme_color,
        },
    )
}

/// Build a FlatBuffers `SeriesReference` table.
pub fn build_series_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    series: &ferrex_model::Series,
) -> WIPOffset<fb::SeriesReference<'a>> {
    let id = uuid_to_fb(series.id.as_uuid());
    let library_id = uuid_to_fb(series.library_id.as_uuid());
    let title = builder.create_string(series.title.as_str());
    let details = build_enhanced_series_details(builder, &series.details);
    let endpoint = builder.create_string(series.endpoint.as_ref());
    let discovered_at = timestamp_to_fb(&series.discovered_at);
    let created_at = timestamp_to_fb(&series.created_at);
    let theme_color = string_opt(builder, series.theme_color.as_deref());

    fb::SeriesReference::create(
        builder,
        &fb::SeriesReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            tmdb_id: series.tmdb_id,
            title: Some(title),
            details: Some(details),
            endpoint: Some(endpoint),
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
            theme_color,
        },
    )
}

/// Build a FlatBuffers `SeasonReference` table.
pub fn build_season_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    season: &ferrex_model::SeasonReference,
) -> WIPOffset<fb::SeasonReference<'a>> {
    let id = uuid_to_fb(season.id.as_uuid());
    let library_id = uuid_to_fb(season.library_id.as_uuid());
    let series_id = uuid_to_fb(season.series_id.as_uuid());
    let details = build_season_details(builder, &season.details);
    let endpoint = builder.create_string(season.endpoint.as_ref());
    let discovered_at = timestamp_to_fb(&season.discovered_at);
    let created_at = timestamp_to_fb(&season.created_at);
    let theme_color = string_opt(builder, season.theme_color.as_deref());

    fb::SeasonReference::create(
        builder,
        &fb::SeasonReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            season_number: season.season_number.value(),
            series_id: Some(&series_id),
            tmdb_series_id: season.tmdb_series_id,
            details: Some(details),
            endpoint: Some(endpoint),
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
            theme_color,
        },
    )
}

/// Build a FlatBuffers `EpisodeReference` table.
pub fn build_episode_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    episode: &ferrex_model::EpisodeReference,
) -> WIPOffset<fb::EpisodeReference<'a>> {
    let id = uuid_to_fb(episode.id.as_uuid());
    let library_id = uuid_to_fb(episode.library_id.as_uuid());
    let season_id = uuid_to_fb(episode.season_id.as_uuid());
    let series_id = uuid_to_fb(episode.series_id.as_uuid());
    let details = build_episode_details(builder, &episode.details);
    let endpoint = builder.create_string(episode.endpoint.as_ref());
    let file = build_media_file(builder, &episode.file);
    let discovered_at = timestamp_to_fb(&episode.discovered_at);
    let created_at = timestamp_to_fb(&episode.created_at);

    fb::EpisodeReference::create(
        builder,
        &fb::EpisodeReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            episode_number: episode.episode_number.value(),
            season_number: episode.season_number.value(),
            season_id: Some(&season_id),
            series_id: Some(&series_id),
            tmdb_series_id: episode.tmdb_series_id,
            details: Some(details),
            endpoint: Some(endpoint),
            file: Some(file),
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
        },
    )
}

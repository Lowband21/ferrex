//! Conversions for `ferrex_model::media` → FlatBuffers.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::common::timestamp_to_fb;
use crate::conversions::details::{
    build_enhanced_movie_details, build_enhanced_series_details,
    build_episode_details, build_season_details,
};
use crate::conversions::files::build_media_file;
use crate::fb::media as fb;
use crate::uuid_helpers::uuid_to_fb;

/// Build a `MovieReference` table.
pub fn build_movie_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    m: &ferrex_model::MovieReference,
) -> WIPOffset<fb::MovieReference<'a>> {
    let id = uuid_to_fb(m.id.as_uuid());
    let library_id = uuid_to_fb(m.library_id.as_uuid());
    let title = builder.create_string(m.title.as_str());
    let endpoint = builder.create_string(m.endpoint.as_ref());
    let theme_color =
        m.theme_color.as_deref().map(|s| builder.create_string(s));
    let details = build_enhanced_movie_details(builder, &m.details);
    let file = build_media_file(builder, &m.file);

    fb::MovieReference::create(
        builder,
        &fb::MovieReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            batch_id: m.batch_id.map(|b| b.as_u32()).unwrap_or(0),
            tmdb_id: m.tmdb_id,
            title: Some(title),
            details: Some(details),
            endpoint: Some(endpoint),
            file: Some(file),
            theme_color,
        },
    )
}

/// Build a `SeriesReference` table.
pub fn build_series_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    s: &ferrex_model::Series,
) -> WIPOffset<fb::SeriesReference<'a>> {
    let id = uuid_to_fb(s.id.as_uuid());
    let library_id = uuid_to_fb(s.library_id.as_uuid());
    let title = builder.create_string(s.title.as_str());
    let endpoint = builder.create_string(s.endpoint.as_ref());
    let theme_color =
        s.theme_color.as_deref().map(|ss| builder.create_string(ss));
    let details = build_enhanced_series_details(builder, &s.details);
    let discovered_at = timestamp_to_fb(&s.discovered_at);
    let created_at = timestamp_to_fb(&s.created_at);

    fb::SeriesReference::create(
        builder,
        &fb::SeriesReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            tmdb_id: s.tmdb_id,
            title: Some(title),
            details: Some(details),
            endpoint: Some(endpoint),
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
            theme_color,
        },
    )
}

/// Build a `SeasonReference` table.
pub fn build_season_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    s: &ferrex_model::SeasonReference,
) -> WIPOffset<fb::SeasonReference<'a>> {
    let id = uuid_to_fb(s.id.as_uuid());
    let library_id = uuid_to_fb(s.library_id.as_uuid());
    let series_id = uuid_to_fb(s.series_id.as_uuid());
    let endpoint = builder.create_string(s.endpoint.as_ref());
    let theme_color =
        s.theme_color.as_deref().map(|ss| builder.create_string(ss));
    let details = build_season_details(builder, &s.details);
    let discovered_at = timestamp_to_fb(&s.discovered_at);
    let created_at = timestamp_to_fb(&s.created_at);

    fb::SeasonReference::create(
        builder,
        &fb::SeasonReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            season_number: s.season_number.value(),
            series_id: Some(&series_id),
            tmdb_series_id: s.tmdb_series_id,
            details: Some(details),
            endpoint: Some(endpoint),
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
            theme_color,
        },
    )
}

/// Build an `EpisodeReference` table.
pub fn build_episode_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    e: &ferrex_model::EpisodeReference,
) -> WIPOffset<fb::EpisodeReference<'a>> {
    let id = uuid_to_fb(e.id.as_uuid());
    let library_id = uuid_to_fb(e.library_id.as_uuid());
    let season_id = uuid_to_fb(e.season_id.as_uuid());
    let series_id = uuid_to_fb(e.series_id.as_uuid());
    let endpoint = builder.create_string(e.endpoint.as_ref());
    let details = build_episode_details(builder, &e.details);
    let file = build_media_file(builder, &e.file);
    let discovered_at = timestamp_to_fb(&e.discovered_at);
    let created_at = timestamp_to_fb(&e.created_at);

    fb::EpisodeReference::create(
        builder,
        &fb::EpisodeReferenceArgs {
            id: Some(&id),
            library_id: Some(&library_id),
            episode_number: e.episode_number.value(),
            season_number: e.season_number.value(),
            season_id: Some(&season_id),
            series_id: Some(&series_id),
            tmdb_series_id: e.tmdb_series_id,
            details: Some(details),
            endpoint: Some(endpoint),
            file: Some(file),
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
        },
    )
}

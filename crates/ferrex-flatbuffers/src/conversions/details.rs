//! Conversions for `ferrex_model` detail structs.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::fb::details as fb;
use crate::uuid_helpers::uuid_to_fb;

fn string_opt<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    value: Option<&str>,
) -> Option<flatbuffers::WIPOffset<&'a str>> {
    value.map(|value| builder.create_string(value))
}

fn uuid_opt(value: Option<&uuid::Uuid>) -> Option<crate::fb::ids::Uuid> {
    value.map(uuid_to_fb)
}

/// Build a FlatBuffers `GenreInfo` table.
pub fn build_genre<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    genre: &ferrex_model::GenreInfo,
) -> WIPOffset<fb::GenreInfo<'a>> {
    let name = builder.create_string(&genre.name);
    fb::GenreInfo::create(
        builder,
        &fb::GenreInfoArgs {
            id: genre.id,
            name: Some(name),
        },
    )
}

fn build_genre_vector<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    genres: &[ferrex_model::GenreInfo],
) -> Option<
    flatbuffers::WIPOffset<
        flatbuffers::Vector<
            'a,
            flatbuffers::ForwardsUOffset<fb::GenreInfo<'a>>,
        >,
    >,
> {
    if genres.is_empty() {
        return None;
    }

    let items: Vec<_> = genres
        .iter()
        .map(|genre| build_genre(builder, genre))
        .collect();
    Some(builder.create_vector(&items))
}

/// Build a FlatBuffers `EnhancedMovieDetails` table.
pub fn build_enhanced_movie_details<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    details: &ferrex_model::EnhancedMovieDetails,
) -> WIPOffset<fb::EnhancedMovieDetails<'a>> {
    let title = builder.create_string(&details.title);
    let original_title = string_opt(builder, details.original_title.as_deref());
    let overview = string_opt(builder, details.overview.as_deref());
    let release_date = string_opt(builder, details.release_date.as_deref());
    let content_rating = string_opt(builder, details.content_rating.as_deref());
    let homepage = string_opt(builder, details.homepage.as_deref());
    let status = string_opt(builder, details.status.as_deref());
    let tagline = string_opt(builder, details.tagline.as_deref());
    let poster_path = string_opt(builder, details.poster_path.as_deref());
    let backdrop_path = string_opt(builder, details.backdrop_path.as_deref());
    let logo_path = string_opt(builder, details.logo_path.as_deref());
    let primary_poster_iid = uuid_opt(details.primary_poster_iid.as_ref());
    let primary_backdrop_iid = uuid_opt(details.primary_backdrop_iid.as_ref());
    let genres = build_genre_vector(builder, &details.genres);

    fb::EnhancedMovieDetails::create(
        builder,
        &fb::EnhancedMovieDetailsArgs {
            id: details.id,
            title: Some(title),
            original_title,
            overview,
            release_date,
            runtime: details.runtime.unwrap_or(0),
            vote_average: details.vote_average.unwrap_or(0.0),
            vote_count: details.vote_count.unwrap_or(0),
            popularity: details.popularity.unwrap_or(0.0),
            content_rating,
            genres,
            homepage,
            status,
            tagline,
            budget: details.budget.unwrap_or(0),
            revenue: details.revenue.unwrap_or(0),
            poster_path,
            backdrop_path,
            logo_path,
            primary_poster_iid: primary_poster_iid.as_ref(),
            primary_backdrop_iid: primary_backdrop_iid.as_ref(),
        },
    )
}

/// Build a FlatBuffers `EnhancedSeriesDetails` table.
pub fn build_enhanced_series_details<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    details: &ferrex_model::EnhancedSeriesDetails,
) -> WIPOffset<fb::EnhancedSeriesDetails<'a>> {
    let name = builder.create_string(&details.name);
    let original_name = string_opt(builder, details.original_name.as_deref());
    let overview = string_opt(builder, details.overview.as_deref());
    let first_air_date = string_opt(builder, details.first_air_date.as_deref());
    let last_air_date = string_opt(builder, details.last_air_date.as_deref());
    let content_rating = string_opt(builder, details.content_rating.as_deref());
    let homepage = string_opt(builder, details.homepage.as_deref());
    let status = string_opt(builder, details.status.as_deref());
    let tagline = string_opt(builder, details.tagline.as_deref());
    let poster_path = string_opt(builder, details.poster_path.as_deref());
    let backdrop_path = string_opt(builder, details.backdrop_path.as_deref());
    let logo_path = string_opt(builder, details.logo_path.as_deref());
    let primary_poster_iid = uuid_opt(details.primary_poster_iid.as_ref());
    let primary_backdrop_iid = uuid_opt(details.primary_backdrop_iid.as_ref());
    let genres = build_genre_vector(builder, &details.genres);

    fb::EnhancedSeriesDetails::create(
        builder,
        &fb::EnhancedSeriesDetailsArgs {
            id: details.id,
            name: Some(name),
            original_name,
            overview,
            first_air_date,
            last_air_date,
            number_of_seasons: details.number_of_seasons.unwrap_or(0),
            number_of_episodes: details.number_of_episodes.unwrap_or(0),
            available_seasons: details.available_seasons.unwrap_or(0),
            available_episodes: details.available_episodes.unwrap_or(0),
            vote_average: details.vote_average.unwrap_or(0.0),
            vote_count: details.vote_count.unwrap_or(0),
            popularity: details.popularity.unwrap_or(0.0),
            content_rating,
            genres,
            homepage,
            status,
            tagline,
            in_production: details.in_production.unwrap_or(false),
            poster_path,
            backdrop_path,
            logo_path,
            primary_poster_iid: primary_poster_iid.as_ref(),
            primary_backdrop_iid: primary_backdrop_iid.as_ref(),
        },
    )
}

/// Build a FlatBuffers `SeasonDetails` table.
pub fn build_season_details<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    details: &ferrex_model::SeasonDetails,
) -> WIPOffset<fb::SeasonDetails<'a>> {
    let name = builder.create_string(&details.name);
    let overview = string_opt(builder, details.overview.as_deref());
    let air_date = string_opt(builder, details.air_date.as_deref());
    let poster_path = string_opt(builder, details.poster_path.as_deref());
    let primary_poster_iid = uuid_opt(details.primary_poster_iid.as_ref());

    fb::SeasonDetails::create(
        builder,
        &fb::SeasonDetailsArgs {
            id: details.id,
            season_number: details.season_number,
            name: Some(name),
            overview,
            air_date,
            episode_count: details.episode_count,
            poster_path,
            primary_poster_iid: primary_poster_iid.as_ref(),
            runtime: details.runtime.unwrap_or(0),
        },
    )
}

/// Build a FlatBuffers `EpisodeDetails` table.
pub fn build_episode_details<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    details: &ferrex_model::EpisodeDetails,
) -> WIPOffset<fb::EpisodeDetails<'a>> {
    let name = builder.create_string(&details.name);
    let overview = string_opt(builder, details.overview.as_deref());
    let air_date = string_opt(builder, details.air_date.as_deref());
    let still_path = string_opt(builder, details.still_path.as_deref());
    let production_code =
        string_opt(builder, details.production_code.as_deref());
    let primary_still_iid = uuid_opt(details.primary_still_iid.as_ref());

    fb::EpisodeDetails::create(
        builder,
        &fb::EpisodeDetailsArgs {
            id: details.id,
            episode_number: details.episode_number,
            season_number: details.season_number,
            name: Some(name),
            overview,
            air_date,
            runtime: details.runtime.unwrap_or(0),
            still_path,
            primary_still_iid: primary_still_iid.as_ref(),
            vote_average: details.vote_average.unwrap_or(0.0),
            vote_count: details.vote_count.unwrap_or(0),
            production_code,
        },
    )
}

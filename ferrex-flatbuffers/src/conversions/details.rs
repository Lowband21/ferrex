//! Conversions for `ferrex-model::details` → FlatBuffers detail tables.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::fb::details as fb;
use crate::uuid_helpers::{option_uuid_to_fb, uuid_to_fb};

/// Build a `GenreInfo` table.
pub fn build_genre<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    g: &ferrex_model::GenreInfo,
) -> WIPOffset<fb::GenreInfo<'a>> {
    let name = builder.create_string(&g.name);
    fb::GenreInfo::create(builder, &fb::GenreInfoArgs {
        id: g.id,
        name: Some(name),
    })
}

/// Build a `CastMember` table.
pub fn build_cast_member<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    c: &ferrex_model::details::CastMember,
) -> WIPOffset<fb::CastMember<'a>> {
    let name = builder.create_string(&c.name);
    let original_name = c.original_name.as_deref().map(|s| builder.create_string(s));
    let character = builder.create_string(&c.character);
    let profile_path = c.profile_path.as_deref().map(|s| builder.create_string(s));
    let credit_id = c.credit_id.as_deref().map(|s| builder.create_string(s));
    let known_for = c.known_for_department.as_deref().map(|s| builder.create_string(s));
    let person_id = c.person_id.as_ref().map(|id| uuid_to_fb(id));
    let image_id = c.image_id.as_ref().map(|id| uuid_to_fb(id));

    let also_known_as_vec: Vec<_> = c.also_known_as.iter()
        .map(|s| builder.create_string(s))
        .collect();
    let also_known_as = if also_known_as_vec.is_empty() {
        None
    } else {
        Some(builder.create_vector(&also_known_as_vec))
    };

    let external_ids = build_person_external_ids(builder, &c.external_ids);

    fb::CastMember::create(builder, &fb::CastMemberArgs {
        id: c.id,
        person_id: person_id.as_ref(),
        credit_id,
        cast_id: c.cast_id.unwrap_or(0),
        name: Some(name),
        original_name,
        character: Some(character),
        profile_path,
        order: c.order,
        gender: c.gender.unwrap_or(0),
        known_for_department: known_for,
        adult: c.adult.unwrap_or(false),
        popularity: c.popularity.unwrap_or(0.0),
        also_known_as,
        external_ids: Some(external_ids),
        image_slot: c.image_slot,
        image_id: image_id.as_ref(),
    })
}

/// Build `PersonExternalIds`.
pub fn build_person_external_ids<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    ids: &ferrex_model::details::PersonExternalIds,
) -> WIPOffset<fb::PersonExternalIds<'a>> {
    let imdb = ids.imdb_id.as_deref().map(|s| builder.create_string(s));
    let facebook = ids.facebook_id.as_deref().map(|s| builder.create_string(s));
    let instagram = ids.instagram_id.as_deref().map(|s| builder.create_string(s));
    let twitter = ids.twitter_id.as_deref().map(|s| builder.create_string(s));
    let wikidata = ids.wikidata_id.as_deref().map(|s| builder.create_string(s));
    let tiktok = ids.tiktok_id.as_deref().map(|s| builder.create_string(s));
    let youtube = ids.youtube_id.as_deref().map(|s| builder.create_string(s));

    fb::PersonExternalIds::create(builder, &fb::PersonExternalIdsArgs {
        imdb_id: imdb,
        facebook_id: facebook,
        instagram_id: instagram,
        twitter_id: twitter,
        wikidata_id: wikidata,
        tiktok_id: tiktok,
        youtube_id: youtube,
    })
}

/// Build `ExternalIds`.
pub fn build_external_ids<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    ids: &ferrex_model::details::ExternalIds,
) -> WIPOffset<fb::ExternalIds<'a>> {
    let imdb = ids.imdb_id.as_deref().map(|s| builder.create_string(s));
    let facebook = ids.facebook_id.as_deref().map(|s| builder.create_string(s));
    let instagram = ids.instagram_id.as_deref().map(|s| builder.create_string(s));
    let twitter = ids.twitter_id.as_deref().map(|s| builder.create_string(s));
    let wikidata = ids.wikidata_id.as_deref().map(|s| builder.create_string(s));
    let tiktok = ids.tiktok_id.as_deref().map(|s| builder.create_string(s));
    let youtube = ids.youtube_id.as_deref().map(|s| builder.create_string(s));
    let freebase = ids.freebase_id.as_deref().map(|s| builder.create_string(s));
    let freebase_mid = ids.freebase_mid.as_deref().map(|s| builder.create_string(s));

    fb::ExternalIds::create(builder, &fb::ExternalIdsArgs {
        imdb_id: imdb,
        tvdb_id: ids.tvdb_id.unwrap_or(0),
        facebook_id: facebook,
        instagram_id: instagram,
        twitter_id: twitter,
        wikidata_id: wikidata,
        tiktok_id: tiktok,
        youtube_id: youtube,
        freebase_id: freebase,
        freebase_mid: freebase_mid,
    })
}

/// Build a `SpokenLanguage` table.
pub fn build_spoken_language<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    l: &ferrex_model::SpokenLanguage,
) -> WIPOffset<fb::SpokenLanguage<'a>> {
    let iso = l.iso_639_1.as_deref().map(|s| builder.create_string(s));
    let name = builder.create_string(&l.name);
    fb::SpokenLanguage::create(builder, &fb::SpokenLanguageArgs {
        iso_639_1: iso,
        name: Some(name),
    })
}

/// Build a `ProductionCompany` table.
pub fn build_production_company<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    p: &ferrex_model::ProductionCompany,
) -> WIPOffset<fb::ProductionCompany<'a>> {
    let name = builder.create_string(&p.name);
    let country = p.origin_country.as_deref().map(|s| builder.create_string(s));
    fb::ProductionCompany::create(builder, &fb::ProductionCompanyArgs {
        id: p.id,
        name: Some(name),
        origin_country: country,
    })
}

/// Build a `ProductionCountry` table.
pub fn build_production_country<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    p: &ferrex_model::ProductionCountry,
) -> WIPOffset<fb::ProductionCountry<'a>> {
    let iso = builder.create_string(&p.iso_3166_1);
    let name = builder.create_string(&p.name);
    fb::ProductionCountry::create(builder, &fb::ProductionCountryArgs {
        iso_3166_1: Some(iso),
        name: Some(name),
    })
}

/// Build `MediaImages`.
pub fn build_media_images<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    imgs: &ferrex_model::image::MediaImages,
) -> WIPOffset<fb::MediaImages<'a>> {
    let posters: Vec<_> = imgs.posters.iter().map(|i| build_image_with_metadata(builder, i)).collect();
    let backdrops: Vec<_> = imgs.backdrops.iter().map(|i| build_image_with_metadata(builder, i)).collect();
    let logos: Vec<_> = imgs.logos.iter().map(|i| build_image_with_metadata(builder, i)).collect();
    let stills: Vec<_> = imgs.stills.iter().map(|i| build_image_with_metadata(builder, i)).collect();

    let posters = if posters.is_empty() { None } else { Some(builder.create_vector(&posters)) };
    let backdrops = if backdrops.is_empty() { None } else { Some(builder.create_vector(&backdrops)) };
    let logos = if logos.is_empty() { None } else { Some(builder.create_vector(&logos)) };
    let stills = if stills.is_empty() { None } else { Some(builder.create_vector(&stills)) };

    fb::MediaImages::create(builder, &fb::MediaImagesArgs {
        posters,
        backdrops,
        logos,
        stills,
    })
}

fn build_image_with_metadata<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    img: &ferrex_model::image::ImageWithMetadata,
) -> WIPOffset<fb::ImageWithMetadata<'a>> {
    let endpoint = builder.create_string(&img.endpoint);
    let meta = build_image_metadata(builder, &img.metadata);
    fb::ImageWithMetadata::create(builder, &fb::ImageWithMetadataArgs {
        endpoint: Some(endpoint),
        metadata: Some(meta),
    })
}

fn build_image_metadata<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    m: &ferrex_model::image::ImageMetadata,
) -> WIPOffset<fb::ImageMetadata<'a>> {
    let file_path = builder.create_string(&m.file_path);
    let iso = m.iso_639_1.as_deref().map(|s| builder.create_string(s));
    fb::ImageMetadata::create(builder, &fb::ImageMetadataArgs {
        file_path: Some(file_path),
        width: m.width,
        height: m.height,
        aspect_ratio: m.aspect_ratio,
        iso_639_1: iso,
        vote_average: m.vote_average,
        vote_count: m.vote_count,
    })
}

/// Build `EnhancedMovieDetails`.
pub fn build_enhanced_movie_details<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    d: &ferrex_model::EnhancedMovieDetails,
) -> WIPOffset<fb::EnhancedMovieDetails<'a>> {
    // Strings
    let title = builder.create_string(&d.title);
    let original_title = d.original_title.as_deref().map(|s| builder.create_string(s));
    let overview = d.overview.as_deref().map(|s| builder.create_string(s));
    let release_date = d.release_date.as_deref().map(|s| builder.create_string(s));
    let content_rating = d.content_rating.as_deref().map(|s| builder.create_string(s));
    let homepage = d.homepage.as_deref().map(|s| builder.create_string(s));
    let status = d.status.as_deref().map(|s| builder.create_string(s));
    let tagline = d.tagline.as_deref().map(|s| builder.create_string(s));
    let poster_path = d.poster_path.as_deref().map(|s| builder.create_string(s));
    let backdrop_path = d.backdrop_path.as_deref().map(|s| builder.create_string(s));
    let logo_path = d.logo_path.as_deref().map(|s| builder.create_string(s));

    // Image IDs
    let primary_poster_iid = d.primary_poster_iid.as_ref().map(|id| uuid_to_fb(id));
    let primary_backdrop_iid = d.primary_backdrop_iid.as_ref().map(|id| uuid_to_fb(id));

    // Vectors
    let genres: Vec<_> = d.genres.iter().map(|g| build_genre(builder, g)).collect();
    let genres = if genres.is_empty() { None } else { Some(builder.create_vector(&genres)) };

    let spoken_languages: Vec<_> = d.spoken_languages.iter().map(|l| build_spoken_language(builder, l)).collect();
    let spoken_languages = if spoken_languages.is_empty() { None } else { Some(builder.create_vector(&spoken_languages)) };

    let production_companies: Vec<_> = d.production_companies.iter().map(|p| build_production_company(builder, p)).collect();
    let production_companies = if production_companies.is_empty() { None } else { Some(builder.create_vector(&production_companies)) };

    let production_countries: Vec<_> = d.production_countries.iter().map(|p| build_production_country(builder, p)).collect();
    let production_countries = if production_countries.is_empty() { None } else { Some(builder.create_vector(&production_countries)) };

    let cast: Vec<_> = d.cast.iter().map(|c| build_cast_member(builder, c)).collect();
    let cast = if cast.is_empty() { None } else { Some(builder.create_vector(&cast)) };

    let images = build_media_images(builder, &d.images);
    let external_ids = build_external_ids(builder, &d.external_ids);

    fb::EnhancedMovieDetails::create(builder, &fb::EnhancedMovieDetailsArgs {
        id: d.id,
        title: Some(title),
        original_title,
        overview,
        release_date,
        runtime: d.runtime.unwrap_or(0),
        vote_average: d.vote_average.unwrap_or(0.0),
        vote_count: d.vote_count.unwrap_or(0),
        popularity: d.popularity.unwrap_or(0.0),
        content_rating,
        content_ratings: None, // TODO: implement when needed
        release_dates: None,   // TODO: implement when needed
        genres,
        spoken_languages,
        production_companies,
        production_countries,
        homepage,
        status,
        tagline,
        budget: d.budget.unwrap_or(0),
        revenue: d.revenue.unwrap_or(0),
        poster_path,
        backdrop_path,
        logo_path,
        primary_poster_iid: primary_poster_iid.as_ref(),
        primary_backdrop_iid: primary_backdrop_iid.as_ref(),
        images: Some(images),
        cast,
        crew: None, // TODO: implement when needed
        videos: None,
        keywords: None,
        external_ids: Some(external_ids),
        alternative_titles: None,
        translations: None,
        collection: None,
        recommendations: None,
        similar: None,
    })
}

/// Build `EnhancedSeriesDetails`.
pub fn build_enhanced_series_details<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    d: &ferrex_model::EnhancedSeriesDetails,
) -> WIPOffset<fb::EnhancedSeriesDetails<'a>> {
    let name = builder.create_string(&d.name);
    let original_name = d.original_name.as_deref().map(|s| builder.create_string(s));
    let overview = d.overview.as_deref().map(|s| builder.create_string(s));
    let first_air_date = d.first_air_date.as_deref().map(|s| builder.create_string(s));
    let last_air_date = d.last_air_date.as_deref().map(|s| builder.create_string(s));
    let content_rating = d.content_rating.as_deref().map(|s| builder.create_string(s));
    let homepage = d.homepage.as_deref().map(|s| builder.create_string(s));
    let status = d.status.as_deref().map(|s| builder.create_string(s));
    let tagline = d.tagline.as_deref().map(|s| builder.create_string(s));
    let poster_path = d.poster_path.as_deref().map(|s| builder.create_string(s));
    let backdrop_path = d.backdrop_path.as_deref().map(|s| builder.create_string(s));
    let logo_path = d.logo_path.as_deref().map(|s| builder.create_string(s));

    let primary_poster_iid = d.primary_poster_iid.as_ref().map(|id| uuid_to_fb(id));
    let primary_backdrop_iid = d.primary_backdrop_iid.as_ref().map(|id| uuid_to_fb(id));

    let genres: Vec<_> = d.genres.iter().map(|g| build_genre(builder, g)).collect();
    let genres = if genres.is_empty() { None } else { Some(builder.create_vector(&genres)) };

    let cast: Vec<_> = d.cast.iter().map(|c| build_cast_member(builder, c)).collect();
    let cast = if cast.is_empty() { None } else { Some(builder.create_vector(&cast)) };

    let images = build_media_images(builder, &d.images);
    let external_ids = build_external_ids(builder, &d.external_ids);

    fb::EnhancedSeriesDetails::create(builder, &fb::EnhancedSeriesDetailsArgs {
        id: d.id,
        name: Some(name),
        original_name,
        overview,
        first_air_date,
        last_air_date,
        number_of_seasons: d.number_of_seasons.unwrap_or(0),
        number_of_episodes: d.number_of_episodes.unwrap_or(0),
        available_seasons: d.available_seasons.unwrap_or(0),
        available_episodes: d.available_episodes.unwrap_or(0),
        vote_average: d.vote_average.unwrap_or(0.0),
        vote_count: d.vote_count.unwrap_or(0),
        popularity: d.popularity.unwrap_or(0.0),
        content_rating,
        content_ratings: None,
        release_dates: None,
        genres,
        networks: None,
        origin_countries: None,
        spoken_languages: None,
        production_companies: None,
        production_countries: None,
        homepage,
        status,
        tagline,
        in_production: d.in_production.unwrap_or(false),
        poster_path,
        backdrop_path,
        logo_path,
        primary_poster_iid: primary_poster_iid.as_ref(),
        primary_backdrop_iid: primary_backdrop_iid.as_ref(),
        images: Some(images),
        cast,
        crew: None,
        videos: None,
        keywords: None,
        external_ids: Some(external_ids),
        alternative_titles: None,
        translations: None,
        episode_groups: None,
        recommendations: None,
        similar: None,
    })
}

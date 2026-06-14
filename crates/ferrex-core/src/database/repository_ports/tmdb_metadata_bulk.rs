use std::collections::HashMap;

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    database::repository_ports::tmdb_metadata::{
        build_movie_content_ratings, build_person_external_ids,
    },
    error::{MediaError, Result},
    types::{
        details::{
            AlternativeTitle, CastMember, CollectionInfo, ContentRating,
            CrewMember, EnhancedMovieDetails, EnhancedSeriesDetails,
            EpisodeDetails, EpisodeGroupMembership, ExternalIds, GenreInfo,
            Keyword, NetworkInfo, ProductionCompany, ProductionCountry,
            RelatedMediaRef, ReleaseDateEntry, ReleaseDatesByCountry,
            SeasonDetails, SpokenLanguage, Translation, Video,
        },
        image::MediaImages,
    },
};

fn push_grouped<T>(map: &mut HashMap<Uuid, Vec<T>>, key: Uuid, value: T) {
    map.entry(key).or_default().push(value);
}

pub(crate) async fn load_movie_details_bulk(
    pool: &PgPool,
    movie_ids: &[Uuid],
) -> Result<HashMap<Uuid, EnhancedMovieDetails>> {
    if movie_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let metadata_rows = sqlx::query!(
        r#"
        SELECT
            movie_id,
            tmdb_id,
            title,
            original_title,
            overview,
            release_date,
            runtime,
            vote_average,
            vote_count,
            popularity,
            primary_certification,
            homepage,
            status,
            tagline,
            budget,
            revenue,
            poster_path,
            backdrop_path,
            primary_poster_image_id,
            primary_backdrop_image_id,
            logo_path,
            imdb_id,
            facebook_id,
            instagram_id,
            twitter_id,
            wikidata_id,
            tiktok_id,
            youtube_id
        FROM movie_metadata
        WHERE movie_id = ANY($1)
        "#,
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load movie metadata: {}", e))
    })?;

    let mut metadata_map = HashMap::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        metadata_map.insert(row.movie_id, row);
    }

    let genres_rows = sqlx::query!(
        "SELECT movie_id, genre_id, name FROM movie_genres WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load movie genres: {}", e))
    })?;

    let mut genres_map = HashMap::new();
    for record in genres_rows {
        push_grouped(
            &mut genres_map,
            record.movie_id,
            GenreInfo {
                id: record.genre_id as u64,
                name: record.name,
            },
        );
    }

    let language_rows = sqlx::query!(
        "SELECT movie_id, iso_639_1, name FROM movie_spoken_languages WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie languages: {}",
            e
        ))
    })?;

    let mut spoken_languages_map = HashMap::new();
    for record in language_rows {
        push_grouped(
            &mut spoken_languages_map,
            record.movie_id,
            SpokenLanguage {
                iso_639_1: record.iso_639_1,
                name: record.name,
            },
        );
    }

    let company_rows = sqlx::query!(
        "SELECT movie_id, company_id, name, origin_country FROM movie_production_companies WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie companies: {}",
            e
        ))
    })?;

    let mut production_companies_map = HashMap::new();
    for record in company_rows {
        push_grouped(
            &mut production_companies_map,
            record.movie_id,
            ProductionCompany {
                id: record.company_id.unwrap_or_default() as u64,
                name: record.name,
                origin_country: record.origin_country,
            },
        );
    }

    let countries_rows = sqlx::query!(
        "SELECT movie_id, iso_3166_1, name FROM movie_production_countries WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie countries: {}",
            e
        ))
    })?;

    let mut production_countries_map = HashMap::new();
    for record in countries_rows {
        push_grouped(
            &mut production_countries_map,
            record.movie_id,
            ProductionCountry {
                iso_3166_1: record.iso_3166_1,
                name: record.name,
            },
        );
    }

    let release_rows = sqlx::query!(
        r#"SELECT movie_id, iso_3166_1, iso_639_1, certification, release_date, release_type, note, descriptors
           FROM movie_release_dates WHERE movie_id = ANY($1)"#,
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie release dates: {}",
            e
        ))
    })?;

    let mut release_map: HashMap<Uuid, HashMap<String, Vec<ReleaseDateEntry>>> =
        HashMap::new();
    for record in release_rows {
        let entry = ReleaseDateEntry {
            certification: record.certification,
            release_date: Some(
                record.release_date.with_timezone(&Utc).to_rfc3339(),
            ),
            release_type: Some(i32::from(record.release_type)),
            note: record.note,
            iso_639_1: record.iso_639_1,
            descriptors: record.descriptors.unwrap_or_default(),
        };

        release_map
            .entry(record.movie_id)
            .or_default()
            .entry(record.iso_3166_1)
            .or_default()
            .push(entry);
    }

    let mut release_dates_map: HashMap<Uuid, Vec<ReleaseDatesByCountry>> =
        HashMap::new();
    for (movie_id, by_country) in release_map {
        let release_dates: Vec<ReleaseDatesByCountry> = by_country
            .into_iter()
            .map(|(iso, entries)| ReleaseDatesByCountry {
                iso_3166_1: iso,
                release_dates: entries,
            })
            .collect();
        release_dates_map.insert(movie_id, release_dates);
    }

    let alt_rows = sqlx::query!(
        "SELECT movie_id, iso_3166_1, title, title_type FROM movie_alternative_titles WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie alternative titles: {}",
            e
        ))
    })?;

    let mut alternative_titles_map = HashMap::new();
    for record in alt_rows {
        push_grouped(
            &mut alternative_titles_map,
            record.movie_id,
            AlternativeTitle {
                title: record.title,
                iso_3166_1: record.iso_3166_1,
                title_type: record.title_type,
            },
        );
    }

    let translation_rows = sqlx::query!(
        r#"SELECT movie_id, iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM movie_translations WHERE movie_id = ANY($1)"#,
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie translations: {}",
            e
        ))
    })?;

    let mut translations_map = HashMap::new();
    for record in translation_rows {
        push_grouped(
            &mut translations_map,
            record.movie_id,
            Translation {
                iso_3166_1: record.iso_3166_1,
                iso_639_1: record.iso_639_1,
                name: record.name,
                english_name: record.english_name,
                title: record.title,
                overview: record.overview,
                homepage: record.homepage,
                tagline: record.tagline,
            },
        );
    }

    let video_rows = sqlx::query!(
        r#"SELECT movie_id, video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM movie_videos WHERE movie_id = ANY($1)"#,
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load movie videos: {}", e))
    })?;

    let mut videos_map = HashMap::new();
    for record in video_rows {
        push_grouped(
            &mut videos_map,
            record.movie_id,
            Video {
                key: record.video_key,
                name: record.name,
                site: record.site,
                video_type: record.video_type,
                official: record.official,
                iso_639_1: record.iso_639_1,
                iso_3166_1: record.iso_3166_1,
                published_at: record
                    .published_at
                    .map(|dt| dt.with_timezone(&Utc).to_rfc3339()),
                size: record.size.map(|s| s as u32),
            },
        );
    }

    let keyword_rows = sqlx::query!(
        "SELECT movie_id, keyword_id, name FROM movie_keywords WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load movie keywords: {}", e))
    })?;

    let mut keywords_map = HashMap::new();
    for record in keyword_rows {
        push_grouped(
            &mut keywords_map,
            record.movie_id,
            Keyword {
                id: record.keyword_id as u64,
                name: record.name,
            },
        );
    }

    let rec_rows = sqlx::query!(
        "SELECT movie_id, recommended_tmdb_id, title FROM movie_recommendations WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie recommendations: {}",
            e
        ))
    })?;

    let mut recommendations_map = HashMap::new();
    for record in rec_rows {
        push_grouped(
            &mut recommendations_map,
            record.movie_id,
            RelatedMediaRef {
                tmdb_id: record.recommended_tmdb_id as u64,
                title: record.title,
            },
        );
    }

    let similar_rows = sqlx::query!(
        "SELECT movie_id, similar_tmdb_id, title FROM movie_similar WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load movie similar: {}", e))
    })?;

    let mut similar_map = HashMap::new();
    for record in similar_rows {
        push_grouped(
            &mut similar_map,
            record.movie_id,
            RelatedMediaRef {
                tmdb_id: record.similar_tmdb_id as u64,
                title: record.title,
            },
        );
    }

    let collection_rows = sqlx::query!(
        "SELECT movie_id, collection_id, name, poster_path, backdrop_path FROM movie_collection_membership WHERE movie_id = ANY($1)",
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load movie collections: {}",
            e
        ))
    })?;

    let mut collection_map = HashMap::new();
    for record in collection_rows {
        collection_map.insert(
            record.movie_id,
            CollectionInfo {
                id: record.collection_id as u64,
                name: record.name,
                poster_path: record.poster_path,
                backdrop_path: record.backdrop_path,
            },
        );
    }

    let cast_rows = sqlx::query!(
        r#"SELECT
                mc.movie_id,
                mc.person_tmdb_id,
                mc.credit_id,
                mc.cast_id,
                COALESCE(mc.character, '') AS "character!",
                mc.order_index,
                mc.profile_image_id AS profile_iid,
                p.id,
                p.name,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM movie_cast mc
            JOIN persons p ON p.id = mc.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = mc.person_id
            WHERE mc.movie_id = ANY($1)
            ORDER BY mc.movie_id, mc.order_index"#,
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie cast: {}", e)))?;

    let mut cast_map = HashMap::new();
    for record in cast_rows {
        let image_slot = record.order_index.unwrap_or_default() as u32;
        push_grouped(
            &mut cast_map,
            record.movie_id,
            CastMember {
                id: record.person_tmdb_id as u64,
                person_id: Some(record.id),
                credit_id: record.credit_id,
                cast_id: record.cast_id.map(|c| c as u64),
                name: record.name.clone(),
                original_name: record.original_name,
                character: record.character,
                profile_path: record.profile_path.clone(),
                order: image_slot,
                gender: record.gender.map(|g| g as u8),
                known_for_department: record.known_for_department.clone(),
                adult: record.adult,
                popularity: record.popularity,
                also_known_as: record.aliases,
                external_ids: build_person_external_ids(
                    record.imdb_id,
                    record.facebook_id,
                    record.instagram_id,
                    record.twitter_id,
                    record.wikidata_id,
                    record.tiktok_id,
                    record.youtube_id,
                ),
                image_slot,
                image_id: record.profile_iid,
            },
        );
    }

    let crew_rows = sqlx::query!(
        r#"SELECT
                mc.movie_id,
                mc.person_tmdb_id,
                mc.credit_id,
                mc.department,
                mc.job,
                p.id,
                p.name,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM movie_crew mc
            JOIN persons p ON p.id = mc.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = p.id
            WHERE mc.movie_id = ANY($1)"#,
        movie_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie crew: {}", e)))?;

    let mut crew_map = HashMap::new();
    for record in crew_rows {
        push_grouped(
            &mut crew_map,
            record.movie_id,
            CrewMember {
                id: record.person_tmdb_id as u64,
                person_id: Some(record.id),
                credit_id: record.credit_id,
                name: record.name.clone(),
                job: record.job,
                department: record.department,
                profile_path: record.profile_path.clone(),
                gender: record.gender.map(|g| g as u8),
                known_for_department: record.known_for_department.clone(),
                adult: record.adult,
                popularity: record.popularity,
                original_name: record.original_name,
                also_known_as: record.aliases,
                external_ids: build_person_external_ids(
                    record.imdb_id,
                    record.facebook_id,
                    record.instagram_id,
                    record.twitter_id,
                    record.wikidata_id,
                    record.tiktok_id,
                    record.youtube_id,
                ),
                profile_iid: None,
            },
        );
    }

    let mut details_map = HashMap::with_capacity(metadata_map.len());
    for (movie_id, row) in metadata_map {
        let release_dates =
            release_dates_map.remove(&movie_id).unwrap_or_default();
        let content_ratings = build_movie_content_ratings(
            &release_dates,
            row.primary_certification.clone(),
        );

        let details = EnhancedMovieDetails {
            id: row.tmdb_id as u64,
            title: row.title.clone(),
            original_title: row.original_title.clone(),
            overview: row.overview.clone(),
            release_date: row.release_date.map(|d| d.to_string()),
            runtime: row.runtime.map(|r| r as u32),
            vote_average: row.vote_average,
            vote_count: row.vote_count.map(|c| c as u32),
            popularity: row.popularity,
            content_rating: row.primary_certification.clone(),
            content_ratings,
            release_dates,
            genres: genres_map.remove(&movie_id).unwrap_or_default(),
            spoken_languages: spoken_languages_map
                .remove(&movie_id)
                .unwrap_or_default(),
            production_companies: production_companies_map
                .remove(&movie_id)
                .unwrap_or_default(),
            production_countries: production_countries_map
                .remove(&movie_id)
                .unwrap_or_default(),
            homepage: row.homepage.clone(),
            status: row.status.clone(),
            tagline: row.tagline.clone(),
            budget: row.budget.map(|b| b as u64),
            revenue: row.revenue.map(|r| r as u64),
            poster_path: row.poster_path.clone(),
            backdrop_path: row.backdrop_path.clone(),
            logo_path: row.logo_path.clone(),
            primary_poster_iid: Some(row.primary_poster_image_id),
            primary_backdrop_iid: row.primary_backdrop_image_id,
            images: MediaImages::default(),
            cast: cast_map.remove(&movie_id).unwrap_or_default(),
            crew: crew_map.remove(&movie_id).unwrap_or_default(),
            videos: videos_map.remove(&movie_id).unwrap_or_default(),
            keywords: keywords_map.remove(&movie_id).unwrap_or_default(),
            external_ids: ExternalIds {
                imdb_id: row.imdb_id.clone(),
                tvdb_id: None,
                facebook_id: row.facebook_id.clone(),
                instagram_id: row.instagram_id.clone(),
                twitter_id: row.twitter_id.clone(),
                wikidata_id: row.wikidata_id.clone(),
                tiktok_id: row.tiktok_id.clone(),
                youtube_id: row.youtube_id.clone(),
                freebase_id: None,
                freebase_mid: None,
            },
            alternative_titles: alternative_titles_map
                .remove(&movie_id)
                .unwrap_or_default(),
            translations: translations_map
                .remove(&movie_id)
                .unwrap_or_default(),
            collection: collection_map.remove(&movie_id),
            recommendations: recommendations_map
                .remove(&movie_id)
                .unwrap_or_default(),
            similar: similar_map.remove(&movie_id).unwrap_or_default(),
        };

        details_map.insert(movie_id, details);
    }

    Ok(details_map)
}

pub(crate) async fn load_series_details_bulk(
    pool: &PgPool,
    series_ids: &[Uuid],
) -> Result<HashMap<Uuid, EnhancedSeriesDetails>> {
    if series_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let metadata_rows = sqlx::query!(
        r#"SELECT
                series_id,
                tmdb_id,
                name,
                original_name,
                overview,
                first_air_date,
                last_air_date,
                number_of_seasons,
                number_of_episodes,
                vote_average,
                vote_count,
                popularity,
                primary_content_rating,
                homepage,
                status,
                tagline,
                in_production,
                poster_path,
                backdrop_path,
                primary_poster_image_id,
                primary_backdrop_image_id,
                logo_path,
                imdb_id,
                tvdb_id,
                facebook_id,
                instagram_id,
                twitter_id,
                wikidata_id,
                tiktok_id,
                youtube_id
            FROM series_metadata
            WHERE series_id = ANY($1)"#,
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load series metadata: {}", e))
    })?;

    let mut metadata_map = HashMap::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        metadata_map.insert(row.series_id, row);
    }

    let genre_rows = sqlx::query!(
        "SELECT series_id, genre_id, name FROM series_genres WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series genres: {}",
            e
        ))
    })?;

    let mut genres_map = HashMap::new();
    for record in genre_rows {
        push_grouped(
            &mut genres_map,
            record.series_id,
            GenreInfo {
                id: record.genre_id as u64,
                name: record.name,
            },
        );
    }

    let origin_rows = sqlx::query!(
        "SELECT series_id, iso_3166_1 FROM series_origin_countries WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series origin countries: {}",
            e
        ))
    })?;

    let mut origin_countries_map = HashMap::new();
    for record in origin_rows {
        push_grouped(
            &mut origin_countries_map,
            record.series_id,
            record.iso_3166_1,
        );
    }

    let language_rows = sqlx::query!(
        "SELECT series_id, iso_639_1, name FROM series_spoken_languages WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series spoken languages: {}",
            e
        ))
    })?;

    let mut spoken_languages_map = HashMap::new();
    for record in language_rows {
        push_grouped(
            &mut spoken_languages_map,
            record.series_id,
            SpokenLanguage {
                iso_639_1: record.iso_639_1,
                name: record.name,
            },
        );
    }

    let company_rows = sqlx::query!(
        "SELECT series_id, company_id, name, origin_country FROM series_production_companies WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series production companies: {}",
            e
        ))
    })?;

    let mut production_companies_map = HashMap::new();
    for record in company_rows {
        push_grouped(
            &mut production_companies_map,
            record.series_id,
            ProductionCompany {
                id: record.company_id.unwrap_or_default() as u64,
                name: record.name,
                origin_country: record.origin_country,
            },
        );
    }

    let country_rows = sqlx::query!(
        "SELECT series_id, iso_3166_1, name FROM series_production_countries WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series production countries: {}",
            e
        ))
    })?;

    let mut production_countries_map = HashMap::new();
    for record in country_rows {
        push_grouped(
            &mut production_countries_map,
            record.series_id,
            ProductionCountry {
                iso_3166_1: record.iso_3166_1,
                name: record.name,
            },
        );
    }

    let network_rows = sqlx::query!(
        "SELECT series_id, network_id, name, origin_country FROM series_networks WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load series networks: {}", e))
    })?;

    let mut networks_map = HashMap::new();
    for record in network_rows {
        push_grouped(
            &mut networks_map,
            record.series_id,
            NetworkInfo {
                id: record.network_id as u64,
                name: record.name,
                origin_country: record.origin_country,
            },
        );
    }

    let rating_rows = sqlx::query!(
        "SELECT series_id, iso_3166_1, rating, rating_system, descriptors FROM series_content_ratings WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series content ratings: {}",
            e
        ))
    })?;

    let mut content_ratings_map = HashMap::new();
    for record in rating_rows {
        push_grouped(
            &mut content_ratings_map,
            record.series_id,
            ContentRating {
                iso_3166_1: record.iso_3166_1,
                rating: record.rating,
                rating_system: record.rating_system,
                descriptors: record.descriptors.unwrap_or_default(),
            },
        );
    }

    let keyword_rows = sqlx::query!(
        "SELECT series_id, keyword_id, name FROM series_keywords WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load series keywords: {}", e))
    })?;

    let mut keywords_map = HashMap::new();
    for record in keyword_rows {
        push_grouped(
            &mut keywords_map,
            record.series_id,
            Keyword {
                id: record.keyword_id as u64,
                name: record.name,
            },
        );
    }

    let video_rows = sqlx::query!(
        r#"SELECT series_id, video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM series_videos WHERE series_id = ANY($1)"#,
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load series videos: {}", e))
    })?;

    let mut videos_map = HashMap::new();
    for record in video_rows {
        push_grouped(
            &mut videos_map,
            record.series_id,
            Video {
                key: record.video_key,
                name: record.name,
                site: record.site,
                video_type: record.video_type,
                official: record.official,
                iso_639_1: record.iso_639_1,
                iso_3166_1: record.iso_3166_1,
                published_at: record
                    .published_at
                    .map(|dt| dt.with_timezone(&Utc).to_rfc3339()),
                size: record.size.map(|s| s as u32),
            },
        );
    }

    let translation_rows = sqlx::query!(
        r#"SELECT series_id, iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM series_translations WHERE series_id = ANY($1)"#,
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series translations: {}",
            e
        ))
    })?;

    let mut translations_map = HashMap::new();
    for record in translation_rows {
        push_grouped(
            &mut translations_map,
            record.series_id,
            Translation {
                iso_3166_1: record.iso_3166_1,
                iso_639_1: record.iso_639_1,
                name: record.name,
                english_name: record.english_name,
                title: record.title,
                overview: record.overview,
                homepage: record.homepage,
                tagline: record.tagline,
            },
        );
    }

    let group_rows = sqlx::query!(
        "SELECT series_id, group_id, name, description, group_type FROM series_episode_groups WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series episode groups: {}",
            e
        ))
    })?;

    let mut episode_groups_map = HashMap::new();
    for record in group_rows {
        push_grouped(
            &mut episode_groups_map,
            record.series_id,
            EpisodeGroupMembership {
                id: record.group_id,
                name: record.name,
                description: record.description,
                group_type: record.group_type,
            },
        );
    }

    let rec_rows = sqlx::query!(
        "SELECT series_id, recommended_tmdb_id, title FROM series_recommendations WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load series recommendations: {}",
            e
        ))
    })?;

    let mut recommendations_map = HashMap::new();
    for record in rec_rows {
        push_grouped(
            &mut recommendations_map,
            record.series_id,
            RelatedMediaRef {
                tmdb_id: record.recommended_tmdb_id as u64,
                title: record.title,
            },
        );
    }

    let similar_rows = sqlx::query!(
        "SELECT series_id, similar_tmdb_id, title FROM series_similar WHERE series_id = ANY($1)",
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load similar series: {}",
            e
        ))
    })?;

    let mut similar_map = HashMap::new();
    for record in similar_rows {
        push_grouped(
            &mut similar_map,
            record.series_id,
            RelatedMediaRef {
                tmdb_id: record.similar_tmdb_id as u64,
                title: record.title,
            },
        );
    }

    let cast_rows = sqlx::query!(
        r#"SELECT
                sc.series_id,
                sc.person_tmdb_id,
                sc.credit_id,
                COALESCE(sc.character, '') AS "character!",
                sc.total_episode_count,
                sc.order_index,
                sc.profile_image_id AS profile_iid,
                p.name,
                p.id,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM series_cast sc
            JOIN persons p ON p.id = sc.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = sc.person_id
            WHERE sc.series_id = ANY($1)
            ORDER BY sc.series_id, sc.order_index"#,
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series cast: {}", e)))?;

    let mut cast_map = HashMap::new();
    for record in cast_rows {
        let image_slot = record.order_index.unwrap_or_default() as u32;
        push_grouped(
            &mut cast_map,
            record.series_id,
            CastMember {
                id: record.person_tmdb_id as u64,
                person_id: Some(record.id),
                credit_id: record.credit_id,
                cast_id: None,
                name: record.name.clone(),
                original_name: record.original_name,
                character: record.character,
                profile_path: record.profile_path.clone(),
                order: image_slot,
                gender: record.gender.map(|g| g as u8),
                known_for_department: record.known_for_department.clone(),
                adult: record.adult,
                popularity: record.popularity,
                also_known_as: record.aliases,
                external_ids: build_person_external_ids(
                    record.imdb_id,
                    record.facebook_id,
                    record.instagram_id,
                    record.twitter_id,
                    record.wikidata_id,
                    record.tiktok_id,
                    record.youtube_id,
                ),
                image_slot,
                image_id: record.profile_iid,
            },
        );
    }

    let crew_rows = sqlx::query!(
        r#"SELECT
                sc.series_id,
                sc.person_tmdb_id,
                sc.credit_id,
                sc.department,
                sc.job,
                p.id,
                p.name,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM series_crew sc
            JOIN persons p ON p.id = sc.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = p.id
            WHERE sc.series_id = ANY($1)"#,
        series_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series crew: {}", e)))?;

    let mut crew_map = HashMap::new();
    for record in crew_rows {
        push_grouped(
            &mut crew_map,
            record.series_id,
            CrewMember {
                id: record.person_tmdb_id as u64,
                person_id: Some(record.id),
                credit_id: record.credit_id,
                name: record.name.clone(),
                job: record.job,
                department: record.department,
                profile_path: record.profile_path.clone(),
                gender: record.gender.map(|g| g as u8),
                known_for_department: record.known_for_department.clone(),
                adult: record.adult,
                popularity: record.popularity,
                original_name: record.original_name,
                also_known_as: record.aliases,
                external_ids: build_person_external_ids(
                    record.imdb_id,
                    record.facebook_id,
                    record.instagram_id,
                    record.twitter_id,
                    record.wikidata_id,
                    record.tiktok_id,
                    record.youtube_id,
                ),
                profile_iid: None,
            },
        );
    }

    let mut details_map = HashMap::with_capacity(metadata_map.len());
    for (series_id, row) in metadata_map {
        let details = EnhancedSeriesDetails {
            id: row.tmdb_id as u64,
            name: row.name.clone(),
            original_name: row.original_name.clone(),
            overview: row.overview.clone(),
            first_air_date: row.first_air_date.map(|d| d.to_string()),
            last_air_date: row.last_air_date.map(|d| d.to_string()),
            number_of_seasons: row.number_of_seasons.map(|n| n as u16),
            number_of_episodes: row.number_of_episodes.map(|n| n as u16),
            available_seasons: None,
            available_episodes: None,
            vote_average: row.vote_average,
            vote_count: row.vote_count.map(|v| v as u32),
            popularity: row.popularity,
            content_rating: row.primary_content_rating.clone(),
            content_ratings: content_ratings_map
                .remove(&series_id)
                .unwrap_or_default(),
            release_dates: Vec::new(),
            genres: genres_map.remove(&series_id).unwrap_or_default(),
            networks: networks_map.remove(&series_id).unwrap_or_default(),
            origin_countries: origin_countries_map
                .remove(&series_id)
                .unwrap_or_default(),
            spoken_languages: spoken_languages_map
                .remove(&series_id)
                .unwrap_or_default(),
            production_companies: production_companies_map
                .remove(&series_id)
                .unwrap_or_default(),
            production_countries: production_countries_map
                .remove(&series_id)
                .unwrap_or_default(),
            homepage: row.homepage.clone(),
            status: row.status.clone(),
            tagline: row.tagline.clone(),
            in_production: row.in_production,
            poster_path: row.poster_path.clone(),
            backdrop_path: row.backdrop_path.clone(),
            logo_path: row.logo_path.clone(),
            primary_poster_iid: Some(row.primary_poster_image_id),
            primary_backdrop_iid: row.primary_backdrop_image_id,
            images: MediaImages::default(),
            cast: cast_map.remove(&series_id).unwrap_or_default(),
            crew: crew_map.remove(&series_id).unwrap_or_default(),
            videos: videos_map.remove(&series_id).unwrap_or_default(),
            keywords: keywords_map.remove(&series_id).unwrap_or_default(),
            external_ids: ExternalIds {
                imdb_id: row.imdb_id.clone(),
                tvdb_id: row.tvdb_id.map(|id| id as u32),
                facebook_id: row.facebook_id.clone(),
                instagram_id: row.instagram_id.clone(),
                twitter_id: row.twitter_id.clone(),
                wikidata_id: row.wikidata_id.clone(),
                tiktok_id: row.tiktok_id.clone(),
                youtube_id: row.youtube_id.clone(),
                freebase_id: None,
                freebase_mid: None,
            },
            alternative_titles: Vec::new(),
            translations: translations_map
                .remove(&series_id)
                .unwrap_or_default(),
            episode_groups: episode_groups_map
                .remove(&series_id)
                .unwrap_or_default(),
            recommendations: recommendations_map
                .remove(&series_id)
                .unwrap_or_default(),
            similar: similar_map.remove(&series_id).unwrap_or_default(),
        };

        details_map.insert(series_id, details);
    }

    Ok(details_map)
}

pub(crate) async fn load_season_details_bulk(
    pool: &PgPool,
    season_ids: &[Uuid],
) -> Result<HashMap<Uuid, SeasonDetails>> {
    if season_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let metadata_rows = sqlx::query!(
        r#"SELECT
                sm.season_id,
                sm.tmdb_id,
                sm.name,
                sm.overview,
                sm.air_date,
                sm.episode_count,
                sm.poster_path,
                sm.primary_poster_image_id,
                sm.runtime,
                sm.imdb_id,
                sm.facebook_id,
                sm.instagram_id,
                sm.twitter_id,
                sm.wikidata_id,
                sr.season_number
            FROM season_metadata sm
            JOIN season_references sr ON sr.id = sm.season_id
            WHERE sm.season_id = ANY($1)"#,
        season_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load season metadata: {}", e))
    })?;

    let mut metadata_map = HashMap::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        metadata_map.insert(row.season_id, row);
    }

    let video_rows = sqlx::query!(
        r#"SELECT season_id, video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM season_videos WHERE season_id = ANY($1)"#,
        season_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load season videos: {}", e))
    })?;

    let mut videos_map = HashMap::new();
    for record in video_rows {
        push_grouped(
            &mut videos_map,
            record.season_id,
            Video {
                key: record.video_key,
                name: record.name,
                site: record.site,
                video_type: record.video_type,
                official: record.official,
                iso_639_1: record.iso_639_1,
                iso_3166_1: record.iso_3166_1,
                published_at: record
                    .published_at
                    .map(|dt| dt.with_timezone(&Utc).to_rfc3339()),
                size: record.size.map(|s| s as u32),
            },
        );
    }

    let keyword_rows = sqlx::query!(
        "SELECT season_id, keyword_id, name FROM season_keywords WHERE season_id = ANY($1)",
        season_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load season keywords: {}", e))
    })?;

    let mut keywords_map = HashMap::new();
    for record in keyword_rows {
        push_grouped(
            &mut keywords_map,
            record.season_id,
            Keyword {
                id: record.keyword_id as u64,
                name: record.name,
            },
        );
    }

    let translation_rows = sqlx::query!(
        r#"SELECT season_id, iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM season_translations WHERE season_id = ANY($1)"#,
        season_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load season translations: {}",
            e
        ))
    })?;

    let mut translations_map = HashMap::new();
    for record in translation_rows {
        push_grouped(
            &mut translations_map,
            record.season_id,
            Translation {
                iso_3166_1: record.iso_3166_1,
                iso_639_1: record.iso_639_1,
                name: record.name,
                english_name: record.english_name,
                title: record.title,
                overview: record.overview,
                homepage: record.homepage,
                tagline: record.tagline,
            },
        );
    }

    let mut details_map = HashMap::with_capacity(metadata_map.len());
    for (season_id, row) in metadata_map {
        let raw_season_number = row.season_number;
        let season_number =
            u16::try_from(raw_season_number).unwrap_or_default();
        let details = SeasonDetails {
            id: row.tmdb_id as u64,
            season_number,
            name: row
                .name
                .clone()
                .unwrap_or_else(|| format!("Season {}", season_number)),
            overview: row.overview.clone(),
            air_date: row.air_date.map(|d| d.to_string()),
            episode_count: row.episode_count.unwrap_or_default() as u16,
            poster_path: row.poster_path.clone(),
            primary_poster_iid: Some(row.primary_poster_image_id),
            runtime: row.runtime.map(|r| r as u32),
            external_ids: ExternalIds {
                imdb_id: row.imdb_id.clone(),
                tvdb_id: None,
                facebook_id: row.facebook_id.clone(),
                instagram_id: row.instagram_id.clone(),
                twitter_id: row.twitter_id.clone(),
                wikidata_id: row.wikidata_id.clone(),
                tiktok_id: None,
                youtube_id: None,
                freebase_id: None,
                freebase_mid: None,
            },
            images: MediaImages::default(),
            videos: videos_map.remove(&season_id).unwrap_or_default(),
            keywords: keywords_map.remove(&season_id).unwrap_or_default(),
            translations: translations_map
                .remove(&season_id)
                .unwrap_or_default(),
        };

        details_map.insert(season_id, details);
    }

    Ok(details_map)
}

pub(crate) async fn load_episode_details_bulk(
    pool: &PgPool,
    episode_ids: &[Uuid],
) -> Result<HashMap<Uuid, EpisodeDetails>> {
    if episode_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let metadata_rows = sqlx::query!(
        r#"SELECT
                episode_id,
                tmdb_id,
                season_number,
                episode_number,
                name,
                overview,
                air_date,
                runtime,
                still_path,
                primary_thumbnail_image_id,
                vote_average,
                vote_count,
                production_code,
                imdb_id,
                tvdb_id,
                facebook_id,
                instagram_id,
                twitter_id,
                wikidata_id
            FROM episode_metadata
            WHERE episode_id = ANY($1)"#,
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load episode metadata: {}", e))
    })?;

    let mut metadata_map = HashMap::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        metadata_map.insert(row.episode_id, row);
    }

    let video_rows = sqlx::query!(
        r#"SELECT episode_id, video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM episode_videos WHERE episode_id = ANY($1)"#,
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load episode videos: {}", e))
    })?;

    let mut videos_map = HashMap::new();
    for record in video_rows {
        push_grouped(
            &mut videos_map,
            record.episode_id,
            Video {
                key: record.video_key,
                name: record.name,
                site: record.site,
                video_type: record.video_type,
                official: record.official,
                iso_639_1: record.iso_639_1,
                iso_3166_1: record.iso_3166_1,
                published_at: record
                    .published_at
                    .map(|dt| dt.with_timezone(&Utc).to_rfc3339()),
                size: record.size.map(|s| s as u32),
            },
        );
    }

    let keyword_rows = sqlx::query!(
        "SELECT episode_id, keyword_id, name FROM episode_keywords WHERE episode_id = ANY($1)",
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load episode keywords: {}",
            e
        ))
    })?;

    let mut keywords_map = HashMap::new();
    for record in keyword_rows {
        push_grouped(
            &mut keywords_map,
            record.episode_id,
            Keyword {
                id: record.keyword_id as u64,
                name: record.name,
            },
        );
    }

    let translation_rows = sqlx::query!(
        r#"SELECT episode_id, iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM episode_translations WHERE episode_id = ANY($1)"#,
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load episode translations: {}",
            e
        ))
    })?;

    let mut translations_map = HashMap::new();
    for record in translation_rows {
        push_grouped(
            &mut translations_map,
            record.episode_id,
            Translation {
                iso_3166_1: record.iso_3166_1,
                iso_639_1: record.iso_639_1,
                name: record.name,
                english_name: record.english_name,
                title: record.title,
                overview: record.overview,
                homepage: record.homepage,
                tagline: record.tagline,
            },
        );
    }

    let rating_rows = sqlx::query!(
        "SELECT episode_id, iso_3166_1, rating, rating_system, descriptors FROM episode_content_ratings WHERE episode_id = ANY($1)",
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load episode content ratings: {}",
            e
        ))
    })?;

    let mut content_ratings_map = HashMap::new();
    for record in rating_rows {
        push_grouped(
            &mut content_ratings_map,
            record.episode_id,
            ContentRating {
                iso_3166_1: record.iso_3166_1,
                rating: record.rating,
                rating_system: record.rating_system,
                descriptors: record.descriptors.unwrap_or_default(),
            },
        );
    }

    let cast_rows = sqlx::query!(
        r#"SELECT
                ec.episode_id,
                ec.person_tmdb_id,
                ec.credit_id,
                COALESCE(ec.character, '') AS "character!",
                ec.order_index,
                ec.profile_image_id AS profile_iid,
                p.id,
                p.name,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM episode_cast ec
            JOIN persons p ON p.id = ec.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = p.id
            WHERE ec.episode_id = ANY($1)
            ORDER BY ec.episode_id, ec.order_index"#,
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load episode cast: {}", e))
    })?;

    let mut guest_map: HashMap<Uuid, HashMap<u64, CastMember>> = HashMap::new();

    for record in cast_rows {
        let image_slot = record.order_index.unwrap_or_default() as u32;
        let member = CastMember {
            id: record.person_tmdb_id as u64,
            person_id: Some(record.id),
            credit_id: record.credit_id,
            cast_id: None,
            name: record.name.clone(),
            original_name: record.original_name,
            character: record.character,
            profile_path: record.profile_path.clone(),
            order: image_slot,
            gender: record.gender.map(|g| g as u8),
            known_for_department: record.known_for_department.clone(),
            adult: record.adult,
            popularity: record.popularity,
            also_known_as: record.aliases.clone(),
            external_ids: build_person_external_ids(
                record.imdb_id,
                record.facebook_id,
                record.instagram_id,
                record.twitter_id,
                record.wikidata_id,
                record.tiktok_id,
                record.youtube_id,
            ),
            image_slot,
            image_id: record.profile_iid,
        };

        guest_map
            .entry(record.episode_id)
            .or_default()
            .insert(member.id, member);
    }

    let guest_rows = sqlx::query!(
        r#"SELECT
                eg.episode_id,
                eg.person_tmdb_id,
                eg.credit_id,
                COALESCE(eg.character, '') AS "character!",
                eg.order_index,
                eg.profile_image_id AS profile_iid,
                p.name,
                p.id,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM episode_guest_stars eg
            JOIN persons p ON p.id = eg.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = p.id
            WHERE eg.episode_id = ANY($1)
            ORDER BY eg.episode_id, eg.order_index"#,
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to load episode guest stars: {}",
            e
        ))
    })?;

    for record in guest_rows {
        let image_slot = record.order_index.unwrap_or_default() as u32;
        let member = CastMember {
            id: record.person_tmdb_id as u64,
            person_id: Some(record.id),
            credit_id: record.credit_id,
            cast_id: None,
            name: record.name.clone(),
            original_name: record.original_name,
            character: record.character,
            profile_path: record.profile_path.clone(),
            order: image_slot,
            gender: record.gender.map(|g| g as u8),
            known_for_department: record.known_for_department.clone(),
            adult: record.adult,
            popularity: record.popularity,
            also_known_as: record.aliases.clone(),
            external_ids: build_person_external_ids(
                record.imdb_id,
                record.facebook_id,
                record.instagram_id,
                record.twitter_id,
                record.wikidata_id,
                record.tiktok_id,
                record.youtube_id,
            ),
            image_slot,
            image_id: record.profile_iid,
        };

        guest_map
            .entry(record.episode_id)
            .or_default()
            .entry(member.id)
            .or_insert(member);
    }

    let crew_rows = sqlx::query!(
        r#"SELECT
                ec.episode_id,
                ec.person_tmdb_id,
                ec.credit_id,
                ec.department,
                ec.job,
                p.id,
                p.name,
                p.original_name,
                p.profile_path,
                p.gender,
                p.known_for_department,
                p.adult,
                p.popularity,
                p.imdb_id,
                p.facebook_id,
                p.instagram_id,
                p.twitter_id,
                p.wikidata_id,
                p.tiktok_id,
                p.youtube_id,
                COALESCE(alias_data.aliases, ARRAY[]::TEXT[]) AS "aliases!: Vec<String>"
            FROM episode_crew ec
            JOIN persons p ON p.id = ec.person_id
            LEFT JOIN (
                SELECT person_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY person_id
            ) alias_data ON alias_data.person_id = p.id
            WHERE ec.episode_id = ANY($1)"#,
        episode_ids
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load episode crew: {}", e))
    })?;

    let mut crew_map = HashMap::new();
    for record in crew_rows {
        push_grouped(
            &mut crew_map,
            record.episode_id,
            CrewMember {
                id: record.person_tmdb_id as u64,
                person_id: Some(record.id),
                credit_id: record.credit_id,
                name: record.name.clone(),
                job: record.job,
                department: record.department,
                profile_path: record.profile_path.clone(),
                gender: record.gender.map(|g| g as u8),
                known_for_department: record.known_for_department.clone(),
                adult: record.adult,
                popularity: record.popularity,
                original_name: record.original_name,
                also_known_as: record.aliases,
                external_ids: build_person_external_ids(
                    record.imdb_id,
                    record.facebook_id,
                    record.instagram_id,
                    record.twitter_id,
                    record.wikidata_id,
                    record.tiktok_id,
                    record.youtube_id,
                ),
                profile_iid: None,
            },
        );
    }

    let mut details_map = HashMap::with_capacity(metadata_map.len());
    for (episode_id, row) in metadata_map {
        let mut guest_stars: Vec<CastMember> = guest_map
            .remove(&episode_id)
            .unwrap_or_default()
            .into_values()
            .collect();
        guest_stars.sort_by_key(|member| member.image_slot);

        let episode_number = row.episode_number.ok_or_else(|| {
            MediaError::NotFound(format!(
                "Episode metadata missing episode_number for id {:#?}",
                episode_id
            ))
        })? as u16;
        let season_number = row.season_number.ok_or_else(|| {
            MediaError::NotFound(format!(
                "Episode metadata missing season_number for id {:#?}",
                episode_id
            ))
        })? as u16;
        let name = row.name.ok_or_else(|| {
            MediaError::NotFound(format!(
                "Episode metadata missing name for id {:#?}",
                episode_id
            ))
        })?;

        let details = EpisodeDetails {
            id: row.tmdb_id as u64,
            episode_number,
            season_number,
            name,
            overview: row.overview,
            air_date: row.air_date.map(|d| d.to_string()),
            runtime: row.runtime.map(|r| r as u32),
            still_path: row.still_path,
            primary_still_iid: row.primary_thumbnail_image_id,
            vote_average: row.vote_average,
            vote_count: row.vote_count.map(|v| v as u32),
            production_code: row.production_code,
            external_ids: ExternalIds {
                imdb_id: row.imdb_id,
                tvdb_id: row.tvdb_id.map(|id| id as u32),
                facebook_id: row.facebook_id,
                instagram_id: row.instagram_id,
                twitter_id: row.twitter_id,
                wikidata_id: row.wikidata_id,
                tiktok_id: None,
                youtube_id: None,
                freebase_id: None,
                freebase_mid: None,
            },
            images: MediaImages::default(),
            videos: videos_map.remove(&episode_id).unwrap_or_default(),
            keywords: keywords_map.remove(&episode_id).unwrap_or_default(),
            translations: translations_map
                .remove(&episode_id)
                .unwrap_or_default(),
            guest_stars,
            crew: crew_map.remove(&episode_id).unwrap_or_default(),
            content_ratings: content_ratings_map
                .remove(&episode_id)
                .unwrap_or_default(),
        };

        details_map.insert(episode_id, details);
    }

    Ok(details_map)
}

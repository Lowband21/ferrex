use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::{
    AlternativeTitle, CastMember, CollectionInfo, ContentRating, CrewMember, EnhancedMovieDetails,
    EnhancedSeriesDetails, EpisodeDetails, EpisodeGroupMembership, EpisodeID, EpisodeNumber,
    EpisodeReference, EpisodeURL, ExternalIds, GenreInfo, Keyword, LibraryID, MediaDetailsOption,
    MediaError, MediaFile, MediaIDLike, MovieID, MovieReference, MovieTitle, MovieURL, NetworkInfo,
    PersonExternalIds, ProductionCompany, ProductionCountry, RelatedMediaRef, ReleaseDateEntry,
    ReleaseDatesByCountry, Result, SeasonDetails, SeasonID, SeasonNumber, SeasonReference,
    SeasonURL, SeriesID, SeriesReference, SeriesTitle, SeriesURL, SpokenLanguage, TmdbDetails,
    Translation, UrlLike, Video,
};

/// Primary entrypoint for TMDB metadata persistence.
pub struct TmdbMetadataRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> fmt::Debug for TmdbMetadataRepository<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TmdbMetadataRepository")
            .field("pool_size", &self.pool.size())
            .field("pool_idle", &self.pool.num_idle())
            .finish()
    }
}

impl<'a> TmdbMetadataRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    pub async fn store_movie_reference(&self, movie: &MovieReference) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "Failed to begin transaction for movie reference {}: {}",
                movie.id, e
            ))
        })?;

        let actual_file_id = store_media_file(&mut tx, &movie.file).await?;

        let target_movie_id = if movie.tmdb_id == 0 {
            upsert_local_movie_reference(&mut tx, movie, actual_file_id).await?
        } else {
            upsert_tmdb_movie_reference(&mut tx, movie, actual_file_id).await?
        };

        if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &movie.details {
            persist_movie_metadata(&mut tx, target_movie_id, movie.tmdb_id, details).await?;
        } else if movie.tmdb_id != 0 {
            // TMDB id present but no metadata; ensure relations removed
            purge_movie_metadata(&mut tx, target_movie_id).await?;
        }

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit movie transaction: {}", e)))
    }

    pub async fn store_series_reference(&self, series: &SeriesReference) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "Failed to begin transaction for series reference {}: {}",
                series.id, e
            ))
        })?;

        let target_series_id = upsert_series_reference(&mut tx, series).await?;

        if let MediaDetailsOption::Details(TmdbDetails::Series(details)) = &series.details {
            persist_series_metadata(&mut tx, target_series_id, series.tmdb_id, details).await?;
        } else if series.tmdb_id != 0 {
            purge_series_metadata(&mut tx, target_series_id).await?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit series transaction: {}", e))
        })
    }

    pub async fn store_season_reference(&self, season: &SeasonReference) -> Result<Uuid> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "Failed to begin transaction for season reference {}: {}",
                season.id, e
            ))
        })?;

        let season_id = upsert_season_reference(&mut tx, season).await?;

        if let MediaDetailsOption::Details(TmdbDetails::Season(details)) = &season.details {
            persist_season_metadata(&mut tx, season_id, season.tmdb_series_id, details).await?;
        } else {
            purge_season_metadata(&mut tx, season_id).await?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit season transaction: {}", e))
        })?;

        Ok(season_id)
    }

    pub async fn store_episode_reference(&self, episode: &EpisodeReference) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            MediaError::Internal(format!(
                "Failed to begin transaction for episode reference {}: {}",
                episode.id, e
            ))
        })?;

        let actual_file_id = store_media_file(&mut tx, &episode.file).await?;

        let episode_uuid = episode.id.to_uuid();
        let insert_id = sqlx::query_scalar!(
            r#"
            INSERT INTO episode_references (
                id, season_id, series_id, tmdb_series_id,
                episode_number, season_number, file_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (series_id, season_number, episode_number) DO UPDATE SET
                tmdb_series_id = EXCLUDED.tmdb_series_id,
                file_id = EXCLUDED.file_id,
                updated_at = NOW()
            RETURNING id
            "#,
            episode_uuid,
            episode.season_id.to_uuid(),
            episode.series_id.to_uuid(),
            episode.tmdb_series_id as i64,
            episode.episode_number.value() as i32,
            episode.season_number.value() as i32,
            actual_file_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to upsert episode reference: {}", e)))?;

        if let MediaDetailsOption::Details(TmdbDetails::Episode(details)) = &episode.details {
            persist_episode_metadata(&mut tx, insert_id, episode.tmdb_series_id, details).await?;
        } else {
            purge_episode_metadata(&mut tx, insert_id).await?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit episode transaction: {}", e))
        })
    }

    pub async fn load_movie_reference(&self, row: PgRow) -> Result<MovieReference> {
        let movie_id = row.try_get::<Uuid, _>("id")?;
        let file_id = row.try_get::<Uuid, _>("file_id")?;

        let tmdb_id: i64 = row.try_get("tmdb_id")?;
        let title: String = row.try_get("title")?;
        let theme_color: Option<String> = row.try_get("theme_color")?;
        let media_file = hydrate_media_file_row(&row)?;

        let details = load_movie_details(self.pool, movie_id).await?;

        Ok(MovieReference {
            id: MovieID(movie_id),
            library_id: media_file.library_id,
            tmdb_id: tmdb_id as u64,
            title: MovieTitle::new(title.clone()).map_err(|e| {
                MediaError::Internal(format!("Invalid stored movie title '{}': {}", title, e))
            })?,
            details: details
                .map(|d| MediaDetailsOption::Details(TmdbDetails::Movie(d)))
                .unwrap_or_else(|| MediaDetailsOption::Endpoint(format!("/movie/{}", movie_id))),
            endpoint: MovieURL::from_string(format!("/stream/{}", file_id)),
            file: media_file,
            theme_color,
        })
    }

    pub async fn load_series_reference(&self, row: PgRow) -> Result<SeriesReference> {
        let series_id = row.try_get::<Uuid, _>("id")?;
        let library_id = row.try_get::<Uuid, _>("library_id")?;
        let tmdb_id: Option<i64> = row.try_get("tmdb_id")?;
        let title: String = row.try_get("title")?;
        let theme_color: Option<String> = row.try_get("theme_color")?;
        let discovered_at: chrono::DateTime<chrono::Utc> = row.try_get("discovered_at")?;
        let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at")?;

        let details = load_series_details(self.pool, series_id).await?;

        Ok(SeriesReference {
            id: SeriesID(series_id),
            library_id: crate::LibraryID(library_id),
            tmdb_id: tmdb_id.unwrap_or_default() as u64,
            title: SeriesTitle::new(title.clone()).map_err(|e| {
                MediaError::Internal(format!("Invalid stored series title '{}': {}", title, e))
            })?,
            details: details
                .map(|d| MediaDetailsOption::Details(TmdbDetails::Series(d)))
                .unwrap_or_else(|| MediaDetailsOption::Endpoint(format!("/series/{}", series_id))),
            endpoint: SeriesURL::from_string(format!("/series/{}", series_id)),
            discovered_at,
            created_at,
            theme_color,
        })
    }

    pub async fn load_season_reference(&self, row: PgRow) -> Result<SeasonReference> {
        let season_id = row.try_get::<Uuid, _>("id")?;
        let series_id = row.try_get::<Uuid, _>("series_id")?;
        let library_id = row.try_get::<Uuid, _>("library_id")?;
        let season_number = row.try_get::<i16, _>("season_number")? as u8;
        let tmdb_series_id: i64 = row.try_get("tmdb_series_id")?;
        let discovered_at: chrono::DateTime<chrono::Utc> = row.try_get("discovered_at")?;
        let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at")?;
        let theme_color = row
            .try_get::<Option<String>, _>("theme_color")
            .unwrap_or(None);

        let details = load_season_details(self.pool, season_id).await?;
        let details_option = details
            .map(|d| MediaDetailsOption::Details(TmdbDetails::Season(d)))
            .unwrap_or_else(|| MediaDetailsOption::Endpoint(format!("/media/{}", season_id)));

        Ok(SeasonReference {
            id: SeasonID(season_id),
            library_id: LibraryID(library_id),
            season_number: SeasonNumber::new(season_number),
            series_id: SeriesID(series_id),
            tmdb_series_id: tmdb_series_id as u64,
            details: details_option,
            endpoint: SeasonURL::from_string(format!("/media/{}", season_id)),
            discovered_at,
            created_at,
            theme_color,
        })
    }

    pub async fn load_episode_reference(&self, row: PgRow) -> Result<EpisodeReference> {
        let episode_id = row.try_get::<Uuid, _>("id")?;
        let season_id = row.try_get::<Uuid, _>("season_id")?;
        let series_id = row.try_get::<Uuid, _>("series_id")?;
        let episode_number = row.try_get::<i16, _>("episode_number")? as u8;
        let season_number = row.try_get::<i16, _>("season_number")? as u8;
        let tmdb_series_id: i64 = row.try_get("tmdb_series_id")?;
        // Episode-level timestamps (distinct from media file timestamps)
        let discovered_at: chrono::DateTime<chrono::Utc> = row
            .try_get("episode_discovered_at")
            .unwrap_or_else(|_| chrono::Utc::now());
        let created_at: chrono::DateTime<chrono::Utc> = row
            .try_get("episode_created_at")
            .unwrap_or_else(|_| chrono::Utc::now());

        let media_file = hydrate_media_file_row(&row)?;

        let details = load_episode_details(self.pool, episode_id).await?;
        let details_option = details
            .map(|d| MediaDetailsOption::Details(TmdbDetails::Episode(d)))
            .unwrap_or_else(|| MediaDetailsOption::Endpoint(format!("/media/{}", episode_id)));

        Ok(EpisodeReference {
            id: EpisodeID(episode_id),
            library_id: media_file.library_id,
            episode_number: EpisodeNumber::new(episode_number),
            season_number: SeasonNumber::new(season_number),
            season_id: SeasonID(season_id),
            series_id: SeriesID(series_id),
            tmdb_series_id: tmdb_series_id as u64,
            details: details_option,
            endpoint: EpisodeURL::from_string(format!("/stream/{}", media_file.id)),
            file: media_file,
            discovered_at,
            created_at,
        })
    }
}

/// Persist movie metadata and associations.
async fn persist_movie_metadata(
    tx: &mut Transaction<'_, Postgres>,
    movie_id: Uuid,
    tmdb_id: u64,
    details: &EnhancedMovieDetails,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO movie_metadata (
            movie_id, tmdb_id, title, original_title, overview, release_date, runtime,
            vote_average, vote_count, popularity, primary_certification, homepage, status,
            tagline, budget, revenue, poster_path, backdrop_path, logo_path, collection_id,
            collection_name, collection_poster_path, collection_backdrop_path,
            imdb_id, facebook_id, instagram_id, twitter_id, wikidata_id, tiktok_id, youtube_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10, $11, $12, $13,
            $14, $15, $16, $17, $18, $19, $20,
            $21, $22, $23,
            $24, $25, $26, $27, $28, $29, $30
        )
        ON CONFLICT (movie_id) DO UPDATE SET
            tmdb_id = EXCLUDED.tmdb_id,
            title = EXCLUDED.title,
            original_title = EXCLUDED.original_title,
            overview = EXCLUDED.overview,
            release_date = EXCLUDED.release_date,
            runtime = EXCLUDED.runtime,
            vote_average = EXCLUDED.vote_average,
            vote_count = EXCLUDED.vote_count,
            popularity = EXCLUDED.popularity,
            primary_certification = EXCLUDED.primary_certification,
            homepage = EXCLUDED.homepage,
            status = EXCLUDED.status,
            tagline = EXCLUDED.tagline,
            budget = EXCLUDED.budget,
            revenue = EXCLUDED.revenue,
            poster_path = EXCLUDED.poster_path,
            backdrop_path = EXCLUDED.backdrop_path,
            logo_path = EXCLUDED.logo_path,
            collection_id = EXCLUDED.collection_id,
            collection_name = EXCLUDED.collection_name,
            collection_poster_path = EXCLUDED.collection_poster_path,
            collection_backdrop_path = EXCLUDED.collection_backdrop_path,
            imdb_id = EXCLUDED.imdb_id,
            facebook_id = EXCLUDED.facebook_id,
            instagram_id = EXCLUDED.instagram_id,
            twitter_id = EXCLUDED.twitter_id,
            wikidata_id = EXCLUDED.wikidata_id,
            tiktok_id = EXCLUDED.tiktok_id,
            youtube_id = EXCLUDED.youtube_id,
            updated_at = NOW()
        "#,
        movie_id,
        tmdb_id as i64,
        details.title,
        details.original_title,
        details.overview,
        details
            .release_date
            .as_ref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        details.runtime.map(|r| r as i32),
        details.vote_average,
        details.vote_count.map(|c| c as i32),
        details.popularity,
        details.content_rating,
        details.homepage,
        details.status,
        details.tagline,
        details.budget.map(|b| b as i64),
        details.revenue.map(|r| r as i64),
        details.poster_path,
        details.backdrop_path,
        details.logo_path,
        details.collection.as_ref().map(|c| c.id as i64),
        details.collection.as_ref().map(|c| c.name.clone()),
        details
            .collection
            .as_ref()
            .and_then(|c| c.poster_path.clone()),
        details
            .collection
            .as_ref()
            .and_then(|c| c.backdrop_path.clone()),
        details.external_ids.imdb_id.clone(),
        details.external_ids.facebook_id.clone(),
        details.external_ids.instagram_id.clone(),
        details.external_ids.twitter_id.clone(),
        details.external_ids.wikidata_id.clone(),
        details.external_ids.tiktok_id.clone(),
        details.external_ids.youtube_id.clone()
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert movie metadata: {}", e)))?;

    sync_movie_child_tables(tx, movie_id, details).await
}

async fn persist_series_metadata(
    tx: &mut Transaction<'_, Postgres>,
    series_id: Uuid,
    tmdb_id: u64,
    details: &EnhancedSeriesDetails,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO series_metadata (
            series_id, tmdb_id, name, original_name, overview, first_air_date, last_air_date,
            number_of_seasons, number_of_episodes, vote_average, vote_count, popularity,
            primary_content_rating, homepage, status, tagline, in_production,
            poster_path, backdrop_path, logo_path,
            imdb_id, tvdb_id, facebook_id, instagram_id, twitter_id, wikidata_id, tiktok_id, youtube_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10, $11, $12,
            $13, $14, $15, $16, $17,
            $18, $19, $20,
            $21, $22, $23, $24, $25, $26, $27, $28
        )
        ON CONFLICT (series_id) DO UPDATE SET
            tmdb_id = EXCLUDED.tmdb_id,
            name = EXCLUDED.name,
            original_name = EXCLUDED.original_name,
            overview = EXCLUDED.overview,
            first_air_date = EXCLUDED.first_air_date,
            last_air_date = EXCLUDED.last_air_date,
            number_of_seasons = EXCLUDED.number_of_seasons,
            number_of_episodes = EXCLUDED.number_of_episodes,
            vote_average = EXCLUDED.vote_average,
            vote_count = EXCLUDED.vote_count,
            popularity = EXCLUDED.popularity,
            primary_content_rating = EXCLUDED.primary_content_rating,
            homepage = EXCLUDED.homepage,
            status = EXCLUDED.status,
            tagline = EXCLUDED.tagline,
            in_production = EXCLUDED.in_production,
            poster_path = EXCLUDED.poster_path,
            backdrop_path = EXCLUDED.backdrop_path,
            logo_path = EXCLUDED.logo_path,
            imdb_id = EXCLUDED.imdb_id,
            tvdb_id = EXCLUDED.tvdb_id,
            facebook_id = EXCLUDED.facebook_id,
            instagram_id = EXCLUDED.instagram_id,
            twitter_id = EXCLUDED.twitter_id,
            wikidata_id = EXCLUDED.wikidata_id,
            tiktok_id = EXCLUDED.tiktok_id,
            youtube_id = EXCLUDED.youtube_id,
            updated_at = NOW()
        "#,
        series_id,
        tmdb_id as i64,
        details.name,
        details.original_name,
        details.overview,
        details
            .first_air_date
            .as_ref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        details
            .last_air_date
            .as_ref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        details.number_of_seasons.map(|n| n as i32),
        details.number_of_episodes.map(|n| n as i32),
        details.vote_average,
        details.vote_count.map(|v| v as i32),
        details.popularity,
        details.content_rating,
        details.homepage,
        details.status,
        details.tagline,
        details.in_production,
        details.poster_path,
        details.backdrop_path,
        details.logo_path,
        details.external_ids.imdb_id.clone(),
        details.external_ids.tvdb_id.map(|id| id as i64),
        details.external_ids.facebook_id.clone(),
        details.external_ids.instagram_id.clone(),
        details.external_ids.twitter_id.clone(),
        details.external_ids.wikidata_id.clone(),
        details.external_ids.tiktok_id.clone(),
        details.external_ids.youtube_id.clone()
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert series metadata: {}", e)))?;

    sync_series_child_tables(tx, series_id, details).await
}

async fn persist_season_metadata(
    tx: &mut Transaction<'_, Postgres>,
    season_id: Uuid,
    tmdb_series_id: u64,
    details: &SeasonDetails,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO season_metadata (
            season_id, tmdb_id, series_tmdb_id, name, overview, air_date,
            episode_count, poster_path, runtime, vote_average, vote_count,
            imdb_id, facebook_id, instagram_id, twitter_id, wikidata_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $6,
            $7, $8, $9, $10, $11,
            $12, $13, $14, $15, $16
        )
        ON CONFLICT (season_id) DO UPDATE SET
            tmdb_id = EXCLUDED.tmdb_id,
            series_tmdb_id = EXCLUDED.series_tmdb_id,
            name = EXCLUDED.name,
            overview = EXCLUDED.overview,
            air_date = EXCLUDED.air_date,
            episode_count = EXCLUDED.episode_count,
            poster_path = EXCLUDED.poster_path,
            runtime = EXCLUDED.runtime,
            vote_average = EXCLUDED.vote_average,
            vote_count = EXCLUDED.vote_count,
            imdb_id = EXCLUDED.imdb_id,
            facebook_id = EXCLUDED.facebook_id,
            instagram_id = EXCLUDED.instagram_id,
            twitter_id = EXCLUDED.twitter_id,
            wikidata_id = EXCLUDED.wikidata_id,
            updated_at = NOW()
        "#,
        season_id,
        details.id as i64,
        tmdb_series_id as i64,
        details.name,
        details.overview,
        details
            .air_date
            .as_ref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        details.episode_count as i32,
        details.poster_path.clone(),
        details.runtime.map(|r| r as i32),
        None::<f32>,
        None::<i32>,
        details.external_ids.imdb_id.clone(),
        details.external_ids.facebook_id.clone(),
        details.external_ids.instagram_id.clone(),
        details.external_ids.twitter_id.clone(),
        details.external_ids.wikidata_id.clone()
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert season metadata: {}", e)))?;

    sqlx::query!(
        "DELETE FROM season_keywords WHERE season_id = $1",
        season_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear season keywords: {}", e)))?;

    for keyword in &details.keywords {
        sqlx::query!(
            "INSERT INTO season_keywords (season_id, keyword_id, name) VALUES ($1, $2, $3)",
            season_id,
            keyword.id as i64,
            keyword.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert season keyword: {}", e)))?;
    }

    sqlx::query!("DELETE FROM season_videos WHERE season_id = $1", season_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear season videos: {}", e)))?;

    for video in &details.videos {
        sqlx::query!(
            r#"INSERT INTO season_videos (
                season_id, video_key, site, name, video_type, official,
                iso_639_1, iso_3166_1, published_at, size
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
            season_id,
            video.key,
            video.site,
            video.name,
            video.video_type,
            video.official,
            video.iso_639_1,
            video.iso_3166_1,
            video
                .published_at
                .as_ref()
                .and_then(|p| chrono::DateTime::parse_from_rfc3339(p).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            video.size.map(|s| s as i32)
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert season video: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM season_translations WHERE season_id = $1",
        season_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear season translations: {}", e)))?;

    for translation in &details.translations {
        sqlx::query!(
            r#"INSERT INTO season_translations (
                season_id, iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
            season_id,
            translation.iso_3166_1,
            translation.iso_639_1,
            translation.name,
            translation.english_name,
            translation.title,
            translation.overview,
            translation.homepage,
            translation.tagline
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert season translation: {}", e)))?;
    }

    Ok(())
}

async fn persist_episode_metadata(
    tx: &mut Transaction<'_, Postgres>,
    episode_id: Uuid,
    series_tmdb_id: u64,
    details: &EpisodeDetails,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO episode_metadata (
            episode_id, tmdb_id, series_tmdb_id, season_tmdb_id,
            season_number, episode_number, name, overview, air_date,
            runtime, still_path, vote_average, vote_count, production_code,
            imdb_id, tvdb_id, facebook_id, instagram_id, twitter_id, wikidata_id
        )
        VALUES (
            $1, $2, $3, $4,
            $5, $6, $7, $8, $9,
            $10, $11, $12, $13, $14,
            $15, $16, $17, $18, $19, $20
        )
        ON CONFLICT (episode_id) DO UPDATE SET
            tmdb_id = EXCLUDED.tmdb_id,
            series_tmdb_id = EXCLUDED.series_tmdb_id,
            season_tmdb_id = EXCLUDED.season_tmdb_id,
            season_number = EXCLUDED.season_number,
            episode_number = EXCLUDED.episode_number,
            name = EXCLUDED.name,
            overview = EXCLUDED.overview,
            air_date = EXCLUDED.air_date,
            runtime = EXCLUDED.runtime,
            still_path = EXCLUDED.still_path,
            vote_average = EXCLUDED.vote_average,
            vote_count = EXCLUDED.vote_count,
            production_code = EXCLUDED.production_code,
            imdb_id = EXCLUDED.imdb_id,
            tvdb_id = EXCLUDED.tvdb_id,
            facebook_id = EXCLUDED.facebook_id,
            instagram_id = EXCLUDED.instagram_id,
            twitter_id = EXCLUDED.twitter_id,
            wikidata_id = EXCLUDED.wikidata_id,
            updated_at = NOW()
        "#,
        episode_id,
        details.id as i64,
        series_tmdb_id as i64,
        None::<i64>,
        details.season_number as i32,
        details.episode_number as i32,
        details.name,
        details.overview,
        details
            .air_date
            .as_ref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        details.runtime.map(|r| r as i32),
        details.still_path.clone(),
        details.vote_average,
        details.vote_count.map(|v| v as i32),
        details.production_code.clone(),
        details.external_ids.imdb_id.clone(),
        details.external_ids.tvdb_id.map(|id| id as i64),
        details.external_ids.facebook_id.clone(),
        details.external_ids.instagram_id.clone(),
        details.external_ids.twitter_id.clone(),
        details.external_ids.wikidata_id.clone()
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert episode metadata: {}", e)))?;

    sync_episode_child_tables(tx, episode_id, details).await
}

async fn purge_movie_metadata(tx: &mut Transaction<'_, Postgres>, movie_id: Uuid) -> Result<()> {
    sqlx::query!("DELETE FROM movie_metadata WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to purge movie metadata: {}", e)))?;
    Ok(())
}

async fn purge_series_metadata(tx: &mut Transaction<'_, Postgres>, series_id: Uuid) -> Result<()> {
    sqlx::query!(
        "DELETE FROM series_metadata WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to purge series metadata: {}", e)))?;
    Ok(())
}

async fn purge_season_metadata(tx: &mut Transaction<'_, Postgres>, season_id: Uuid) -> Result<()> {
    sqlx::query!(
        "DELETE FROM season_metadata WHERE season_id = $1",
        season_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to purge season metadata: {}", e)))?;
    Ok(())
}

async fn purge_episode_metadata(
    tx: &mut Transaction<'_, Postgres>,
    episode_id: Uuid,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM episode_metadata WHERE episode_id = $1",
        episode_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to purge episode metadata: {}", e)))?;
    Ok(())
}

async fn sync_movie_child_tables(
    tx: &mut Transaction<'_, Postgres>,
    movie_id: Uuid,
    details: &EnhancedMovieDetails,
) -> Result<()> {
    sqlx::query!("DELETE FROM movie_genres WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear movie genres: {}", e)))?;

    for genre in &details.genres {
        sqlx::query!(
            r#"INSERT INTO movie_genres (movie_id, genre_id, name)
               VALUES ($1, $2, $3)
               ON CONFLICT (movie_id, genre_id) DO NOTHING"#,
            movie_id,
            genre.id as i64,
            genre.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie genre: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM movie_spoken_languages WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear movie spoken languages: {}", e)))?;

    for language in &details.spoken_languages {
        sqlx::query!(
            r#"INSERT INTO movie_spoken_languages (movie_id, iso_639_1, name)
               VALUES ($1, $2, $3)
               ON CONFLICT (movie_id, name) DO NOTHING"#,
            movie_id,
            language.iso_639_1.clone(),
            language.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie language: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM movie_production_companies WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear movie companies: {}", e)))?;

    for company in &details.production_companies {
        sqlx::query!(
            r#"INSERT INTO movie_production_companies (movie_id, company_id, name, origin_country)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (movie_id, name) DO NOTHING"#,
            movie_id,
            company.id as i64,
            company.name,
            company.origin_country
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie company: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM movie_production_countries WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear movie countries: {}", e)))?;

    for country in &details.production_countries {
        sqlx::query!(
            r#"INSERT INTO movie_production_countries (movie_id, iso_3166_1, name)
               VALUES ($1, $2, $3)
               ON CONFLICT (movie_id, iso_3166_1) DO NOTHING"#,
            movie_id,
            country.iso_3166_1,
            country.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie country: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM movie_release_dates WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear movie release dates: {}", e)))?;

    for country in &details.release_dates {
        for entry in &country.release_dates {
            sqlx::query!(
                r#"INSERT INTO movie_release_dates (
                    movie_id, iso_3166_1, iso_639_1, certification,
                    release_date, release_type, note, descriptors
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (movie_id, iso_3166_1, release_type, release_date) DO NOTHING"#,
                movie_id,
                country.iso_3166_1,
                entry.iso_639_1,
                entry.certification,
                entry
                    .release_date
                    .as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc)),
                entry.release_type.map(|t| t as i16),
                entry.note,
                &entry.descriptors
            )
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to insert movie release date: {}", e))
            })?;
        }
    }

    sqlx::query!(
        "DELETE FROM movie_alternative_titles WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear alternative titles: {}", e)))?;

    let mut seen_alternative_titles: HashSet<(String, String, String)> = HashSet::new();

    for title in &details.alternative_titles {
        let iso = title
            .iso_3166_1
            .as_ref()
            .map(|val| val.trim())
            .filter(|val| !val.is_empty())
            .map(|val| val.to_string());

        let title_type = title
            .title_type
            .as_ref()
            .map(|val| val.trim())
            .filter(|val| !val.is_empty())
            .map(|val| val.to_string());

        let title_value = title.title.trim();
        if title_value.is_empty() {
            continue;
        }

        let title_value = title_value.to_string();
        let key = (
            iso.clone().unwrap_or_default(),
            title_type.clone().unwrap_or_default(),
            title_value.clone(),
        );

        if !seen_alternative_titles.insert(key) {
            continue;
        }

        sqlx::query!(
            r#"INSERT INTO movie_alternative_titles (movie_id, iso_3166_1, title, title_type)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT DO NOTHING"#,
            movie_id,
            iso.as_deref(),
            &title_value,
            title_type.as_deref()
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert alternative title: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM movie_translations WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear movie translations: {}", e)))?;

    for translation in &details.translations {
        sqlx::query!(
            r#"INSERT INTO movie_translations (
                movie_id, iso_3166_1, iso_639_1, name, english_name,
                title, overview, homepage, tagline
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (movie_id, iso_3166_1, iso_639_1) DO NOTHING"#,
            movie_id,
            translation.iso_3166_1,
            translation.iso_639_1,
            translation.name,
            translation.english_name,
            translation.title,
            translation.overview,
            translation.homepage,
            translation.tagline
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie translation: {}", e)))?;
    }

    sqlx::query!("DELETE FROM movie_videos WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear movie videos: {}", e)))?;

    for video in &details.videos {
        sqlx::query!(
            r#"INSERT INTO movie_videos (
                movie_id, video_key, site, name, video_type, official,
                iso_639_1, iso_3166_1, published_at, size)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (movie_id, video_key, site) DO NOTHING"#,
            movie_id,
            video.key,
            video.site,
            video.name,
            video.video_type,
            video.official,
            video.iso_639_1,
            video.iso_3166_1,
            video
                .published_at
                .as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            video.size.map(|s| s as i32)
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie video: {}", e)))?;
    }

    sqlx::query!("DELETE FROM movie_keywords WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear movie keywords: {}", e)))?;

    for keyword in &details.keywords {
        sqlx::query!(
            r#"INSERT INTO movie_keywords (movie_id, keyword_id, name)
               VALUES ($1, $2, $3)
               ON CONFLICT (movie_id, keyword_id) DO NOTHING"#,
            movie_id,
            keyword.id as i64,
            keyword.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie keyword: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM movie_recommendations WHERE movie_id = $1",
        movie_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear movie recommendations: {}", e)))?;

    for recommendation in &details.recommendations {
        sqlx::query!(
            r#"INSERT INTO movie_recommendations (movie_id, recommended_tmdb_id, title)
               VALUES ($1, $2, $3)
               ON CONFLICT (movie_id, recommended_tmdb_id) DO NOTHING"#,
            movie_id,
            recommendation.tmdb_id as i64,
            recommendation.title
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to insert movie recommendation: {}", e))
        })?;
    }

    sqlx::query!("DELETE FROM movie_similar WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear similar movies: {}", e)))?;

    for similar in &details.similar {
        sqlx::query!(
            r#"INSERT INTO movie_similar (movie_id, similar_tmdb_id, title)
               VALUES ($1, $2, $3)
               ON CONFLICT (movie_id, similar_tmdb_id) DO NOTHING"#,
            movie_id,
            similar.tmdb_id as i64,
            similar.title
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert similar movie: {}", e)))?;
    }

    if let Some(collection) = &details.collection {
        sqlx::query!(
            r#"INSERT INTO movie_collection_membership (
                movie_id, collection_id, name, poster_path, backdrop_path
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (movie_id) DO UPDATE SET
                collection_id = EXCLUDED.collection_id,
                name = EXCLUDED.name,
                poster_path = EXCLUDED.poster_path,
                backdrop_path = EXCLUDED.backdrop_path
            "#,
            movie_id,
            collection.id as i64,
            collection.name,
            collection.poster_path,
            collection.backdrop_path
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to upsert movie collection: {}", e)))?;
    } else {
        sqlx::query!(
            "DELETE FROM movie_collection_membership WHERE movie_id = $1",
            movie_id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear movie collection membership: {}",
                e
            ))
        })?;
    }

    sync_movie_people(tx, movie_id, &details.cast, &details.crew).await
}

async fn upsert_profile_image_id(
    tx: &mut Transaction<'_, Postgres>,
    tmdb_path: &str,
) -> Result<Uuid> {
    let row = sqlx::query!(
        r#"
        INSERT INTO images (tmdb_path, created_at, updated_at)
        VALUES ($1, NOW(), NOW())
        ON CONFLICT (tmdb_path) DO UPDATE SET updated_at = EXCLUDED.updated_at
        RETURNING id
        "#,
        tmdb_path
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to upsert profile image {}: {}",
            tmdb_path, e
        ))
    })?;

    Ok(row.id)
}

async fn ensure_profile_image_id(
    tx: &mut Transaction<'_, Postgres>,
    cache: &mut HashMap<u64, Option<Uuid>>,
    member: &impl PersonLike,
) -> Result<Option<Uuid>> {
    if let Some(cached) = cache.get(&member.id()) {
        return Ok(*cached);
    }

    let image_id = match member.profile_path() {
        Some(path) => {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(upsert_profile_image_id(tx, trimmed).await?)
            }
        }
        None => None,
    };

    cache.insert(member.id(), image_id);
    Ok(image_id)
}

async fn sync_movie_people(
    tx: &mut Transaction<'_, Postgres>,
    movie_id: Uuid,
    cast: &[CastMember],
    crew: &[CrewMember],
) -> Result<()> {
    let mut profile_cache: HashMap<u64, Option<Uuid>> = HashMap::new();
    let mut processed_people: HashSet<u64> = HashSet::new();
    let mut seen_cast: HashSet<(u64, String)> = HashSet::new();
    let mut seen_crew: HashSet<(u64, String, String)> = HashSet::new();

    for member in cast {
        if processed_people.insert(member.id) {
            upsert_person(tx, member.id, member).await?;
        }
    }
    for member in crew {
        if processed_people.insert(member.id) {
            upsert_person(tx, member.id, member).await?;
        }
    }

    sqlx::query!("DELETE FROM movie_cast WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear movie cast: {}", e)))?;

    for member in cast {
        let character = member.character.trim().to_string();
        if !seen_cast.insert((member.id, character.clone())) {
            continue;
        }

        let profile_image_id = ensure_profile_image_id(tx, &mut profile_cache, member).await?;
        sqlx::query!(
            r#"INSERT INTO movie_cast (
                movie_id, person_tmdb_id, credit_id, cast_id, character, order_index, profile_image_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (movie_id, person_tmdb_id, character) DO NOTHING"#,
            movie_id,
            member.id as i64,
            member.credit_id.clone(),
            member.cast_id.map(|id| id as i64),
            &character,
            member.image_slot as i32,
            profile_image_id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie cast member: {}", e)))?;
    }

    sqlx::query!("DELETE FROM movie_crew WHERE movie_id = $1", movie_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear movie crew: {}", e)))?;

    for member in crew {
        let department = member.department.trim().to_string();
        let job = member.job.trim().to_string();
        if !seen_crew.insert((member.id, department.clone(), job.clone())) {
            continue;
        }

        sqlx::query!(
            r#"INSERT INTO movie_crew (
                movie_id, person_tmdb_id, credit_id, department, job
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (movie_id, person_tmdb_id, department, job) DO NOTHING"#,
            movie_id,
            member.id as i64,
            member.credit_id.clone(),
            &department,
            &job
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie crew member: {}", e)))?;
    }

    Ok(())
}

async fn sync_series_child_tables(
    tx: &mut Transaction<'_, Postgres>,
    series_id: Uuid,
    details: &EnhancedSeriesDetails,
) -> Result<()> {
    sqlx::query!("DELETE FROM series_genres WHERE series_id = $1", series_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear series genres: {}", e)))?;

    for genre in &details.genres {
        sqlx::query!(
            "INSERT INTO series_genres (series_id, genre_id, name) VALUES ($1, $2, $3)",
            series_id,
            genre.id as i64,
            genre.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series genre: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_origin_countries WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series origin countries: {}", e)))?;

    for country in &details.origin_countries {
        sqlx::query!(
            "INSERT INTO series_origin_countries (series_id, iso_3166_1) VALUES ($1, $2)",
            series_id,
            country
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to insert series origin country: {}", e))
        })?;
    }

    sqlx::query!(
        "DELETE FROM series_spoken_languages WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series languages: {}", e)))?;

    for language in &details.spoken_languages {
        sqlx::query!(
            "INSERT INTO series_spoken_languages (series_id, iso_639_1, name) VALUES ($1, $2, $3)",
            series_id,
            language.iso_639_1.clone(),
            language.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series language: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_production_companies WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series companies: {}", e)))?;

    for company in &details.production_companies {
        sqlx::query!(
            "INSERT INTO series_production_companies (series_id, company_id, name, origin_country) VALUES ($1, $2, $3, $4)",
            series_id,
            company.id as i64,
            company.name,
            company.origin_country
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series company: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_production_countries WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        MediaError::Internal(format!(
            "Failed to clear series production countries: {}",
            e
        ))
    })?;

    for country in &details.production_countries {
        sqlx::query!(
            "INSERT INTO series_production_countries (series_id, iso_3166_1, name) VALUES ($1, $2, $3)",
            series_id,
            country.iso_3166_1,
            country.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series production country: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_networks WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series networks: {}", e)))?;

    for network in &details.networks {
        sqlx::query!(
            "INSERT INTO series_networks (series_id, network_id, name, origin_country) VALUES ($1, $2, $3, $4)",
            series_id,
            network.id as i64,
            network.name,
            network.origin_country
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series network: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_content_ratings WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series content ratings: {}", e)))?;

    for rating in &details.content_ratings {
        sqlx::query!(
            "INSERT INTO series_content_ratings (series_id, iso_3166_1, rating, rating_system, descriptors) VALUES ($1, $2, $3, $4, $5)",
            series_id,
            rating.iso_3166_1,
            rating.rating,
            rating.rating_system,
            &rating.descriptors
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series content rating: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_keywords WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series keywords: {}", e)))?;

    for keyword in &details.keywords {
        sqlx::query!(
            "INSERT INTO series_keywords (series_id, keyword_id, name) VALUES ($1, $2, $3)",
            series_id,
            keyword.id as i64,
            keyword.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series keyword: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_episode_groups WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series episode groups: {}", e)))?;

    for group in &details.episode_groups {
        sqlx::query!(
            "INSERT INTO series_episode_groups (series_id, group_id, name, description, group_type) VALUES ($1, $2, $3, $4, $5)",
            series_id,
            group.id,
            group.name,
            group.description,
            group.group_type
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series episode group: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_translations WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series translations: {}", e)))?;

    for translation in &details.translations {
        sqlx::query!(
            r#"INSERT INTO series_translations (
                series_id, iso_3166_1, iso_639_1, name, english_name,
                title, overview, homepage, tagline
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
            series_id,
            translation.iso_3166_1,
            translation.iso_639_1,
            translation.name,
            translation.english_name,
            translation.title,
            translation.overview,
            translation.homepage,
            translation.tagline
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series translation: {}", e)))?;
    }

    sqlx::query!("DELETE FROM series_videos WHERE series_id = $1", series_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear series videos: {}", e)))?;

    for video in &details.videos {
        sqlx::query!(
            r#"INSERT INTO series_videos (
                series_id, video_key, site, name, video_type, official,
                iso_639_1, iso_3166_1, published_at, size
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
            series_id,
            video.key,
            video.site,
            video.name,
            video.video_type,
            video.official,
            video.iso_639_1,
            video.iso_3166_1,
            video
                .published_at
                .as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            video.size.map(|s| s as i32)
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series video: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM series_recommendations WHERE series_id = $1",
        series_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear series recommendations: {}", e)))?;

    for recommendation in &details.recommendations {
        sqlx::query!(
            "INSERT INTO series_recommendations (series_id, recommended_tmdb_id, title) VALUES ($1, $2, $3)",
            series_id,
            recommendation.tmdb_id as i64,
            recommendation.title
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series recommendation: {}", e)))?;
    }

    sqlx::query!("DELETE FROM series_similar WHERE series_id = $1", series_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear similar series: {}", e)))?;

    for similar in &details.similar {
        sqlx::query!(
            "INSERT INTO series_similar (series_id, similar_tmdb_id, title) VALUES ($1, $2, $3)",
            series_id,
            similar.tmdb_id as i64,
            similar.title
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert similar series: {}", e)))?;
    }

    sync_series_people(tx, series_id, &details.cast, &details.crew).await
}

async fn sync_series_people(
    tx: &mut Transaction<'_, Postgres>,
    series_id: Uuid,
    cast: &[CastMember],
    crew: &[CrewMember],
) -> Result<()> {
    let mut profile_cache: HashMap<u64, Option<Uuid>> = HashMap::new();
    let mut processed_people: HashSet<u64> = HashSet::new();
    let mut seen_cast: HashSet<(u64, String)> = HashSet::new();
    let mut seen_crew: HashSet<(u64, String, String)> = HashSet::new();

    for member in cast {
        if processed_people.insert(member.id) {
            upsert_person(tx, member.id, member).await?;
        }
    }
    for member in crew {
        if processed_people.insert(member.id) {
            upsert_person(tx, member.id, member).await?;
        }
    }

    sqlx::query!("DELETE FROM series_cast WHERE series_id = $1", series_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear series cast: {}", e)))?;

    for member in cast {
        let character = member.character.trim().to_string();
        if !seen_cast.insert((member.id, character.clone())) {
            continue;
        }

        let profile_image_id = ensure_profile_image_id(tx, &mut profile_cache, member).await?;
        sqlx::query!(
            r#"INSERT INTO series_cast (
                series_id, person_tmdb_id, credit_id, character, total_episode_count, order_index, profile_image_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (series_id, person_tmdb_id, character) DO NOTHING"#,
            series_id,
            member.id as i64,
            member.credit_id.clone(),
            &character,
            member.order as i32,
            member.image_slot as i32,
            profile_image_id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series cast member: {}", e)))?;
    }

    sqlx::query!("DELETE FROM series_crew WHERE series_id = $1", series_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear series crew: {}", e)))?;

    for member in crew {
        let department = member.department.trim().to_string();
        let job = member.job.trim().to_string();
        if !seen_crew.insert((member.id, department.clone(), job.clone())) {
            continue;
        }

        sqlx::query!(
            r#"INSERT INTO series_crew (
                series_id, person_tmdb_id, credit_id, department, job
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (series_id, person_tmdb_id, department, job) DO NOTHING"#,
            series_id,
            member.id as i64,
            member.credit_id.clone(),
            &department,
            &job
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert series crew member: {}", e)))?;
    }

    Ok(())
}

async fn sync_episode_child_tables(
    tx: &mut Transaction<'_, Postgres>,
    episode_id: Uuid,
    details: &EpisodeDetails,
) -> Result<()> {
    let mut profile_cache: HashMap<u64, Option<Uuid>> = HashMap::new();

    sqlx::query!(
        "DELETE FROM episode_keywords WHERE episode_id = $1",
        episode_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear episode keywords: {}", e)))?;

    for keyword in &details.keywords {
        sqlx::query!(
            "INSERT INTO episode_keywords (episode_id, keyword_id, name) VALUES ($1, $2, $3)",
            episode_id,
            keyword.id as i64,
            keyword.name
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert episode keyword: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM episode_videos WHERE episode_id = $1",
        episode_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear episode videos: {}", e)))?;

    for video in &details.videos {
        sqlx::query!(
            r#"INSERT INTO episode_videos (
                episode_id, video_key, site, name, video_type, official,
                iso_639_1, iso_3166_1, published_at, size
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
            episode_id,
            video.key,
            video.site,
            video.name,
            video.video_type,
            video.official,
            video.iso_639_1,
            video.iso_3166_1,
            video
                .published_at
                .as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            video.size.map(|s| s as i32)
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert episode video: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM episode_translations WHERE episode_id = $1",
        episode_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear episode translations: {}", e)))?;

    for translation in &details.translations {
        sqlx::query!(
            r#"INSERT INTO episode_translations (
                episode_id, iso_3166_1, iso_639_1, name, english_name,
                title, overview, homepage, tagline
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
            episode_id,
            translation.iso_3166_1,
            translation.iso_639_1,
            translation.name,
            translation.english_name,
            translation.title,
            translation.overview,
            translation.homepage,
            translation.tagline
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to insert episode translation: {}", e))
        })?;
    }

    sqlx::query!(
        "DELETE FROM episode_content_ratings WHERE episode_id = $1",
        episode_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear episode content ratings: {}", e)))?;

    for rating in &details.content_ratings {
        sqlx::query!(
            "INSERT INTO episode_content_ratings (episode_id, iso_3166_1, rating, rating_system, descriptors) VALUES ($1, $2, $3, $4, $5)",
            episode_id,
            rating.iso_3166_1,
            rating.rating,
            rating.rating_system,
            &rating.descriptors
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert episode content rating: {}", e)))?;
    }

    let mut processed_people: HashSet<u64> = HashSet::new();

    sqlx::query!("DELETE FROM episode_cast WHERE episode_id = $1", episode_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear episode cast: {}", e)))?;

    for member in &details.guest_stars {
        if processed_people.insert(member.id) {
            upsert_person(tx, member.id, member).await?;
        }
        let profile_image_id = ensure_profile_image_id(tx, &mut profile_cache, member).await?;
        sqlx::query!(
            "INSERT INTO episode_cast (episode_id, person_tmdb_id, credit_id, character, order_index, profile_image_id) VALUES ($1, $2, $3, $4, $5, $6)",
            episode_id,
            member.id as i64,
            member.credit_id.clone(),
            member.character,
            member.image_slot as i32,
            profile_image_id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert episode cast: {}", e)))?;
    }

    sqlx::query!(
        "DELETE FROM episode_guest_stars WHERE episode_id = $1",
        episode_id
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear episode guest stars: {}", e)))?;

    for member in &details.guest_stars {
        let profile_image_id = ensure_profile_image_id(tx, &mut profile_cache, member).await?;
        sqlx::query!(
            "INSERT INTO episode_guest_stars (episode_id, person_tmdb_id, credit_id, character, order_index, profile_image_id) VALUES ($1, $2, $3, $4, $5, $6)",
            episode_id,
            member.id as i64,
            member.credit_id.clone(),
            member.character,
            member.image_slot as i32,
            profile_image_id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert episode guest star: {}", e)))?;
    }

    sqlx::query!("DELETE FROM episode_crew WHERE episode_id = $1", episode_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to clear episode crew: {}", e)))?;

    for member in &details.crew {
        if processed_people.insert(member.id) {
            upsert_person(tx, member.id, member).await?;
        }
        sqlx::query!(
            "INSERT INTO episode_crew (episode_id, person_tmdb_id, credit_id, department, job) VALUES ($1, $2, $3, $4, $5)",
            episode_id,
            member.id as i64,
            member.credit_id.clone(),
            member.department,
            member.job
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert episode crew: {}", e)))?;
    }

    Ok(())
}

async fn upsert_person(
    tx: &mut Transaction<'_, Postgres>,
    tmdb_id: u64,
    member: &impl PersonLike,
) -> Result<()> {
    let gender = member.gender().map(|g| g as i16);
    sqlx::query!(
        r#"
        INSERT INTO persons (
            tmdb_id, name, original_name, gender, known_for_department, profile_path,
            adult, popularity, biography, birthday, deathday, place_of_birth, homepage,
            imdb_id, facebook_id, instagram_id, twitter_id, wikidata_id, tiktok_id, youtube_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $6,
            $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, $17, $18, $19, $20
        )
        ON CONFLICT (tmdb_id) DO UPDATE SET
            name = EXCLUDED.name,
            original_name = EXCLUDED.original_name,
            gender = EXCLUDED.gender,
            known_for_department = EXCLUDED.known_for_department,
            profile_path = EXCLUDED.profile_path,
            adult = EXCLUDED.adult,
            popularity = EXCLUDED.popularity,
            imdb_id = EXCLUDED.imdb_id,
            facebook_id = EXCLUDED.facebook_id,
            instagram_id = EXCLUDED.instagram_id,
            twitter_id = EXCLUDED.twitter_id,
            wikidata_id = EXCLUDED.wikidata_id,
            tiktok_id = EXCLUDED.tiktok_id,
            youtube_id = EXCLUDED.youtube_id,
            updated_at = NOW()
        "#,
        tmdb_id as i64,
        member.name(),
        member.original_name(),
        gender,
        member.known_for_department(),
        member.profile_path(),
        member.adult(),
        member.popularity(),
        None::<String>,
        None::<chrono::NaiveDate>,
        None::<chrono::NaiveDate>,
        None::<String>,
        None::<String>,
        member.external_ids().imdb_id.clone(),
        member.external_ids().facebook_id.clone(),
        member.external_ids().instagram_id.clone(),
        member.external_ids().twitter_id.clone(),
        member.external_ids().wikidata_id.clone(),
        member.external_ids().tiktok_id.clone(),
        member.external_ids().youtube_id.clone()
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert person {}: {}", tmdb_id, e)))?;

    sqlx::query!(
        "DELETE FROM person_aliases WHERE tmdb_id = $1",
        tmdb_id as i64
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to clear person aliases: {}", e)))?;

    for alias in member.aliases() {
        sqlx::query!(
            "INSERT INTO person_aliases (tmdb_id, alias) VALUES ($1, $2)",
            tmdb_id as i64,
            alias
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert person alias: {}", e)))?;
    }

    Ok(())
}

/// Helper trait to provide shared accessors for cast/crew data.
trait PersonLike {
    fn id(&self) -> u64;
    fn name(&self) -> String;
    fn original_name(&self) -> Option<String>;
    fn known_for_department(&self) -> Option<String>;
    fn profile_path(&self) -> Option<String>;
    fn gender(&self) -> Option<u8>;
    fn adult(&self) -> Option<bool>;
    fn popularity(&self) -> Option<f32>;
    fn external_ids(&self) -> &PersonExternalIds;
    fn aliases(&self) -> &[String];
}

impl PersonLike for CastMember {
    fn id(&self) -> u64 {
        self.id
    }
    fn name(&self) -> String {
        self.name.clone()
    }
    fn original_name(&self) -> Option<String> {
        self.original_name.clone()
    }
    fn known_for_department(&self) -> Option<String> {
        self.known_for_department.clone()
    }
    fn profile_path(&self) -> Option<String> {
        self.profile_path.clone()
    }
    fn gender(&self) -> Option<u8> {
        self.gender
    }
    fn adult(&self) -> Option<bool> {
        self.adult
    }
    fn popularity(&self) -> Option<f32> {
        self.popularity
    }
    fn external_ids(&self) -> &PersonExternalIds {
        &self.external_ids
    }
    fn aliases(&self) -> &[String] {
        &self.also_known_as
    }
}

impl PersonLike for CrewMember {
    fn id(&self) -> u64 {
        self.id
    }
    fn name(&self) -> String {
        self.name.clone()
    }
    fn original_name(&self) -> Option<String> {
        self.original_name.clone()
    }
    fn known_for_department(&self) -> Option<String> {
        self.known_for_department.clone()
    }
    fn profile_path(&self) -> Option<String> {
        self.profile_path.clone()
    }
    fn gender(&self) -> Option<u8> {
        self.gender
    }
    fn adult(&self) -> Option<bool> {
        self.adult
    }
    fn popularity(&self) -> Option<f32> {
        self.popularity
    }
    fn external_ids(&self) -> &PersonExternalIds {
        &self.external_ids
    }
    fn aliases(&self) -> &[String] {
        &self.also_known_as
    }
}

/// Store or update media file row.
async fn store_media_file(
    tx: &mut Transaction<'_, Postgres>,
    media_file: &MediaFile,
) -> Result<Uuid> {
    let technical_metadata = media_file
        .media_file_metadata
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| MediaError::InvalidMedia(format!("Failed to serialize metadata: {}", e)))?;

    let parsed_info = technical_metadata
        .as_ref()
        .and_then(|m| m.get("parsed_info"))
        .cloned();

    let file_path_str = media_file.path.to_string_lossy().to_string();

    let actual_id = sqlx::query_scalar!(
        r#"
        INSERT INTO media_files (
            id, library_id, file_path, filename, file_size, created_at,
            technical_metadata, parsed_info
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (file_path) DO UPDATE SET
            filename = EXCLUDED.filename,
            file_size = EXCLUDED.file_size,
            technical_metadata = EXCLUDED.technical_metadata,
            parsed_info = EXCLUDED.parsed_info,
            updated_at = NOW()
        RETURNING id
        "#,
        media_file.id,
        media_file.library_id.as_uuid(),
        file_path_str,
        media_file.filename,
        media_file.size as i64,
        media_file.created_at,
        technical_metadata,
        parsed_info
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert media file: {}", e)))?;

    Ok(actual_id)
}

async fn upsert_local_movie_reference(
    tx: &mut Transaction<'_, Postgres>,
    movie: &MovieReference,
    file_id: Uuid,
) -> Result<Uuid> {
    let movie_uuid = movie.id.to_uuid();
    let existing = sqlx::query!(
        "SELECT id FROM movie_references WHERE file_id = $1",
        file_id
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to check existing movie reference: {}", e))
    })?;

    if let Some(row) = existing {
        sqlx::query!(
            r#"
            UPDATE movie_references
            SET title = $1, theme_color = $2, updated_at = NOW()
            WHERE id = $3
            "#,
            movie.title.as_str(),
            movie.theme_color.as_deref(),
            row.id
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update movie reference: {}", e)))?;
        Ok(row.id)
    } else {
        sqlx::query!(
            r#"
            INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, theme_color)
            VALUES ($1, $2, $3, 0, $4, $5)
            "#,
            movie_uuid,
            movie.library_id.as_uuid(),
            file_id,
            movie.title.as_str(),
            movie.theme_color.as_deref()
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to insert movie reference: {}", e)))?;
        Ok(movie_uuid)
    }
}

async fn upsert_tmdb_movie_reference(
    tx: &mut Transaction<'_, Postgres>,
    movie: &MovieReference,
    file_id: Uuid,
) -> Result<Uuid> {
    let row = sqlx::query!(
        r#"
        INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, theme_color)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (tmdb_id, library_id) DO UPDATE SET
            file_id = EXCLUDED.file_id,
            title = EXCLUDED.title,
            theme_color = EXCLUDED.theme_color,
            updated_at = NOW()
        RETURNING id
        "#,
        movie.id.to_uuid(),
        movie.library_id.as_uuid(),
        file_id,
        movie.tmdb_id as i64,
        movie.title.as_str(),
        movie.theme_color.as_deref()
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to store movie reference: {}", e)))?;

    Ok(row.id)
}

async fn upsert_series_reference(
    tx: &mut Transaction<'_, Postgres>,
    series: &SeriesReference,
) -> Result<Uuid> {
    let tmdb_id = if series.tmdb_id == 0 {
        None
    } else {
        Some(series.tmdb_id as i64)
    };

    let result_id = if let Some(tmdb_value) = tmdb_id {
        let existing = sqlx::query!(
            r#"
            SELECT id
            FROM series_references
            WHERE library_id = $1 AND tmdb_id = $2
            "#,
            series.library_id.as_uuid(),
            tmdb_value
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to lookup series reference: {}", e)))?;

        if let Some(row) = existing {
            sqlx::query!(
                r#"
                UPDATE series_references
                SET title = $2, theme_color = $3, updated_at = NOW()
                WHERE id = $1
                "#,
                row.id,
                series.title.as_str(),
                series.theme_color.as_deref()
            )
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to update series reference: {}", e))
            })?;

            row.id
        } else {
            let inserted = sqlx::query!(
                r#"
                INSERT INTO series_references (id, library_id, tmdb_id, title, theme_color, created_at)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (id) DO UPDATE SET
                    title = EXCLUDED.title,
                    theme_color = EXCLUDED.theme_color,
                    updated_at = NOW()
                RETURNING id
                "#,
                series.id.to_uuid(),
                series.library_id.as_uuid(),
                tmdb_value,
                series.title.as_str(),
                series.theme_color.as_deref(),
                series.created_at
            )
            .fetch_one(&mut **tx)
            .await?;

            inserted.id
        }
    } else {
        let row = sqlx::query!(
            r#"
            INSERT INTO series_references (id, library_id, tmdb_id, title, theme_color, created_at)
            VALUES ($1, $2, NULL, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                title = EXCLUDED.title,
                theme_color = EXCLUDED.theme_color,
                updated_at = NOW()
            RETURNING id
            "#,
            series.id.to_uuid(),
            series.library_id.as_uuid(),
            series.title.as_str(),
            series.theme_color.as_deref(),
            series.created_at
        )
        .fetch_one(&mut **tx)
        .await?;

        row.id
    };

    Ok(result_id)
}

async fn upsert_season_reference(
    tx: &mut Transaction<'_, Postgres>,
    season: &SeasonReference,
) -> Result<Uuid> {
    let row = sqlx::query!(
        r#"
        INSERT INTO season_references (
            id,
            season_number,
            series_id,
            library_id,
            tmdb_series_id,
            created_at,
            theme_color
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (series_id, season_number) DO UPDATE SET
            tmdb_series_id = EXCLUDED.tmdb_series_id,
            theme_color = EXCLUDED.theme_color,
            updated_at = NOW()
        RETURNING id
        "#,
        season.id.to_uuid(),
        season.season_number.value() as i32,
        season.series_id.to_uuid(),
        season.library_id.as_uuid(),
        season.tmdb_series_id as i64,
        season.created_at,
        season.theme_color.clone()
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to upsert season reference: {}", e)))?;

    Ok(row.id)
}

fn hydrate_media_file_row(row: &PgRow) -> Result<MediaFile> {
    let media_file_metadata: Option<serde_json::Value> = row.try_get("technical_metadata").ok();
    let parsed = media_file_metadata
        .as_ref()
        .and_then(|tm| serde_json::from_value::<crate::MediaFileMetadata>(tm.clone()).ok());

    let library_id = crate::LibraryID(row.try_get("library_id")?);

    Ok(MediaFile {
        id: row.try_get("file_id")?,
        path: std::path::PathBuf::from(row.try_get::<String, _>("file_path")?),
        filename: row.try_get("filename")?,
        size: row.try_get::<i64, _>("file_size")? as u64,
        discovered_at: row.try_get("file_discovered_at")?,
        created_at: row.try_get("file_created_at")?,
        media_file_metadata: parsed,
        library_id,
    })
}

async fn load_series_details(
    pool: &PgPool,
    series_id: Uuid,
) -> Result<Option<EnhancedSeriesDetails>> {
    let metadata = sqlx::query!(
        r#"SELECT
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
            WHERE series_id = $1"#,
        series_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series metadata: {}", e)))?;

    let Some(row) = metadata else {
        return Ok(None);
    };

    let genres = sqlx::query!(
        "SELECT genre_id, name FROM series_genres WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series genres: {}", e)))?
    .into_iter()
    .map(|record| GenreInfo {
        id: record.genre_id as u64,
        name: record.name,
    })
    .collect();

    let origin_countries = sqlx::query!(
        "SELECT iso_3166_1 FROM series_origin_countries WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series origin countries: {}", e)))?
    .into_iter()
    .map(|record| record.iso_3166_1)
    .collect();

    let spoken_languages = sqlx::query!(
        "SELECT iso_639_1, name FROM series_spoken_languages WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series spoken languages: {}", e)))?
    .into_iter()
    .map(|record| SpokenLanguage {
        iso_639_1: record.iso_639_1,
        name: record.name,
    })
    .collect();

    let production_companies = sqlx::query!(
        "SELECT company_id, name, origin_country FROM series_production_companies WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series production companies: {}", e)))?
    .into_iter()
    .map(|record| ProductionCompany {
        id: record.company_id.unwrap_or_default() as u64,
        name: record.name,
        origin_country: record.origin_country,
    })
    .collect();

    let production_countries = sqlx::query!(
        "SELECT iso_3166_1, name FROM series_production_countries WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        MediaError::Internal(format!("Failed to load series production countries: {}", e))
    })?
    .into_iter()
    .map(|record| ProductionCountry {
        iso_3166_1: record.iso_3166_1,
        name: record.name,
    })
    .collect();

    let networks = sqlx::query!(
        "SELECT network_id, name, origin_country FROM series_networks WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series networks: {}", e)))?
    .into_iter()
    .map(|record| NetworkInfo {
        id: record.network_id as u64,
        name: record.name,
        origin_country: record.origin_country,
    })
    .collect();

    let content_ratings = sqlx::query!(
        "SELECT iso_3166_1, rating, rating_system, descriptors FROM series_content_ratings WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series content ratings: {}", e)))?
    .into_iter()
    .map(|record| ContentRating {
        iso_3166_1: record.iso_3166_1,
        rating: record.rating,
        rating_system: record.rating_system,
        descriptors: record.descriptors.unwrap_or_default(),
    })
    .collect();

    let keywords = sqlx::query!(
        "SELECT keyword_id, name FROM series_keywords WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series keywords: {}", e)))?
    .into_iter()
    .map(|record| Keyword {
        id: record.keyword_id as u64,
        name: record.name,
    })
    .collect();

    let videos = sqlx::query!(
        r#"SELECT video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM series_videos WHERE series_id = $1"#,
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series videos: {}", e)))?
    .into_iter()
    .map(|record| Video {
        key: record.video_key,
        name: record.name,
        site: record.site,
        video_type: record.video_type,
        official: record.official,
        iso_639_1: record.iso_639_1,
        iso_3166_1: record.iso_3166_1,
        published_at: record
            .published_at
            .map(|dt| dt.with_timezone(&chrono::Utc).to_rfc3339()),
        size: record.size.map(|s| s as u32),
    })
    .collect();

    let translations = sqlx::query!(
        r#"SELECT iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM series_translations WHERE series_id = $1"#,
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series translations: {}", e)))?
    .into_iter()
    .map(|record| Translation {
        iso_3166_1: record.iso_3166_1,
        iso_639_1: record.iso_639_1,
        name: record.name,
        english_name: record.english_name,
        title: record.title,
        overview: record.overview,
        homepage: record.homepage,
        tagline: record.tagline,
    })
    .collect();

    let episode_groups = sqlx::query!(
        "SELECT group_id, name, description, group_type FROM series_episode_groups WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series episode groups: {}", e)))?
    .into_iter()
    .map(|record| EpisodeGroupMembership {
        id: record.group_id,
        name: record.name,
        description: record.description,
        group_type: record.group_type,
    })
    .collect();

    let recommendations = sqlx::query!(
        "SELECT recommended_tmdb_id, title FROM series_recommendations WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series recommendations: {}", e)))?
    .into_iter()
    .map(|record| RelatedMediaRef {
        tmdb_id: record.recommended_tmdb_id as u64,
        title: record.title,
    })
    .collect();

    let similar = sqlx::query!(
        "SELECT similar_tmdb_id, title FROM series_similar WHERE series_id = $1",
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load similar series: {}", e)))?
    .into_iter()
    .map(|record| RelatedMediaRef {
        tmdb_id: record.similar_tmdb_id as u64,
        title: record.title,
    })
    .collect();

    let cast = load_series_cast(pool, series_id).await?;
    let crew = load_series_crew(pool, series_id).await?;

    let details = EnhancedSeriesDetails {
        id: row.tmdb_id as u64,
        name: row.name.clone(),
        original_name: row.original_name.clone(),
        overview: row.overview.clone(),
        first_air_date: row.first_air_date.map(|d| d.to_string()),
        last_air_date: row.last_air_date.map(|d| d.to_string()),
        number_of_seasons: row.number_of_seasons.map(|n| n as u32),
        number_of_episodes: row.number_of_episodes.map(|n| n as u32),
        vote_average: row.vote_average,
        vote_count: row.vote_count.map(|v| v as u32),
        popularity: row.popularity,
        content_rating: row.primary_content_rating.clone(),
        content_ratings,
        release_dates: Vec::new(),
        genres,
        networks,
        origin_countries,
        spoken_languages,
        production_companies,
        production_countries,
        homepage: row.homepage.clone(),
        status: row.status.clone(),
        tagline: row.tagline.clone(),
        in_production: row.in_production,
        poster_path: row.poster_path.clone(),
        backdrop_path: row.backdrop_path.clone(),
        logo_path: row.logo_path.clone(),
        images: crate::MediaImages::default(),
        cast,
        crew,
        videos,
        keywords,
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
        translations,
        episode_groups,
        recommendations,
        similar,
    };

    Ok(Some(details))
}

async fn load_movie_details(pool: &PgPool, movie_id: Uuid) -> Result<Option<EnhancedMovieDetails>> {
    let metadata = sqlx::query!(
        r#"
        SELECT * FROM movie_metadata WHERE movie_id = $1
        "#,
        movie_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie metadata: {}", e)))?;

    let Some(row) = metadata else {
        return Ok(None);
    };

    let genres = sqlx::query!(
        "SELECT genre_id, name FROM movie_genres WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie genres: {}", e)))?
    .into_iter()
    .map(|record| GenreInfo {
        id: record.genre_id as u64,
        name: record.name,
    })
    .collect();

    let spoken_languages = sqlx::query!(
        "SELECT iso_639_1, name FROM movie_spoken_languages WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie languages: {}", e)))?
    .into_iter()
    .map(|record| SpokenLanguage {
        iso_639_1: record.iso_639_1,
        name: record.name,
    })
    .collect();

    let production_companies = sqlx::query!(
        "SELECT company_id, name, origin_country FROM movie_production_companies WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie companies: {}", e)))?
    .into_iter()
    .map(|record| ProductionCompany {
        id: record.company_id.unwrap_or_default() as u64,
        name: record.name,
        origin_country: record.origin_country,
    })
    .collect();

    let production_countries = sqlx::query!(
        "SELECT iso_3166_1, name FROM movie_production_countries WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie countries: {}", e)))?
    .into_iter()
    .map(|record| ProductionCountry {
        iso_3166_1: record.iso_3166_1,
        name: record.name,
    })
    .collect();

    let release_dates_rows = sqlx::query!(
        r#"SELECT iso_3166_1, iso_639_1, certification, release_date, release_type, note, descriptors
             FROM movie_release_dates WHERE movie_id = $1"#,
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie release dates: {}", e)))?;

    let mut release_map: HashMap<String, Vec<ReleaseDateEntry>> = HashMap::new();
    for record in release_dates_rows {
        release_map
            .entry(record.iso_3166_1)
            .or_default()
            .push(ReleaseDateEntry {
                certification: record.certification,
                release_date: Some(record.release_date.with_timezone(&chrono::Utc).to_rfc3339()),
                release_type: Some(i32::from(record.release_type)),
                note: record.note,
                iso_639_1: record.iso_639_1,
                descriptors: record.descriptors.unwrap_or_default(),
            });
    }

    let release_dates: Vec<ReleaseDatesByCountry> = release_map
        .into_iter()
        .map(|(iso, entries)| ReleaseDatesByCountry {
            iso_3166_1: iso,
            release_dates: entries,
        })
        .collect();

    let content_ratings =
        build_movie_content_ratings(&release_dates, row.primary_certification.clone());

    let alternative_titles = sqlx::query!(
        "SELECT iso_3166_1, title, title_type FROM movie_alternative_titles WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| AlternativeTitle {
        title: record.title,
        iso_3166_1: record.iso_3166_1,
        title_type: record.title_type,
    })
    .collect();

    let translations = sqlx::query!(
        r#"SELECT iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM movie_translations WHERE movie_id = $1"#,
        movie_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| Translation {
        iso_3166_1: record.iso_3166_1,
        iso_639_1: record.iso_639_1,
        name: record.name,
        english_name: record.english_name,
        title: record.title,
        overview: record.overview,
        homepage: record.homepage,
        tagline: record.tagline,
    })
    .collect();

    let videos = sqlx::query!(
        r#"SELECT video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
             FROM movie_videos WHERE movie_id = $1"#,
        movie_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| Video {
        key: record.video_key,
        name: record.name,
        site: record.site,
        video_type: record.video_type,
        official: record.official,
        iso_639_1: record.iso_639_1,
        iso_3166_1: record.iso_3166_1,
        published_at: record
            .published_at
            .map(|dt| dt.with_timezone(&chrono::Utc).to_rfc3339()),
        size: record.size.map(|s| s as u32),
    })
    .collect();

    let keywords = sqlx::query!(
        "SELECT keyword_id, name FROM movie_keywords WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| Keyword {
        id: record.keyword_id as u64,
        name: record.name,
    })
    .collect();

    let recommendations = sqlx::query!(
        "SELECT recommended_tmdb_id, title FROM movie_recommendations WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| RelatedMediaRef {
        tmdb_id: record.recommended_tmdb_id as u64,
        title: record.title,
    })
    .collect();

    let similar = sqlx::query!(
        "SELECT similar_tmdb_id, title FROM movie_similar WHERE movie_id = $1",
        movie_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| RelatedMediaRef {
        tmdb_id: record.similar_tmdb_id as u64,
        title: record.title,
    })
    .collect();

    let collection = sqlx::query!(
        "SELECT collection_id, name, poster_path, backdrop_path FROM movie_collection_membership WHERE movie_id = $1",
        movie_id
    )
    .fetch_optional(pool)
    .await?
    .map(|record| CollectionInfo {
        id: record.collection_id as u64,
        name: record.name,
        poster_path: record.poster_path,
        backdrop_path: record.backdrop_path,
    });

    let cast = load_cast(pool, movie_id).await?;
    let crew = load_crew(pool, movie_id).await?;

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
        genres,
        spoken_languages,
        production_companies,
        production_countries,
        homepage: row.homepage.clone(),
        status: row.status.clone(),
        tagline: row.tagline.clone(),
        budget: row.budget.map(|b| b as u64),
        revenue: row.revenue.map(|r| r as u64),
        poster_path: row.poster_path.clone(),
        backdrop_path: row.backdrop_path.clone(),
        logo_path: row.logo_path.clone(),
        images: crate::MediaImages::default(),
        cast,
        crew,
        videos,
        keywords,
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
        alternative_titles,
        translations,
        collection,
        recommendations,
        similar,
    };

    Ok(Some(details))
}

async fn load_season_details(pool: &PgPool, season_id: Uuid) -> Result<Option<SeasonDetails>> {
    let row = sqlx::query!(
        r#"
        SELECT
            sm.tmdb_id,
            sm.name,
            sm.overview,
            sm.air_date,
            sm.episode_count,
            sm.poster_path,
            sm.runtime,
            sm.imdb_id,
            sm.facebook_id,
            sm.instagram_id,
            sm.twitter_id,
            sm.wikidata_id,
            sr.season_number
        FROM season_metadata sm
        JOIN season_references sr ON sr.id = sm.season_id
        WHERE sm.season_id = $1
        "#,
        season_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load season metadata: {}", e)))?;

    let Some(row) = row else {
        return Ok(None);
    };

    let videos = sqlx::query!(
        r#"SELECT video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM season_videos WHERE season_id = $1"#,
        season_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load season videos: {}", e)))?
    .into_iter()
    .map(|record| Video {
        key: record.video_key,
        name: record.name,
        site: record.site,
        video_type: record.video_type,
        official: record.official,
        iso_639_1: record.iso_639_1,
        iso_3166_1: record.iso_3166_1,
        published_at: record
            .published_at
            .map(|dt| dt.with_timezone(&chrono::Utc).to_rfc3339()),
        size: record.size.map(|s| s as u32),
    })
    .collect();

    let keywords = sqlx::query!(
        "SELECT keyword_id, name FROM season_keywords WHERE season_id = $1",
        season_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load season keywords: {}", e)))?
    .into_iter()
    .map(|record| Keyword {
        id: record.keyword_id as u64,
        name: record.name,
    })
    .collect();

    let translations = sqlx::query!(
        r#"SELECT iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM season_translations WHERE season_id = $1"#,
        season_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load season translations: {}", e)))?
    .into_iter()
    .map(|record| Translation {
        iso_3166_1: record.iso_3166_1,
        iso_639_1: record.iso_639_1,
        name: record.name,
        english_name: record.english_name,
        title: record.title,
        overview: record.overview,
        homepage: record.homepage,
        tagline: record.tagline,
    })
    .collect();

    let raw_season_number = row.season_number;
    let season_number = match u8::try_from(raw_season_number) {
        Ok(value) => value,
        Err(_) => 0,
    };
    let details = SeasonDetails {
        id: row.tmdb_id as u64,
        season_number,
        name: row
            .name
            .clone()
            .unwrap_or_else(|| format!("Season {}", season_number)),
        overview: row.overview.clone(),
        air_date: row.air_date.map(|d| d.to_string()),
        episode_count: row.episode_count.unwrap_or_default() as u32,
        poster_path: row.poster_path.clone(),
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
        images: crate::MediaImages::default(),
        videos,
        keywords,
        translations,
    };

    Ok(Some(details))
}

async fn load_episode_details(pool: &PgPool, episode_id: Uuid) -> Result<Option<EpisodeDetails>> {
    let row = sqlx::query!(
        r#"
        SELECT
            tmdb_id,
            season_number,
            episode_number,
            name,
            overview,
            air_date,
            runtime,
            still_path,
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
        WHERE episode_id = $1
        "#,
        episode_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode metadata: {}", e)))?;

    let Some(row) = row else {
        return Ok(None);
    };

    let videos = sqlx::query!(
        r#"SELECT video_key, site, name, video_type, official, iso_639_1, iso_3166_1, published_at, size
            FROM episode_videos WHERE episode_id = $1"#,
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode videos: {}", e)))?
    .into_iter()
    .map(|record| Video {
        key: record.video_key,
        name: record.name,
        site: record.site,
        video_type: record.video_type,
        official: record.official,
        iso_639_1: record.iso_639_1,
        iso_3166_1: record.iso_3166_1,
        published_at: record
            .published_at
            .map(|dt| dt.with_timezone(&chrono::Utc).to_rfc3339()),
        size: record.size.map(|s| s as u32),
    })
    .collect();

    let keywords = sqlx::query!(
        "SELECT keyword_id, name FROM episode_keywords WHERE episode_id = $1",
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode keywords: {}", e)))?
    .into_iter()
    .map(|record| Keyword {
        id: record.keyword_id as u64,
        name: record.name,
    })
    .collect();

    let translations = sqlx::query!(
        r#"SELECT iso_3166_1, iso_639_1, name, english_name, title, overview, homepage, tagline
            FROM episode_translations WHERE episode_id = $1"#,
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode translations: {}", e)))?
    .into_iter()
    .map(|record| Translation {
        iso_3166_1: record.iso_3166_1,
        iso_639_1: record.iso_639_1,
        name: record.name,
        english_name: record.english_name,
        title: record.title,
        overview: record.overview,
        homepage: record.homepage,
        tagline: record.tagline,
    })
    .collect();

    let content_ratings = sqlx::query!(
        "SELECT iso_3166_1, rating, rating_system, descriptors FROM episode_content_ratings WHERE episode_id = $1",
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode content ratings: {}", e)))?
    .into_iter()
    .map(|record| ContentRating {
        iso_3166_1: record.iso_3166_1,
        rating: record.rating,
        rating_system: record.rating_system,
        descriptors: record.descriptors.unwrap_or_default(),
    })
    .collect();

    let mut guest_map: HashMap<u64, CastMember> = HashMap::new();

    let primary_cast = sqlx::query!(
        r#"SELECT
                ec.person_tmdb_id,
                ec.credit_id,
                COALESCE(ec.character, '') AS "character!",
                ec.order_index,
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
            JOIN persons p ON p.tmdb_id = ec.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = ec.person_tmdb_id
            WHERE ec.episode_id = $1
            ORDER BY ec.order_index"#,
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode cast: {}", e)))?;

    for record in primary_cast {
        let image_slot = record.order_index.unwrap_or_default() as u32;
        let profile_available = record.profile_path.as_ref().is_some();
        let profile_media_id = profile_available.then(|| {
            Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!("person-{}", record.person_tmdb_id).as_bytes(),
            )
        });

        let member = CastMember {
            id: record.person_tmdb_id as u64,
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
            profile_media_id,
            profile_image_index: profile_available.then_some(image_slot),
        };
        guest_map.insert(member.id, member);
    }

    let guest_rows = sqlx::query!(
        r#"SELECT
                eg.person_tmdb_id,
                eg.credit_id,
                COALESCE(eg.character, '') AS "character!",
                eg.order_index,
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
            FROM episode_guest_stars eg
            JOIN persons p ON p.tmdb_id = eg.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = eg.person_tmdb_id
            WHERE eg.episode_id = $1
            ORDER BY eg.order_index"#,
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode guest stars: {}", e)))?;

    for record in guest_rows {
        let image_slot = record.order_index.unwrap_or_default() as u32;
        let profile_available = record.profile_path.as_ref().is_some();
        let profile_media_id = profile_available.then(|| {
            Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!("person-{}", record.person_tmdb_id).as_bytes(),
            )
        });

        let member = CastMember {
            id: record.person_tmdb_id as u64,
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
            profile_media_id,
            profile_image_index: profile_available.then_some(image_slot),
        };
        guest_map.entry(member.id).or_insert(member);
    }

    let mut guest_stars: Vec<CastMember> = guest_map.into_values().collect();
    guest_stars.sort_by_key(|member| member.image_slot);

    let crew_rows = sqlx::query!(
        r#"SELECT
                ec.person_tmdb_id,
                ec.credit_id,
                ec.department,
                ec.job,
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
            JOIN persons p ON p.tmdb_id = ec.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = ec.person_tmdb_id
            WHERE ec.episode_id = $1
        "#,
        episode_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load episode crew: {}", e)))?;

    let crew = crew_rows
        .into_iter()
        .map(|record| CrewMember {
            id: record.person_tmdb_id as u64,
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
        })
        .collect();

    let details = EpisodeDetails {
        id: row.tmdb_id as u64,
        episode_number: row.episode_number.unwrap_or_default() as u8,
        season_number: row.season_number.unwrap_or_default() as u8,
        name: row
            .name
            .clone()
            .unwrap_or_else(|| "Untitled Episode".to_string()),
        overview: row.overview.clone(),
        air_date: row.air_date.map(|d| d.to_string()),
        runtime: row.runtime.map(|r| r as u32),
        still_path: row.still_path.clone(),
        vote_average: row.vote_average,
        vote_count: row.vote_count.map(|v| v as u32),
        production_code: row.production_code.clone(),
        external_ids: ExternalIds {
            imdb_id: row.imdb_id.clone(),
            tvdb_id: row.tvdb_id.map(|id| id as u32),
            facebook_id: row.facebook_id.clone(),
            instagram_id: row.instagram_id.clone(),
            twitter_id: row.twitter_id.clone(),
            wikidata_id: row.wikidata_id.clone(),
            tiktok_id: None,
            youtube_id: None,
            freebase_id: None,
            freebase_mid: None,
        },
        images: crate::MediaImages::default(),
        videos,
        keywords,
        translations,
        guest_stars,
        crew,
        content_ratings,
    };

    Ok(Some(details))
}

fn build_movie_content_ratings(
    release_dates: &[ReleaseDatesByCountry],
    primary_certification: Option<String>,
) -> Vec<ContentRating> {
    fn release_type_priority(release_type: Option<i32>) -> i32 {
        match release_type {
            Some(3) => 0, // Theatrical (primary)
            Some(4) => 1, // Digital
            Some(2) => 2, // Limited
            Some(1) => 3, // Premiere
            Some(5) => 4, // Physical
            Some(6) => 5, // TV
            _ => 6,
        }
    }

    let mut ratings = Vec::new();
    let mut seen_countries: HashSet<&str> = HashSet::new();

    for country in release_dates {
        let best_entry = country
            .release_dates
            .iter()
            .filter(|entry| {
                entry
                    .certification
                    .as_ref()
                    .map(|c| !c.trim().is_empty())
                    .unwrap_or(false)
            })
            .min_by(|a, b| {
                release_type_priority(a.release_type)
                    .cmp(&release_type_priority(b.release_type))
                    .then_with(|| a.release_date.cmp(&b.release_date))
            });

        if let Some(entry) = best_entry {
            ratings.push(ContentRating {
                iso_3166_1: country.iso_3166_1.clone(),
                rating: entry.certification.clone(),
                rating_system: None,
                descriptors: entry.descriptors.clone(),
            });
            seen_countries.insert(country.iso_3166_1.as_str());
        }
    }

    if let Some(cert) = primary_certification
        .as_ref()
        .filter(|c| !c.trim().is_empty())
        && !seen_countries.contains("US")
    {
        ratings.push(ContentRating {
            iso_3166_1: "US".to_string(),
            rating: Some(cert.clone()),
            rating_system: None,
            descriptors: Vec::new(),
        });
    }

    ratings
}

fn build_person_external_ids(
    imdb_id: Option<String>,
    facebook_id: Option<String>,
    instagram_id: Option<String>,
    twitter_id: Option<String>,
    wikidata_id: Option<String>,
    tiktok_id: Option<String>,
    youtube_id: Option<String>,
) -> PersonExternalIds {
    PersonExternalIds {
        imdb_id,
        facebook_id,
        instagram_id,
        twitter_id,
        wikidata_id,
        tiktok_id,
        youtube_id,
    }
}

async fn load_cast(pool: &PgPool, movie_id: Uuid) -> Result<Vec<CastMember>> {
    let cast_rows = sqlx::query!(
        r#"SELECT
                mc.person_tmdb_id,
                mc.credit_id,
                mc.cast_id,
                COALESCE(mc.character, '') AS "character!",
                mc.order_index,
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
            JOIN persons p ON p.tmdb_id = mc.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = mc.person_tmdb_id
            WHERE mc.movie_id = $1
            ORDER BY mc.order_index"#,
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie cast: {}", e)))?;

    Ok(cast_rows
        .into_iter()
        .map(|record| {
            let image_slot = record.order_index.unwrap_or_default() as u32;
            let profile_available = record.profile_path.as_ref().is_some();
            let profile_media_id = profile_available.then(|| {
                Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!("person-{}", record.person_tmdb_id).as_bytes(),
                )
            });

            CastMember {
                id: record.person_tmdb_id as u64,
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
                profile_media_id,
                profile_image_index: profile_available.then_some(image_slot),
            }
        })
        .collect())
}

async fn load_crew(pool: &PgPool, movie_id: Uuid) -> Result<Vec<CrewMember>> {
    let crew_rows = sqlx::query!(
        r#"SELECT
                mc.person_tmdb_id,
                mc.credit_id,
                mc.department,
                mc.job,
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
            JOIN persons p ON p.tmdb_id = mc.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = mc.person_tmdb_id
            WHERE mc.movie_id = $1"#,
        movie_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load movie crew: {}", e)))?;

    Ok(crew_rows
        .into_iter()
        .map(|record| CrewMember {
            id: record.person_tmdb_id as u64,
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
        })
        .collect())
}

async fn load_series_cast(pool: &PgPool, series_id: Uuid) -> Result<Vec<CastMember>> {
    let cast_rows = sqlx::query!(
        r#"SELECT
                sc.person_tmdb_id,
                sc.credit_id,
                COALESCE(sc.character, '') AS "character!",
                sc.total_episode_count,
                sc.order_index,
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
            FROM series_cast sc
            JOIN persons p ON p.tmdb_id = sc.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = sc.person_tmdb_id
            WHERE sc.series_id = $1
            ORDER BY sc.order_index"#,
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series cast: {}", e)))?;

    Ok(cast_rows
        .into_iter()
        .map(|record| {
            let image_slot = record.order_index.unwrap_or_default() as u32;
            let profile_available = record.profile_path.as_ref().is_some();
            let profile_media_id = profile_available.then(|| {
                Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!("person-{}", record.person_tmdb_id).as_bytes(),
                )
            });

            CastMember {
                id: record.person_tmdb_id as u64,
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
                profile_media_id,
                profile_image_index: profile_available.then_some(image_slot),
            }
        })
        .collect())
}

async fn load_series_crew(pool: &PgPool, series_id: Uuid) -> Result<Vec<CrewMember>> {
    let crew_rows = sqlx::query!(
        r#"SELECT
                sc.person_tmdb_id,
                sc.credit_id,
                sc.department,
                sc.job,
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
            JOIN persons p ON p.tmdb_id = sc.person_tmdb_id
            LEFT JOIN (
                SELECT tmdb_id, ARRAY_AGG(alias ORDER BY alias) AS aliases
                FROM person_aliases
                GROUP BY tmdb_id
            ) alias_data ON alias_data.tmdb_id = sc.person_tmdb_id
            WHERE sc.series_id = $1"#,
        series_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to load series crew: {}", e)))?;

    Ok(crew_rows
        .into_iter()
        .map(|record| CrewMember {
            id: record.person_tmdb_id as u64,
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
        })
        .collect())
}

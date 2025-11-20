use super::traits::*;
use crate::{MediaError, MediaFile, MediaFileMetadata, Result, Library};
use crate::media::{
    MovieReference, SeriesReference, SeasonReference, EpisodeReference,
    LibraryReference, MovieID, SeriesID, SeasonID, EpisodeID,
    MovieTitle, SeriesTitle, SeasonNumber, EpisodeNumber, 
    MediaDetailsOption, MovieURL, SeriesURL, SeasonURL, EpisodeURL,
};
use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
}

impl PostgresDatabase {
    pub async fn new(connection_string: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(connection_string)
            .await
            .map_err(|e| MediaError::Internal(format!("Database connection failed: {}", e)))?;
        
        Ok(PostgresDatabase { pool })
    }
}

#[async_trait]
impl MediaDatabaseTrait for PostgresDatabase {
    async fn initialize_schema(&self) -> Result<()> {
        // TODO: Run migrations for new reference schema
        Ok(())
    }

    async fn store_media(&self, media_file: MediaFile) -> Result<String> {
        // TODO: Implement storing media files with new schema
        Ok(media_file.id.to_string())
    }

    async fn store_media_batch(&self, media_files: Vec<MediaFile>) -> Result<Vec<String>> {
        // TODO: Implement batch storing
        Ok(media_files.into_iter().map(|m| m.id.to_string()).collect())
    }

    async fn get_media(&self, id: &str) -> Result<Option<MediaFile>> {
        // TODO: Implement fetching media files
        Ok(None)
    }

    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>> {
        // TODO: Implement fetching by path
        Ok(None)
    }

    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>> {
        // TODO: Implement media listing
        Ok(vec![])
    }

    async fn get_stats(&self) -> Result<MediaStats> {
        // TODO: Implement stats
        Ok(MediaStats {
            total_files: 0,
            total_size: 0,
            by_type: HashMap::new(),
        })
    }

    async fn file_exists(&self, path: &str) -> Result<bool> {
        // TODO: Check file existence
        Ok(false)
    }

    async fn delete_media(&self, id: &str) -> Result<()> {
        // TODO: Implement deletion
        Ok(())
    }

    async fn get_all_media(&self) -> Result<Vec<MediaFile>> {
        // TODO: Implement get all
        Ok(vec![])
    }

    async fn store_external_metadata(&self, media_id: &str, metadata: &MediaFileMetadata) -> Result<()> {
        // TODO: Implement metadata storage
        Ok(())
    }

    async fn store_tv_show(&self, show_info: &TvShowInfo) -> Result<String> {
        // TODO: Implement TV show storage
        Ok(Uuid::new_v4().to_string())
    }

    async fn get_tv_show(&self, tmdb_id: &str) -> Result<Option<TvShowInfo>> {
        // TODO: Implement TV show fetching
        Ok(None)
    }

    async fn link_episode_to_file(
        &self,
        media_file_id: &str,
        show_tmdb_id: &str,
        season: i32,
        episode: i32,
    ) -> Result<()> {
        // TODO: Implement episode linking
        Ok(())
    }

    async fn create_library(&self, library: Library) -> Result<String> {
        // TODO: Implement library creation
        Ok(library.id.to_string())
    }

    async fn get_library(&self, id: &str) -> Result<Option<Library>> {
        // TODO: Implement library fetching
        Ok(None)
    }

    async fn list_libraries(&self) -> Result<Vec<Library>> {
        // TODO: Implement library listing
        Ok(vec![])
    }

    async fn update_library(&self, id: &str, library: Library) -> Result<()> {
        // TODO: Implement library update
        Ok(())
    }

    async fn delete_library(&self, id: &str) -> Result<()> {
        // TODO: Implement library deletion
        Ok(())
    }

    async fn update_library_last_scan(&self, id: &str) -> Result<()> {
        // TODO: Implement last scan update
        Ok(())
    }
    
    async fn store_movie_reference(&self, movie: &MovieReference) -> Result<()> {
        let movie_uuid = Uuid::parse_str(movie.id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid movie ID: {}", e)))?;
        let library_id = movie.file.library_id
            .ok_or_else(|| MediaError::InvalidMedia("Movie must have library ID".to_string()))?;
            
        sqlx::query!(
            r#"
            INSERT INTO movie_references (id, tmdb_id, title, file_id, library_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                tmdb_id = EXCLUDED.tmdb_id,
                title = EXCLUDED.title,
                file_id = EXCLUDED.file_id,
                updated_at = CURRENT_TIMESTAMP
            "#,
            movie_uuid,
            movie.tmdb_id as i64,
            movie.title.as_str(),
            movie.file.id,
            library_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database insert failed: {}", e)))?;
        
        Ok(())
    }

    async fn store_series_reference(&self, series: &SeriesReference) -> Result<()> {
        let series_uuid = Uuid::parse_str(series.id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid series ID: {}", e)))?;
            
        // For now, we'll need to determine library_id from context or add it to SeriesReference
        // This is a limitation that would need to be addressed in a real implementation
        sqlx::query!(
            r#"
            INSERT INTO series_references (id, tmdb_id, title, library_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET
                tmdb_id = EXCLUDED.tmdb_id,
                title = EXCLUDED.title,
                updated_at = CURRENT_TIMESTAMP
            "#,
            series_uuid,
            series.tmdb_id as i64,
            series.title.as_str(),
            Uuid::new_v4() // Placeholder - needs proper library_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database insert failed: {}", e)))?;
        
        Ok(())
    }

    async fn store_season_reference(&self, season: &SeasonReference) -> Result<()> {
        let season_uuid = Uuid::parse_str(season.id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid season ID: {}", e)))?;
        let series_uuid = Uuid::parse_str(season.series_id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid series ID: {}", e)))?;
            
        sqlx::query!(
            r#"
            INSERT INTO season_references (id, season_number, series_id, tmdb_series_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (series_id, season_number) DO UPDATE SET
                tmdb_series_id = EXCLUDED.tmdb_series_id,
                updated_at = CURRENT_TIMESTAMP
            "#,
            season_uuid,
            season.season_number.value() as i16,
            series_uuid,
            season.tmdb_series_id as i64
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database insert failed: {}", e)))?;
        
        Ok(())
    }

    async fn store_episode_reference(&self, episode: &EpisodeReference) -> Result<()> {
        let episode_uuid = Uuid::parse_str(episode.id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid episode ID: {}", e)))?;
        let season_uuid = Uuid::parse_str(episode.season_id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid season ID: {}", e)))?;
        let series_uuid = Uuid::parse_str(episode.series_id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid series ID: {}", e)))?;
        let library_id = episode.file.library_id
            .ok_or_else(|| MediaError::InvalidMedia("Episode must have library ID".to_string()))?;
            
        sqlx::query!(
            r#"
            INSERT INTO episode_references 
                (id, episode_number, season_number, season_id, series_id, tmdb_series_id, file_id, library_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (series_id, season_number, episode_number) DO UPDATE SET
                season_id = EXCLUDED.season_id,
                tmdb_series_id = EXCLUDED.tmdb_series_id,
                file_id = EXCLUDED.file_id,
                updated_at = CURRENT_TIMESTAMP
            "#,
            episode_uuid,
            episode.episode_number.value() as i16,
            episode.season_number.value() as i16,
            season_uuid,
            series_uuid,
            episode.tmdb_series_id as i64,
            episode.file.id,
            library_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database insert failed: {}", e)))?;
        
        Ok(())
    }
    
    async fn get_all_movie_references(&self) -> Result<Vec<MovieReference>> {
        // TODO: Implement movie references fetching
        Ok(vec![])
    }

    async fn get_series_references(&self) -> Result<Vec<SeriesReference>> {
        // TODO: Implement series references fetching
        Ok(vec![])
    }

    async fn get_series_seasons(&self, series_id: &SeriesID) -> Result<Vec<SeasonReference>> {
        let series_uuid = Uuid::parse_str(series_id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid series ID: {}", e)))?;
            
        let rows = sqlx::query!(
            r#"
            SELECT sr.id, sr.season_number, sr.series_id, sr.tmdb_series_id,
                   sm.name, sm.overview, sm.air_date, sm.episode_count, sm.poster_path
            FROM season_references sr
            LEFT JOIN season_metadata sm ON sm.season_id = sr.id
            WHERE sr.series_id = $1
            ORDER BY sr.season_number
            "#,
            series_uuid
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;
        
        let mut seasons = Vec::new();
        for row in rows {
            let season_ref = SeasonReference {
                id: SeasonID::new(row.id.to_string())?,
                season_number: SeasonNumber::new(row.season_number as u8),
                series_id: series_id.clone(),
                tmdb_series_id: row.tmdb_series_id as u64,
                details: MediaDetailsOption::Endpoint(
                    format!("/api/series/{}/season/{}", row.tmdb_series_id, row.season_number)
                ),
                endpoint: SeasonURL::from_string(
                    format!("/api/season/{}", row.id)
                ),
            };
            seasons.push(season_ref);
        }
        
        Ok(seasons)
    }

    async fn get_season_episodes(&self, season_id: &SeasonID) -> Result<Vec<EpisodeReference>> {
        let season_uuid = Uuid::parse_str(season_id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid season ID: {}", e)))?;
            
        let rows = sqlx::query!(
            r#"
            SELECT er.id, er.episode_number, er.season_number, er.season_id, er.series_id,
                   er.tmdb_series_id, er.file_id,
                   mf.file_path, mf.file_name as filename, mf.file_size as size, mf.library_id, mf.created_at as file_created_at,
                   em.name, em.overview, em.air_date, em.runtime, em.still_path, em.vote_average
            FROM episode_references er
            LEFT JOIN media_files mf ON mf.id = er.file_id
            LEFT JOIN episode_metadata em ON em.episode_id = er.id
            WHERE er.season_id = $1
            ORDER BY er.episode_number
            "#,
            season_uuid
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;
        
        let mut episodes = Vec::new();
        for row in rows {
            let media_file = if let (Some(file_id), Some(file_path), Some(filename), Some(size), Some(library_id), Some(created_at)) = 
                (row.file_id, row.file_path, row.filename, row.size, row.library_id, row.file_created_at) {
                MediaFile {
                    id: file_id,
                    path: file_path.into(),
                    filename,
                    size: size as u64,
                    created_at,
                    media_file_metadata: None,
                    library_id: Some(library_id),
                }
            } else {
                MediaFile::default()
            };
            
            let episode_ref = EpisodeReference {
                id: EpisodeID::new(row.id.to_string())?,
                episode_number: EpisodeNumber::new(row.episode_number as u8),
                season_number: SeasonNumber::new(row.season_number as u8),
                season_id: SeasonID::new(row.season_id.to_string())?,
                series_id: SeriesID::new(row.series_id.to_string())?,
                tmdb_series_id: row.tmdb_series_id as u64,
                details: MediaDetailsOption::Endpoint(
                    format!("/api/episode/lookup/{}", row.id)
                ),
                endpoint: EpisodeURL::from_string(
                    format!("/api/stream/{}", row.file_id.unwrap_or(row.id))
                ),
                file: media_file,
            };
            episodes.push(episode_ref);
        }
        
        Ok(episodes)
    }

    async fn get_library_movies(&self, library_id: Uuid) -> Result<Vec<MovieReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT mr.id, mr.tmdb_id, mr.title, mr.file_id,
                   mf.file_path, mf.file_name as filename, mf.file_size as size, mf.library_id, mf.created_at as file_created_at,
                   mm.overview, mm.release_date, mm.runtime, mm.vote_average, mm.poster_path
            FROM movie_references mr
            LEFT JOIN media_files mf ON mf.id = mr.file_id
            LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
            WHERE mr.library_id = $1
            ORDER BY mr.title
            "#,
            library_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;
        
        let mut movies = Vec::new();
        for row in rows {
            let media_file = MediaFile {
                id: row.file_id.unwrap_or(Uuid::new_v4()),
                path: row.file_path.into(),
                filename: row.filename,
                size: row.size as u64,
                created_at: row.file_created_at.unwrap_or_else(chrono::Utc::now),
                media_file_metadata: None,
                library_id: row.library_id,
            };
            
            let movie_ref = MovieReference {
                id: MovieID::new(row.id.to_string())?,
                tmdb_id: row.tmdb_id as u64,
                title: MovieTitle::new(row.title)?,
                details: MediaDetailsOption::Endpoint(
                    format!("/api/movie/{}", row.id)
                ),
                endpoint: MovieURL::from_string(
                    format!("/api/stream/{}", row.file_id.unwrap_or(row.id))
                ),
                file: media_file,
            };
            movies.push(movie_ref);
        }
        
        Ok(movies)
    }

    async fn get_library_series(&self, library_id: Uuid) -> Result<Vec<SeriesReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT sr.id, sr.tmdb_id, sr.title,
                   sm.overview, sm.first_air_date, sm.number_of_seasons, sm.vote_average, sm.poster_path
            FROM series_references sr
            LEFT JOIN series_metadata sm ON sm.series_id = sr.id
            WHERE sr.library_id = $1
            ORDER BY sr.title
            "#,
            library_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;
        
        let mut series_list = Vec::new();
        for row in rows {
            let series_ref = SeriesReference {
                id: SeriesID::new(row.id.to_string())?,
                tmdb_id: row.tmdb_id as u64,
                title: SeriesTitle::new(row.title)?,
                details: MediaDetailsOption::Endpoint(
                    format!("/api/series/{}", row.id)
                ),
                endpoint: SeriesURL::from_string(
                    format!("/api/series/{}", row.id)
                ),
            };
            series_list.push(series_ref);
        }
        
        Ok(series_list)
    }

    async fn get_movie_reference(&self, id: &MovieID) -> Result<MovieReference> {
        // TODO: Implement individual movie reference fetching
        Err(MediaError::NotFound("Movie not found".to_string()))
    }

    async fn get_series_reference(&self, id: &SeriesID) -> Result<SeriesReference> {
        // TODO: Implement individual series reference fetching
        Err(MediaError::NotFound("Series not found".to_string()))
    }

    async fn get_season_reference(&self, id: &SeasonID) -> Result<SeasonReference> {
        // TODO: Implement individual season reference fetching
        Err(MediaError::NotFound("Season not found".to_string()))
    }

    async fn get_episode_reference(&self, id: &EpisodeID) -> Result<EpisodeReference> {
        // TODO: Implement individual episode reference fetching
        Err(MediaError::NotFound("Episode not found".to_string()))
    }

    async fn update_movie_tmdb_id(&self, id: &MovieID, tmdb_id: u64) -> Result<()> {
        let movie_uuid = Uuid::parse_str(id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid movie ID: {}", e)))?;
            
        sqlx::query!(
            "UPDATE movie_references SET tmdb_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            tmdb_id as i64,
            movie_uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database update failed: {}", e)))?;
        
        Ok(())
    }

    async fn update_series_tmdb_id(&self, id: &SeriesID, tmdb_id: u64) -> Result<()> {
        let series_uuid = Uuid::parse_str(id.as_str())
            .map_err(|e| MediaError::InvalidMedia(format!("Invalid series ID: {}", e)))?;
            
        sqlx::query!(
            "UPDATE series_references SET tmdb_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            tmdb_id as i64,
            series_uuid
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database update failed: {}", e)))?;
        
        Ok(())
    }

    async fn list_library_references(&self) -> Result<Vec<LibraryReference>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, name, library_type, paths, created_at
            FROM libraries
            WHERE enabled = true
            ORDER BY name
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;
        
        let mut libraries = Vec::new();
        for row in rows {
            let library_type = match row.library_type.as_str() {
                "movies" => crate::LibraryType::Movies,
                "tvshows" => crate::LibraryType::TvShows,
                _ => continue,
            };
            
            let library_ref = LibraryReference {
                id: row.id,
                name: row.name,
                library_type,
                paths: row.paths.into_iter().map(PathBuf::from).collect(),
            };
            libraries.push(library_ref);
        }
        
        Ok(libraries)
    }

    async fn get_library_reference(&self, id: Uuid) -> Result<LibraryReference> {
        let row = sqlx::query!(
            r#"
            SELECT id, name, library_type, paths
            FROM libraries
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;
        
        match row {
            Some(row) => {
                let library_type = match row.library_type.as_str() {
                    "movies" => crate::LibraryType::Movies,
                    "tvshows" => crate::LibraryType::TvShows,
                    _ => return Err(MediaError::InvalidMedia("Unknown library type".to_string())),
                };
                
                Ok(LibraryReference {
                    id: row.id,
                    name: row.name,
                    library_type,
                    paths: row.paths.into_iter().map(PathBuf::from).collect(),
                })
            }
            None => Err(MediaError::NotFound("Library not found".to_string())),
        }
    }
}
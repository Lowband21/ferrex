#[cfg(test)]
mod tests {
    use ferrex_core::media::*;
    use ferrex_core::{MediaFile, FileType};
    use std::path::PathBuf;
    use uuid::Uuid;
    
    fn create_test_media_file() -> MediaFile {
        MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from("/test/movie.mp4"),
            name: "movie.mp4".to_string(),
            size: 1_000_000,
            media_type: Some(FileType::Movie),
            parent_directory: Some(PathBuf::from("/test")),
            library_id: Some(Uuid::new_v4()),
            parent_media_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_scanned: Some(chrono::Utc::now()),
            metadata: None,
        }
    }
    
    #[test]
    fn test_movie_id_validation() {
        // Valid UUID
        let valid_id = MovieID::new(Uuid::new_v4().to_string());
        assert!(valid_id.is_ok());
        
        // Invalid UUID
        let invalid_id = MovieID::new("not-a-uuid".to_string());
        assert!(invalid_id.is_err());
        
        // Empty string
        let empty_id = MovieID::new("".to_string());
        assert!(empty_id.is_err());
    }
    
    #[test]
    fn test_movie_title_validation() {
        // Valid title
        let valid_title = MovieTitle::new("The Matrix");
        assert!(valid_title.is_ok());
        assert_eq!(valid_title.unwrap().as_str(), "The Matrix");
        
        // Empty title
        let empty_title = MovieTitle::new("");
        assert!(empty_title.is_err());
        
        // Whitespace only
        let whitespace_title = MovieTitle::new("   ");
        assert!(whitespace_title.is_err());
    }
    
    #[test]
    fn test_season_number_validation() {
        // Valid season numbers
        let season1 = SeasonNumber::new(1);
        assert_eq!(season1.value(), 1);
        
        let season10 = SeasonNumber::new(10);
        assert_eq!(season10.value(), 10);
        
        // Season 0 is valid (specials)
        let season0 = SeasonNumber::new(0);
        assert_eq!(season0.value(), 0);
        
        // Max value
        let season_max = SeasonNumber::new(255);
        assert_eq!(season_max.value(), 255);
    }
    
    #[test]
    fn test_episode_number_validation() {
        // Valid episode numbers
        let episode1 = EpisodeNumber::new(1);
        assert_eq!(episode1.value(), 1);
        
        let episode24 = EpisodeNumber::new(24);
        assert_eq!(episode24.value(), 24);
        
        // Episode 0 is valid (pilot/special)
        let episode0 = EpisodeNumber::new(0);
        assert_eq!(episode0.value(), 0);
    }
    
    #[test]
    fn test_movie_reference_creation() {
        let file = create_test_media_file();
        let movie_id = MovieID::new(Uuid::new_v4().to_string()).unwrap();
        let title = MovieTitle::new("Test Movie").unwrap();
        let endpoint_url = url::Url::parse(&format!("/api/stream/{}", file.id)).unwrap();
        let movie_url = MovieURL::new(endpoint_url.clone());
        
        let movie_ref = MovieReference {
            id: movie_id.clone(),
            tmdb_id: 12345,
            title: title.clone(),
            details: MediaDetailsOption::Endpoint(
                url::Url::parse("/api/movie/123").unwrap()
            ),
            endpoint: movie_url,
            file: file.clone(),
        };
        
        assert_eq!(movie_ref.id, movie_id);
        assert_eq!(movie_ref.tmdb_id, 12345);
        assert_eq!(movie_ref.title, title);
        assert_eq!(movie_ref.file.id, file.id);
    }
    
    #[test]
    fn test_episode_reference_hierarchy() {
        let file = create_test_media_file();
        let series_id = SeriesID::new(Uuid::new_v4().to_string()).unwrap();
        let season_id = SeasonID::new(Uuid::new_v4().to_string()).unwrap();
        let episode_id = EpisodeID::new(Uuid::new_v4().to_string()).unwrap();
        
        let episode_ref = EpisodeReference {
            id: episode_id.clone(),
            episode_number: EpisodeNumber::new(5),
            season_number: SeasonNumber::new(2),
            season_id: season_id.clone(),
            series_id: series_id.clone(),
            tmdb_series_id: 1399, // Breaking Bad
            details: MediaDetailsOption::Endpoint(
                url::Url::parse("/api/episode/123").unwrap()
            ),
            endpoint: EpisodeURL::new(
                url::Url::parse(&format!("/api/stream/{}", file.id)).unwrap()
            ),
            file,
        };
        
        // Verify hierarchy
        assert_eq!(episode_ref.series_id, series_id);
        assert_eq!(episode_ref.season_id, season_id);
        assert_eq!(episode_ref.episode_number.value(), 5);
        assert_eq!(episode_ref.season_number.value(), 2);
    }
    
    #[test]
    fn test_media_details_option() {
        // Test endpoint variant
        let endpoint_url = url::Url::parse("https://api.example.com/movie/123").unwrap();
        let endpoint_option = MediaDetailsOption::Endpoint(endpoint_url.clone());
        
        match endpoint_option {
            MediaDetailsOption::Endpoint(url) => assert_eq!(url, endpoint_url),
            _ => panic!("Expected Endpoint variant"),
        }
        
        // Test details variant with movie
        let movie_details = tmdb_api::movie::Movie {
            id: 550,
            title: "Fight Club".to_string(),
            overview: "A ticking-time-bomb insomniac...".to_string(),
            release_date: Some("1999-10-15".to_string()),
            poster_path: Some("/poster.jpg".to_string()),
            backdrop_path: Some("/backdrop.jpg".to_string()),
            vote_average: 8.4,
            vote_count: 26000,
            popularity: 60.0,
            adult: false,
            video: false,
            original_language: "en".to_string(),
            original_title: "Fight Club".to_string(),
            genre_ids: vec![18, 53],
            runtime: Some(139),
            status: Some("Released".to_string()),
            tagline: Some("Mischief. Mayhem. Soap.".to_string()),
            homepage: Some("https://www.foxmovies.com/movies/fight-club".to_string()),
            imdb_id: Some("tt0137523".to_string()),
            revenue: Some(100853753),
            budget: Some(63000000),
            belongs_to_collection: None,
            genres: vec![],
            production_companies: vec![],
            production_countries: vec![],
            spoken_languages: vec![],
        };
        
        let details_option = MediaDetailsOption::Details(TmdbDetails::Movie(movie_details));
        
        match details_option {
            MediaDetailsOption::Details(TmdbDetails::Movie(m)) => {
                assert_eq!(m.id, 550);
                assert_eq!(m.title, "Fight Club");
            }
            _ => panic!("Expected Details variant with Movie"),
        }
    }
    
    #[test]
    fn test_media_reference_enum() {
        let file = create_test_media_file();
        let movie = MovieReference {
            id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
            tmdb_id: 550,
            title: MovieTitle::new("Fight Club").unwrap(),
            details: MediaDetailsOption::Endpoint(
                url::Url::parse("/api/movie/123").unwrap()
            ),
            endpoint: MovieURL::new(
                url::Url::parse(&format!("/api/stream/{}", file.id)).unwrap()
            ),
            file,
        };
        
        let media_ref = MediaReference::Movie(movie.clone());
        
        match media_ref {
            MediaReference::Movie(m) => {
                assert_eq!(m.id, movie.id);
                assert_eq!(m.tmdb_id, movie.tmdb_id);
            }
            _ => panic!("Expected Movie variant"),
        }
    }
    
    #[test]
    fn test_media_id_enum() {
        let movie_id = MovieID::new(Uuid::new_v4().to_string()).unwrap();
        let media_id = MediaId::Movie(movie_id.clone());
        
        match media_id {
            MediaId::Movie(id) => assert_eq!(id, movie_id),
            _ => panic!("Expected Movie variant"),
        }
    }
}
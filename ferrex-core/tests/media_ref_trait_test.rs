use ferrex_core::media::*;
use ferrex_core::{MovieID, SeriesID, SeasonID, EpisodeID, MediaId};
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn test_media_ref_trait_basic_usage() {
    // Create a movie reference
    let movie = MovieReference {
        id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
        tmdb_id: 550,
        title: MovieTitle::from("Fight Club"),
        details: MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
            id: 550,
            title: "Fight Club".to_string(),
            overview: Some("An insomniac office worker...".to_string()),
            release_date: Some("1999-10-15".to_string()),
            runtime: Some(139),
            vote_average: Some(8.4),
            vote_count: Some(26280),
            popularity: Some(61.416),
            genres: vec!["Drama".to_string(), "Thriller".to_string()],
            production_companies: vec![],
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            images: MediaImages::default(),
            cast: vec![],
            crew: vec![],
            videos: vec![],
            keywords: vec![],
            external_ids: ExternalIds::default(),
        })),
        endpoint: MovieURL::from_string("/api/v1/movies/550".to_string()),
        file: MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from("/movies/Fight.Club.1999.mkv"),
            filename: "Fight.Club.1999.mkv".to_string(),
            size: 2_000_000_000,
            created_at: Utc::now(),
            media_file_metadata: None,
            library_id: Uuid::new_v4(),
        },
        theme_color: Some("#2C3E50".to_string()),
    };
    
    // Test trait methods directly
    assert_eq!(movie.title(), "Fight Club");
    assert_eq!(movie.year(), Some(1999));
    assert_eq!(movie.rating(), Some(8.4));
    assert_eq!(movie.genres(), vec!["Drama", "Thriller"]);
    assert!(movie.theme_color().is_some());
    
    // Test through MediaReference enum
    let media_ref = MediaReference::Movie(movie.clone());
    
    // Use trait methods via as_ref()
    assert_eq!(media_ref.as_ref().title(), "Fight Club");
    assert_eq!(media_ref.as_ref().year(), Some(1999));
    assert_eq!(media_ref.as_ref().rating(), Some(8.4));
    
    // Backward compatibility - existing methods still work
    assert_eq!(media_ref.title(), "Fight Club");
    assert_eq!(media_ref.year(), Some(1999));
    assert_eq!(media_ref.rating(), Some(8.4));
}

#[test]
fn test_playable_trait() {
    let movie = MovieReference {
        id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
        tmdb_id: 550,
        title: MovieTitle::from("Fight Club"),
        details: MediaDetailsOption::Endpoint("/api/v1/movies/550".to_string()),
        endpoint: MovieURL::from_string("/api/v1/movies/550".to_string()),
        file: MediaFile {
            id: Uuid::new_v4(),
            path: PathBuf::from("/movies/Fight.Club.1999.mkv"),
            filename: "Fight.Club.1999.mkv".to_string(),
            size: 2_000_000_000,
            created_at: Utc::now(),
            media_file_metadata: Some(MediaFileMetadata {
                duration: Some(8340.0), // 139 minutes in seconds
                width: Some(1920),
                height: Some(1080),
                video_codec: Some("h264".to_string()),
                audio_codec: Some("aac".to_string()),
                bitrate: Some(5_000_000),
                framerate: Some(23.976),
                file_size: 2_000_000_000,
                color_primaries: None,
                color_transfer: None,
                color_space: None,
                bit_depth: Some(8),
                parsed_info: None,
            }),
            library_id: Uuid::new_v4(),
        },
        theme_color: None,
    };
    
    // Test as Playable
    let media_ref = MediaReference::Movie(movie);
    
    if let Some(playable) = media_ref.as_playable() {
        assert_eq!(playable.file().filename, "Fight.Club.1999.mkv");
        assert_eq!(playable.duration().unwrap().as_secs(), 8340);
        assert!(playable.can_transcode());
    } else {
        panic!("Movie should be playable");
    }
    
    // Series should not be playable
    let series = SeriesReference {
        id: SeriesID::new(Uuid::new_v4().to_string()).unwrap(),
        library_id: Uuid::new_v4(),
        tmdb_id: 1396,
        title: SeriesTitle::from("Breaking Bad"),
        details: MediaDetailsOption::Endpoint("/api/v1/series/1396".to_string()),
        endpoint: SeriesURL::from_string("/api/v1/series/1396".to_string()),
        created_at: Utc::now(),
        theme_color: None,
    };
    
    let series_ref = MediaReference::Series(series);
    assert!(series_ref.as_playable().is_none());
}

#[test]
fn test_browsable_trait() {
    let series = SeriesReference {
        id: SeriesID::new(Uuid::new_v4().to_string()).unwrap(),
        library_id: Uuid::new_v4(),
        tmdb_id: 1396,
        title: SeriesTitle::from("Breaking Bad"),
        created_at: Utc::now(),
        details: MediaDetailsOption::Details(TmdbDetails::Series(EnhancedSeriesDetails {
            id: 1396,
            name: "Breaking Bad".to_string(),
            overview: Some("When Walter White...".to_string()),
            first_air_date: Some("2008-01-20".to_string()),
            last_air_date: Some("2013-09-29".to_string()),
            number_of_seasons: Some(5),
            number_of_episodes: Some(62),
            vote_average: Some(8.9),
            vote_count: Some(11507),
            popularity: Some(203.869),
            genres: vec!["Drama".to_string(), "Crime".to_string()],
            networks: vec!["AMC".to_string()],
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            images: MediaImages::default(),
            cast: vec![],
            crew: vec![],
            videos: vec![],
            keywords: vec![],
            external_ids: ExternalIds::default(),
        })),
        endpoint: SeriesURL::from_string("/api/v1/series/1396".to_string()),
        theme_color: None,
    };
    
    let series_ref = MediaReference::Series(series.clone());
    
    if let Some(browsable) = series_ref.as_browsable() {
        assert_eq!(browsable.child_count(), Some(62));
        assert_eq!(browsable.library_id(), series.library_id);
    } else {
        panic!("Series should be browsable");
    }
    
    // Movies should not be browsable
    let movie = MovieReference {
        id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
        tmdb_id: 550,
        title: MovieTitle::from("Fight Club"),
        details: MediaDetailsOption::Endpoint("/api/v1/movies/550".to_string()),
        endpoint: MovieURL::from_string("/api/v1/movies/550".to_string()),
        file: MediaFile::default(),
        theme_color: None,
    };
    
    let movie_ref = MediaReference::Movie(movie);
    assert!(movie_ref.as_browsable().is_none());
}

#[test]
fn test_type_specific_accessors() {
    let movie = MovieReference {
        id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
        tmdb_id: 550,
        title: MovieTitle::from("Fight Club"),
        details: MediaDetailsOption::Endpoint("/api/v1/movies/550".to_string()),
        endpoint: MovieURL::from_string("/api/v1/movies/550".to_string()),
        file: MediaFile::default(),
        theme_color: None,
    };
    
    let movie_ref = MediaReference::Movie(movie.clone());
    
    // Type-specific accessors
    assert!(movie_ref.as_movie().is_some());
    assert_eq!(movie_ref.as_movie().unwrap().tmdb_id, 550);
    assert!(movie_ref.as_series().is_none());
    assert!(movie_ref.as_season().is_none());
    assert!(movie_ref.as_episode().is_none());
    
    // Media type helper
    assert_eq!(movie_ref.media_type(), "movie");
}

#[test]
fn test_sorting_with_traits() {
    let mut media_items = vec![
        MediaReference::Movie(MovieReference {
            id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
            tmdb_id: 550,
            title: MovieTitle::from("Fight Club"),
            details: MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
                id: 550,
                title: "Fight Club".to_string(),
                overview: None,
                release_date: Some("1999-10-15".to_string()),
                runtime: None,
                vote_average: Some(8.4),
                vote_count: None,
                popularity: None,
                genres: vec![],
                production_companies: vec![],
                poster_path: None,
                backdrop_path: None,
                logo_path: None,
                images: MediaImages::default(),
                cast: vec![],
                crew: vec![],
                videos: vec![],
                keywords: vec![],
                external_ids: ExternalIds::default(),
            })),
            endpoint: MovieURL::from_string("/api/v1/movies/550".to_string()),
            file: MediaFile::default(),
            theme_color: None,
        }),
        MediaReference::Movie(MovieReference {
            id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
            tmdb_id: 24428,
            title: MovieTitle::from("The Avengers"),
            details: MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
                id: 24428,
                title: "The Avengers".to_string(),
                overview: None,
                release_date: Some("2012-04-25".to_string()),
                runtime: None,
                vote_average: Some(7.7),
                vote_count: None,
                popularity: None,
                genres: vec![],
                production_companies: vec![],
                poster_path: None,
                backdrop_path: None,
                logo_path: None,
                images: MediaImages::default(),
                cast: vec![],
                crew: vec![],
                videos: vec![],
                keywords: vec![],
                external_ids: ExternalIds::default(),
            })),
            endpoint: MovieURL::from_string("/api/v1/movies/24428".to_string()),
            file: MediaFile::default(),
            theme_color: None,
        }),
    ];
    
    // Sort using the new trait-based approach
    media_items.sort_by_key(|m| m.as_ref().title().to_string());
    assert_eq!(media_items[0].as_ref().title(), "Fight Club");
    assert_eq!(media_items[1].as_ref().title(), "The Avengers");
    
    // Sort by rating using trait methods
    media_items.sort_by(|a, b| {
        b.as_ref().rating()
            .partial_cmp(&a.as_ref().rating())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    assert_eq!(media_items[0].as_ref().rating(), Some(8.4));
    assert_eq!(media_items[1].as_ref().rating(), Some(7.7));
}
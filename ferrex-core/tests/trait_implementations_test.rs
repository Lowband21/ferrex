use ferrex_core::media::*;
use std::collections::HashSet;

#[test]
fn test_display_implementations() {
    // Test ID types Display
    let movie_id = MovieID::new("movie-123".to_string()).unwrap();
    assert_eq!(format!("{}", movie_id), "movie-123");
    
    let series_id = SeriesID::new("series-456".to_string()).unwrap();
    assert_eq!(format!("{}", series_id), "series-456");
    
    // Test Title types Display
    let movie_title = MovieTitle::from("The Matrix");
    assert_eq!(format!("{}", movie_title), "The Matrix");
    
    let series_title = SeriesTitle::from("Breaking Bad");
    assert_eq!(format!("{}", series_title), "Breaking Bad");
    
    // Test Number types Display
    let season_num = SeasonNumber::from(3);
    assert_eq!(format!("{}", season_num), "3");
    
    let episode_num = EpisodeNumber::from(7);
    assert_eq!(format!("{}", episode_num), "7");
    
    // Test URL types Display
    let movie_url = MovieURL::from_string("/api/v1/movies/550".to_string());
    assert_eq!(format!("{}", movie_url), "/api/v1/movies/550");
}

#[test]
fn test_from_implementations() {
    // Test From<String> for titles
    let movie_title = MovieTitle::from("Inception".to_string());
    assert_eq!(movie_title.as_str(), "Inception");
    
    // Test From<&str> for titles
    let series_title = SeriesTitle::from("The Wire");
    assert_eq!(series_title.as_str(), "The Wire");
    
    // Test From<u8> for numbers
    let season = SeasonNumber::from(2u8);
    assert_eq!(season.value(), 2);
    
    let episode = EpisodeNumber::from(10u8);
    assert_eq!(episode.value(), 10);
}

#[test]
fn test_hash_implementations() {
    // Test that types can be used in HashSet
    let mut movie_ids = HashSet::new();
    movie_ids.insert(MovieID::new("movie-1".to_string()).unwrap());
    movie_ids.insert(MovieID::new("movie-2".to_string()).unwrap());
    movie_ids.insert(MovieID::new("movie-1".to_string()).unwrap()); // Duplicate
    assert_eq!(movie_ids.len(), 2); // Should only have 2 unique IDs
    
    let mut titles = HashSet::new();
    titles.insert(MovieTitle::from("Star Wars"));
    titles.insert(MovieTitle::from("Star Trek"));
    titles.insert(MovieTitle::from("Star Wars")); // Duplicate
    assert_eq!(titles.len(), 2);
}

#[test]
fn test_ordering_implementations() {
    // Test title ordering
    let mut titles = vec![
        MovieTitle::from("Zootopia"),
        MovieTitle::from("Avatar"),
        MovieTitle::from("Matrix"),
    ];
    titles.sort();
    assert_eq!(titles[0].as_str(), "Avatar");
    assert_eq!(titles[1].as_str(), "Matrix");
    assert_eq!(titles[2].as_str(), "Zootopia");
    
    // Test season/episode number ordering
    let mut seasons = vec![
        SeasonNumber::from(3),
        SeasonNumber::from(1),
        SeasonNumber::from(2),
    ];
    seasons.sort();
    assert_eq!(seasons[0].value(), 1);
    assert_eq!(seasons[1].value(), 2);
    assert_eq!(seasons[2].value(), 3);
}

#[test]
fn test_default_implementations() {
    // Test default season number
    let default_season = SeasonNumber::default();
    assert_eq!(default_season.value(), 1);
    
    // Test default episode number
    let default_episode = EpisodeNumber::default();
    assert_eq!(default_episode.value(), 1);
    
    // Test default media images
    let default_images = MediaImages::default();
    assert!(default_images.posters.is_empty());
    assert!(default_images.backdrops.is_empty());
    assert!(default_images.logos.is_empty());
    assert!(default_images.stills.is_empty());
}

#[test]
fn test_asref_implementations() {
    // Test AsRef<str> for titles
    let movie_title = MovieTitle::from("Interstellar");
    let title_ref: &str = movie_title.as_ref();
    assert_eq!(title_ref, "Interstellar");
    
    // Test AsRef<str> for URLs
    let movie_url = MovieURL::from_string("/api/movies/123".to_string());
    let url_ref: &str = movie_url.as_ref();
    assert_eq!(url_ref, "/api/movies/123");
}
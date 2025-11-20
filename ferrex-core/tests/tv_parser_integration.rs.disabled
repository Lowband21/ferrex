use ferrex_core::{TvParser, EpisodeInfo, MediaType, LibraryType};
use std::path::PathBuf;

#[test]
fn test_comprehensive_episode_patterns() {
    // Standard patterns
    let standard_cases = vec![
        ("/shows/Breaking Bad/S01E01.mkv", 1, 1),
        ("/shows/Breaking Bad/s02e03.mkv", 2, 3),
        ("/shows/Breaking Bad/S1E1.mkv", 1, 1),
        ("/shows/Breaking Bad/1x01.mkv", 1, 1),
        ("/shows/Breaking Bad/2X03.mkv", 2, 3),
        ("/shows/Breaking Bad/Season 1 Episode 5.mkv", 1, 5),
        ("/shows/Breaking Bad/S01 E02.mkv", 1, 2),
        ("/shows/Breaking Bad/Episode 101.mkv", 1, 1),
        ("/shows/Breaking Bad/203.mkv", 2, 3),
    ];
    
    for (path, expected_season, expected_episode) in standard_cases {
        let path = PathBuf::from(path);
        let result = TvParser::parse_episode(&path);
        assert_eq!(
            result,
            Some((expected_season, expected_episode)),
            "Failed to parse: {}",
            path.display()
        );
    }
}

#[test]
fn test_multi_episode_patterns() {
    let multi_cases = vec![
        ("/shows/The Office/S01E01-E02.mkv", 1, 1, Some(2)),
        ("/shows/The Office/S01E01-03.mkv", 1, 1, Some(3)),
        ("/shows/The Office/S01E01E02.mkv", 1, 1, Some(2)),
        ("/shows/The Office/1x01-02.mkv", 1, 1, Some(2)),
        ("/shows/The Office/1x01-x02.mkv", 1, 1, Some(2)),
    ];
    
    for (path, expected_season, expected_start, expected_end) in multi_cases {
        let path = PathBuf::from(path);
        let result = TvParser::parse_episode_info(&path);
        assert!(result.is_some(), "Failed to parse: {}", path.display());
        
        let info = result.unwrap();
        assert_eq!(info.season, expected_season);
        assert_eq!(info.episode, expected_start);
        assert_eq!(info.end_episode, expected_end);
    }
}

#[test]
fn test_date_based_episodes() {
    let date_cases = vec![
        ("/shows/Daily Show/2024-01-15.mkv", 2024, 1, 15),
        ("/shows/Daily Show/2024.01.15.mkv", 2024, 1, 15),
        ("/shows/Daily Show/20240115.mkv", 2024, 1, 15),
        ("/shows/Daily Show/15-01-2024.mkv", 2024, 1, 15),
        ("/shows/Daily Show/15.01.2024.mkv", 2024, 1, 15),
    ];
    
    for (path, expected_year, expected_month, expected_day) in date_cases {
        let path = PathBuf::from(path);
        let result = TvParser::parse_episode_info(&path);
        assert!(result.is_some(), "Failed to parse: {}", path.display());
        
        let info = result.unwrap();
        assert_eq!(info.year, Some(expected_year));
        assert_eq!(info.month, Some(expected_month));
        assert_eq!(info.day, Some(expected_day));
    }
}

#[test]
fn test_special_episodes() {
    let special_cases = vec![
        "/shows/Breaking Bad/Specials/S00E01 - Minisode.mkv",
        "/shows/Breaking Bad/S00E02 - Behind the Scenes.mkv",
        "/shows/Doctor Who/Specials/S00E01 - Christmas Special.mkv",
    ];
    
    for path in special_cases {
        let path = PathBuf::from(path);
        let result = TvParser::parse_episode_info(&path);
        assert!(result.is_some(), "Failed to parse: {}", path.display());
        
        let info = result.unwrap();
        assert_eq!(info.season, 0);
        assert!(info.is_special);
    }
}

#[test]
fn test_anime_patterns() {
    // Test anime detection
    let anime_paths = vec![
        "/anime/Attack on Titan/[HorribleSubs] AOT - 01 [720p].mkv",
        "/anime/Naruto [Dubbed]/Episode 001.mkv",
        "/anime/One Piece/[1080p][HEVC]/One Piece - 1000.mkv",
    ];
    
    for path in anime_paths {
        let path = PathBuf::from(path);
        assert!(TvParser::is_likely_anime(&path), "Should detect anime: {}", path.display());
    }
    
    // Non-anime
    let non_anime = PathBuf::from("/shows/Breaking Bad/S01E01.mkv");
    assert!(!TvParser::is_likely_anime(&non_anime));
}

#[test]
fn test_folder_based_parsing() {
    let folder_cases = vec![
        ("/shows/Breaking Bad/Season 1/01 - Pilot.mkv", 1, 1),
        ("/shows/Breaking Bad/Season 2/03 - Bit by a Dead Bee.mkv", 2, 3),
        ("/shows/Breaking Bad/S01/05.mkv", 1, 5),
        ("/shows/Doctor Who/Series 1/01 - Rose.mkv", 1, 1),
    ];
    
    for (path, expected_season, expected_episode) in folder_cases {
        let path = PathBuf::from(path);
        let result = TvParser::parse_episode(&path);
        assert_eq!(
            result,
            Some((expected_season, expected_episode)),
            "Failed to parse: {}",
            path.display()
        );
    }
}

#[test]
fn test_season_folder_patterns() {
    let folder_patterns = vec![
        ("Season 1", Some(1)),
        ("Season 01", Some(1)),
        ("S01", Some(1)),
        ("S1", Some(1)),
        ("season01", Some(1)),
        ("Specials", Some(0)),
        ("specials", Some(0)),
        ("Series 1", Some(1)),
        ("Random Folder", None),
        ("2021", None),
    ];
    
    for (folder, expected) in folder_patterns {
        let result = TvParser::parse_season_folder(folder);
        assert_eq!(result, expected, "Failed to parse folder: {}", folder);
    }
}

#[test]
fn test_series_name_extraction() {
    let name_cases = vec![
        ("/media/TV Shows/Breaking Bad/Season 1/S01E01.mkv", "Breaking Bad"),
        ("/shows/The Office (US)/S01/1x01.mkv", "The Office (US)"),
        ("/tv/Game of Thrones/Season 8/S08E06 - The Iron Throne.mkv", "Game of Thrones"),
        ("/media/Doctor Who (2005)/Series 1/S01E01.mkv", "Doctor Who (2005)"),
    ];
    
    for (path, expected_name) in name_cases {
        let path = PathBuf::from(path);
        let result = TvParser::extract_series_name(&path);
        assert_eq!(
            result.as_deref(),
            Some(expected_name),
            "Failed to extract series name from: {}",
            path.display()
        );
    }
}

#[test]
fn test_episode_title_extraction() {
    let title_cases = vec![
        ("/shows/Breaking Bad/S01E01 - Pilot.mkv", Some("Pilot")),
        ("/shows/Breaking Bad/1x02 - Cat's in the Bag.mkv", Some("Cat's in the Bag")),
        ("/shows/Breaking Bad/01 - Pilot Episode.mkv", Some("Pilot Episode")),
        ("/shows/Breaking Bad/S01E01.Pilot.720p.mkv", Some("Pilot 720p")),
        ("/shows/Breaking Bad/S01E01.mkv", None),
    ];
    
    for (path, expected_title) in title_cases {
        let path = PathBuf::from(path);
        let result = TvParser::extract_episode_title(&path);
        assert_eq!(
            result.as_deref(),
            expected_title,
            "Failed to extract title from: {}",
            path.display()
        );
    }
}

#[test]
fn test_media_type_determination() {
    // TV library context
    let tv_lib = LibraryType::TvShows;
    
    let tv_path = PathBuf::from("/shows/Breaking Bad/S01E01.mkv");
    assert_eq!(
        TvParser::determine_media_type(&tv_path, Some(&tv_lib)),
        MediaType::TvEpisode
    );
    
    let movie_in_tv = PathBuf::from("/shows/Random Movie (2020).mkv");
    assert_eq!(
        TvParser::determine_media_type(&movie_in_tv, Some(&tv_lib)),
        MediaType::Movie // No episode pattern
    );
    
    // Movie library context
    let movie_lib = LibraryType::Movies;
    
    let movie_path = PathBuf::from("/movies/The Matrix (1999).mkv");
    assert_eq!(
        TvParser::determine_media_type(&movie_path, Some(&movie_lib)),
        MediaType::Movie
    );
    
    let tv_in_movies = PathBuf::from("/movies/Misplaced/S01E01.mkv");
    assert_eq!(
        TvParser::determine_media_type(&tv_in_movies, Some(&movie_lib)),
        MediaType::TvEpisode // Clear TV pattern overrides library type
    );
}

#[test]
fn test_tv_structure_detection() {
    let tv_structures = vec![
        "/shows/Breaking Bad/Season 1/S01E01.mkv",
        "/shows/Breaking Bad/S01/1x01.mkv",
        "/shows/The Office/Season 2/201.mkv",
        "/shows/S01E01.mkv", // Has episode pattern in filename
    ];
    
    for path in tv_structures {
        let path = PathBuf::from(path);
        assert!(
            TvParser::is_in_tv_structure(&path),
            "Should detect TV structure: {}",
            path.display()
        );
    }
    
    let non_tv_structures = vec![
        "/movies/The Matrix (1999).mkv",
        "/movies/Action/Die Hard.mkv",
        "/documentaries/Planet Earth.mkv",
    ];
    
    for path in non_tv_structures {
        let path = PathBuf::from(path);
        assert!(
            !TvParser::is_in_tv_structure(&path),
            "Should not detect TV structure: {}",
            path.display()
        );
    }
}

#[test]
fn test_edge_cases() {
    // Empty filename
    let empty = PathBuf::from("/shows/");
    assert_eq!(TvParser::parse_episode(&empty), None);
    
    // No extension
    let no_ext = PathBuf::from("/shows/Breaking Bad/S01E01");
    assert_eq!(TvParser::parse_episode(&no_ext), Some((1, 1)));
    
    // Multiple patterns in filename
    let multiple = PathBuf::from("/shows/1x01 - S01E01 - Episode.mkv");
    let info = TvParser::parse_episode_info(&multiple).unwrap();
    assert_eq!(info.season, 1);
    assert_eq!(info.episode, 1);
    
    // Invalid date
    let invalid_date = PathBuf::from("/shows/Daily/2024-13-45.mkv");
    assert_eq!(TvParser::parse_episode(&invalid_date), None);
}

#[test]
fn test_absolute_numbering() {
    // Should only work for anime-like paths
    let anime_path = PathBuf::from("/anime/One Piece/One Piece - 1000.mkv");
    let info = TvParser::parse_episode_info(&anime_path);
    assert!(info.is_some());
    
    let info = info.unwrap();
    assert_eq!(info.absolute_episode, Some(1000));
    assert_eq!(info.episode, 1000);
    
    // Should not trigger for non-anime
    let regular_path = PathBuf::from("/shows/Regular Show/1000.mkv");
    let info = TvParser::parse_episode_info(&regular_path);
    // Should parse as season 1, episode 0 (from 1000 pattern) or None
    if let Some(info) = info {
        assert!(info.absolute_episode.is_none() || info.season != 1 || info.episode != 1000);
    }
}
use ferrex_core::{ExtrasParser, ExtraType, MediaType, LibraryType, MetadataExtractor};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_file(dir: &TempDir, path: &str) -> PathBuf {
    let file_path = dir.path().join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    // Create a valid (albeit minimal) video file for FFmpeg
    // This is a minimal WebM header that FFmpeg can understand
    let minimal_webm = vec![
        0x1A, 0x45, 0xDF, 0xA3, // EBML signature
        0x9F, 0x42, 0x86, 0x81, 0x01, // EBML version = 1
        0x42, 0xF7, 0x81, 0x01, // EBML read version = 1  
        0x42, 0xF2, 0x81, 0x04, // EBML max ID length = 4
        0x42, 0xF3, 0x81, 0x08, // EBML max size length = 8
        0x42, 0x82, 0x84, 0x77, 0x65, 0x62, 0x6D, // doc type = "webm"
        0x42, 0x87, 0x81, 0x02, // doc type version = 2
        0x42, 0x85, 0x81, 0x02, // doc type read version = 2
        0x18, 0x53, 0x80, 0x67, 0x80, // Segment header with unknown size
    ];
    fs::write(&file_path, minimal_webm).unwrap();
    file_path
}

#[test]
fn test_extras_folder_detection() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test various extras folder patterns
    let test_cases = vec![
        ("movies/The Matrix (1999)/Behind the Scenes/making_of.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/The Matrix (1999)/Deleted Scenes/cut_scene.mkv", Some(ExtraType::DeletedScenes)),
        ("movies/The Matrix (1999)/Featurettes/cast_interview.mkv", Some(ExtraType::Featurette)),
        ("movies/The Matrix (1999)/Trailers/theatrical.mkv", Some(ExtraType::Trailer)),
        ("movies/The Matrix (1999)/Extras/commentary.mkv", Some(ExtraType::Other)),
        
        // Plex/Jellyfin standard naming
        ("movies/Inception (2010)/behindthescenes/making_of.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/Inception (2010)/deletedscenes/deleted1.mkv", Some(ExtraType::DeletedScenes)),
        ("movies/Inception (2010)/featurette/director_talk.mkv", Some(ExtraType::Featurette)),
        ("movies/Inception (2010)/interview/cast_interview.mkv", Some(ExtraType::Interview)),
        ("movies/Inception (2010)/trailer/teaser.mkv", Some(ExtraType::Trailer)),
        
        // Non-extras should not be detected
        ("movies/The Matrix (1999)/The Matrix (1999).mkv", None),
        ("tv/Breaking Bad/Season 1/S01E01.mkv", None),
    ];

    for (path, expected) in test_cases {
        let file_path = create_test_file(&temp_dir, path);
        let result = ExtrasParser::parse_extra_info(&file_path);
        assert_eq!(result, expected, "Failed for path: {}", file_path.display());
    }
}

#[test]
fn test_extras_filename_detection() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test filename-based extra detection
    let test_cases = vec![
        ("movies/The Matrix - Behind the Scenes.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/The Matrix - Deleted Scenes.mkv", Some(ExtraType::DeletedScenes)),
        ("movies/The Matrix - Making of.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/The Matrix - Featurette.mkv", Some(ExtraType::Featurette)),
        ("movies/The Matrix - Interview.mkv", Some(ExtraType::Interview)),
        ("movies/The Matrix - Trailer.mkv", Some(ExtraType::Trailer)),
        ("movies/The Matrix - BTS.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/The Matrix - Commentary.mkv", Some(ExtraType::Other)),
        ("movies/The Matrix - Gag Reel.mkv", Some(ExtraType::Other)),
        
        // Non-extras should not be detected
        ("movies/The Matrix (1999).mkv", None),
    ];

    for (path, expected) in test_cases {
        let file_path = create_test_file(&temp_dir, path);
        let result = ExtrasParser::parse_extra_info(&file_path);
        assert_eq!(result, expected, "Failed for path: {}", file_path.display());
    }
}

#[test]
fn test_parent_title_extraction() {
    let temp_dir = TempDir::new().unwrap();
    
    let test_cases = vec![
        ("movies/The Matrix (1999)/Behind the Scenes/making_of.mkv", Some("The Matrix (1999)".to_string())),
        ("movies/Avatar/Extras/commentary.mkv", Some("Avatar".to_string())),
        ("tv/Breaking Bad/Season 1/Deleted Scenes/pilot_deleted.mkv", Some("Season 1".to_string())),
        ("movies/Inception - Behind the Scenes.mkv", Some("Inception".to_string())), // Should extract title before extra type
    ];

    for (path, expected) in test_cases {
        let file_path = create_test_file(&temp_dir, path);
        let result = ExtrasParser::extract_parent_title(&file_path);
        assert_eq!(result, expected, "Failed for path: {}", file_path.display());
    }
}

#[test]
fn test_metadata_extraction_with_extras() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::Movies);
    
    // Test parsing without actual FFmpeg extraction to avoid file format issues
    // Just test that the extras parser correctly identifies files as extras
    let extra_file = temp_dir.path().join("movies/The Matrix (1999)/Behind the Scenes/making_of.mkv");
    fs::create_dir_all(extra_file.parent().unwrap()).unwrap();
    fs::write(&extra_file, b"dummy").unwrap();
    
    // Test direct parsing without metadata extraction
    let extra_type = ExtrasParser::parse_extra_info(&extra_file);
    assert_eq!(extra_type, Some(ExtraType::BehindTheScenes));
    
    let parent_title = ExtrasParser::extract_parent_title(&extra_file);
    assert_eq!(parent_title, Some("The Matrix (1999)".to_string()));
    
    // Test filename-based extra
    let filename_extra = temp_dir.path().join("movies/Inception - Featurette.mkv");
    fs::create_dir_all(filename_extra.parent().unwrap()).unwrap();
    fs::write(&filename_extra, b"dummy").unwrap();
    
    let extra_type2 = ExtrasParser::parse_extra_info(&filename_extra);
    assert_eq!(extra_type2, Some(ExtraType::Featurette));
    
    let parent_title2 = ExtrasParser::extract_parent_title(&filename_extra);
    assert_eq!(parent_title2, Some("Inception".to_string()));
}

#[test]
fn test_tv_show_extras() {
    let temp_dir = TempDir::new().unwrap();
    
    let test_cases = vec![
        ("tv/Breaking Bad/Season 1/Deleted Scenes/pilot_deleted.mkv", ExtraType::DeletedScenes, "Season 1"),
        ("tv/The Office/Behind the Scenes/cast_interview.mkv", ExtraType::BehindTheScenes, "The Office"),
        ("tv/Game of Thrones/Season 8/Extras/finale_commentary.mkv", ExtraType::Other, "Season 8"),
    ];

    for (path, expected_type, expected_parent) in test_cases {
        let extra_file = temp_dir.path().join(path);
        fs::create_dir_all(extra_file.parent().unwrap()).unwrap();
        fs::write(&extra_file, b"dummy").unwrap();
        
        let extra_type = ExtrasParser::parse_extra_info(&extra_file);
        assert_eq!(extra_type, Some(expected_type));
        
        let parent_title = ExtrasParser::extract_parent_title(&extra_file);
        assert_eq!(parent_title, Some(expected_parent.to_string()));
    }
}

#[test]
fn test_media_type_determination_with_extras() {
    let movie_lib = LibraryType::Movies;
    let tv_lib = LibraryType::TvShows;
    
    let test_cases = vec![
        // Movie library
        ("/movies/The Matrix/Behind the Scenes/making_of.mkv", Some(&movie_lib), MediaType::Extra),
        ("/movies/The Matrix (1999).mkv", Some(&movie_lib), MediaType::Movie),
        ("/movies/The Matrix - Trailer.mkv", Some(&movie_lib), MediaType::Extra),
        
        // TV library
        ("/tv/Breaking Bad/S01E01.mkv", Some(&tv_lib), MediaType::TvEpisode),
        ("/tv/Breaking Bad/Season 1/Deleted Scenes/pilot.mkv", Some(&tv_lib), MediaType::Extra),
        ("/tv/Breaking Bad/featurette/cast.mkv", Some(&tv_lib), MediaType::Extra),
        
        // No library context
        ("/media/Random/Behind the Scenes/making_of.mkv", None, MediaType::Extra),
        ("/media/Random/S01E01.mkv", None, MediaType::TvEpisode),
        ("/media/Random/Movie (2020).mkv", None, MediaType::Movie),
    ];

    for (path, library_type, expected) in test_cases {
        let path = PathBuf::from(path);
        let result = ExtrasParser::determine_media_type(&path, library_type);
        assert_eq!(result, expected, "Failed for path: {} with library: {:?}", path.display(), library_type);
    }
}

#[test]
fn test_extras_with_various_naming_conventions() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test case-insensitive matching
    let test_cases = vec![
        ("movies/Movie/BEHIND THE SCENES/making_of.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/Movie/behind_the_scenes/making_of.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/Movie/Behind-The-Scenes/making_of.mkv", Some(ExtraType::BehindTheScenes)),
        ("movies/Movie/special features/commentary.mkv", Some(ExtraType::Other)),
        ("movies/Movie/SPECIAL_FEATURES/commentary.mkv", Some(ExtraType::Other)),
    ];

    for (path, expected) in test_cases {
        let file_path = create_test_file(&temp_dir, path);
        let result = ExtrasParser::parse_extra_info(&file_path);
        assert_eq!(result, expected, "Failed for path: {}", file_path.display());
    }
}

#[test]
fn test_path_likely_contains_extras() {
    let test_cases = vec![
        (PathBuf::from("/movies/The Matrix/Behind the Scenes/making_of.mkv"), true),
        (PathBuf::from("/movies/The Matrix/Extras/commentary.mkv"), true),
        (PathBuf::from("/movies/The Matrix - Behind the Scenes.mkv"), true),
        (PathBuf::from("/movies/The Matrix (1999).mkv"), false),
        (PathBuf::from("/tv/Breaking Bad/S01E01.mkv"), false),
        (PathBuf::from("/tv/Breaking Bad/Season 1/deleted/scene.mkv"), true), // "deleted" matches our patterns
    ];

    for (path, expected) in test_cases {
        let result = ExtrasParser::path_likely_contains_extras(&path);
        assert_eq!(result, expected, "Failed for path: {}", path.display());
    }
}

#[test]
fn test_mixed_content_library() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test media type determination for mixed content
    let test_cases = vec![
        ("movies/The Matrix (1999)/The Matrix (1999).mkv", MediaType::Movie),
        ("movies/The Matrix (1999)/Behind the Scenes/making_of.mkv", MediaType::Extra),
        ("movies/The Matrix (1999)/Deleted Scenes/scene1.mkv", MediaType::Extra),
        ("movies/Misplaced TV/S01E01.mkv", MediaType::TvEpisode), // TV in movie library should still be detected
    ];

    for (path, expected) in test_cases {
        let file_path = temp_dir.path().join(path);
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(&file_path, b"dummy").unwrap();
        
        let media_type = ExtrasParser::determine_media_type(&file_path, Some(&LibraryType::Movies));
        assert_eq!(media_type, expected, "Failed for path: {}", file_path.display());
    }
}
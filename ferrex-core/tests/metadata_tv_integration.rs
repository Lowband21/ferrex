use ferrex_core::{
    metadata::FilenameParser,
    types::{LibraryType, ParsedMediaInfo},
};
use std::path::PathBuf;

#[test]
fn test_metadata_extraction_with_tv_library_context() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);

    // Standard TV episode
    let tv_file =
        PathBuf::from("TV Shows/Breaking Bad/Season 1/S01E01 - Pilot.mkv");
    let parsed_info = parser
        .parse_filename_with_type(&tv_file)
        .expect("should parse");
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        assert_eq!(episode.show_name, "Breaking Bad");
        assert_eq!(episode.season, 1);
        assert_eq!(episode.episode, 1);
        assert_eq!(episode.episode_title.as_deref(), Some("Pilot"));
    } else {
        panic!("Expected Episode variant");
    }
}

#[test]
fn test_metadata_extraction_multi_episode() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);
    let multi_file = PathBuf::from(
        "TV Shows/The Office/S01E01-E02 - Pilot & Diversity Day.mkv",
    );
    let parsed_info = parser
        .parse_filename_with_type(&multi_file)
        .expect("should parse");
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        assert_eq!(episode.season, 1);
        assert_eq!(episode.episode, 1);
    } else {
        panic!("Expected Episode variant");
    }
    // Note: end_episode info is in EpisodeInfo but not in ParsedMediaInfo yet
}

#[test]
fn test_metadata_extraction_date_based() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);
    let date_file = PathBuf::from("TV Shows/Daily Show/2024-01-15.mkv");
    let parsed_info = parser
        .parse_filename_with_type(&date_file)
        .expect("should parse");
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        assert_eq!(episode.season, 2024);
        assert_eq!(episode.episode, 115); // Encoded as MMDD
    } else {
        panic!("Expected Episode variant");
    }
}

#[test]
fn test_metadata_extraction_specials() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);
    let special_file = PathBuf::from(
        "TV Shows/Doctor Who/Specials/S00E01 - Christmas Special.mkv",
    );
    let parsed_info = parser
        .parse_filename_with_type(&special_file)
        .expect("should parse");
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        assert_eq!(episode.season, 0);
        assert_eq!(episode.episode, 1);
    } else {
        panic!("Expected Episode variant");
    }
}

#[test]
fn test_metadata_extraction_folder_based() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);
    let folder_file =
        PathBuf::from("TV Shows/The Wire/Season 1/03 - The Buys.mkv");
    let parsed_info = parser
        .parse_filename_with_type(&folder_file)
        .expect("should parse");
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        assert_eq!(episode.show_name, "The Wire");
        assert_eq!(episode.season, 1);
        assert_eq!(episode.episode, 3);
        assert_eq!(episode.episode_title.as_deref(), Some("The Buys"));
    } else {
        panic!("Expected Episode variant");
    }
}

#[test]
fn test_metadata_extraction_movie_in_tv_library() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);
    // Movie file without TV patterns in TV library
    let movie_file =
        PathBuf::from("TV Shows/Documentaries/Planet Earth (2006).mkv");
    let parsed_info = parser
        .parse_filename_with_type(&movie_file)
        .expect("should parse");
    // Should be detected as movie since no TV patterns
    if let ParsedMediaInfo::Movie(_) = parsed_info {
        // Expected Movie variant
    } else {
        panic!("Expected Movie variant");
    }
}

#[test]
fn test_metadata_extraction_tv_in_movie_library() {
    let parser = FilenameParser::with_library_type(LibraryType::Movies);
    // TV file in movie library
    let tv_file = PathBuf::from("Movies/Misplaced/S01E01 - Episode.mkv");
    let parsed_info = parser
        .parse_filename_with_type(&tv_file)
        .expect("should parse");
    // Should still be detected as TV due to strong pattern
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        assert_eq!(episode.season, 1);
        assert_eq!(episode.episode, 1);
    } else {
        panic!("Expected Episode variant");
    }
}

#[test]
fn test_metadata_extraction_anime() {
    let parser = FilenameParser::with_library_type(LibraryType::Series);
    let anime_file = PathBuf::from(
        "TV Shows/Anime/[HorribleSubs] Attack on Titan - 01 [720p].mkv",
    );
    let parsed_info = parser
        .parse_filename_with_type(&anime_file)
        .expect("should parse");
    if let ParsedMediaInfo::Episode(episode) = parsed_info {
        // Should parse the episode number
        assert!(episode.episode > 0);
    } else {
        panic!("Expected Episode variant");
    }
}

#[test]
fn test_metadata_library_context_switching() {
    let mut parser = FilenameParser::new();
    let tv_file = PathBuf::from("Media/Breaking Bad/S01E01.mkv");

    // Without library context
    let info1 = parser
        .parse_filename_with_type(&tv_file)
        .expect("should parse");
    assert!(matches!(info1, ParsedMediaInfo::Episode(_)));

    // Set to movie library
    parser.set_library_type(Some(LibraryType::Movies));
    let info2 = parser
        .parse_filename_with_type(&tv_file)
        .expect("should parse");
    // Still TV due to strong pattern
    assert!(matches!(info2, ParsedMediaInfo::Episode(_)));

    // Set to TV library
    parser.set_library_type(Some(LibraryType::Series));
    let info3 = parser
        .parse_filename_with_type(&tv_file)
        .expect("should parse");
    assert!(matches!(info3, ParsedMediaInfo::Episode(_)));
}

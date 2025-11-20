use ferrex_core::{MetadataExtractor, LibraryType, MediaType};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_file(dir: &TempDir, path: &str) -> PathBuf {
    let file_path = dir.path().join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&file_path, b"fake video content").unwrap();
    file_path
}

#[test]
fn test_metadata_extraction_with_tv_library_context() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    // Standard TV episode
    let tv_file = create_test_file(&temp_dir, "TV Shows/Breaking Bad/Season 1/S01E01 - Pilot.mkv");
    let metadata = extractor.extract_metadata(&tv_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    assert_eq!(parsed.show_name.as_deref(), Some("Breaking Bad"));
    assert_eq!(parsed.season, Some(1));
    assert_eq!(parsed.episode, Some(1));
    assert_eq!(parsed.episode_title.as_deref(), Some("Pilot"));
}

#[test]
fn test_metadata_extraction_multi_episode() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    let multi_file = create_test_file(&temp_dir, "TV Shows/The Office/S01E01-E02 - Pilot & Diversity Day.mkv");
    let metadata = extractor.extract_metadata(&multi_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    assert_eq!(parsed.season, Some(1));
    assert_eq!(parsed.episode, Some(1));
    // Note: end_episode info is in EpisodeInfo but not in ParsedMediaInfo yet
}

#[test]
fn test_metadata_extraction_date_based() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    let date_file = create_test_file(&temp_dir, "TV Shows/Daily Show/2024-01-15.mkv");
    let metadata = extractor.extract_metadata(&date_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    assert_eq!(parsed.season, Some(2024));
    assert_eq!(parsed.episode, Some(115)); // Encoded as MMDD
}

#[test]
fn test_metadata_extraction_specials() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    let special_file = create_test_file(&temp_dir, "TV Shows/Doctor Who/Specials/S00E01 - Christmas Special.mkv");
    let metadata = extractor.extract_metadata(&special_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    assert_eq!(parsed.season, Some(0));
    assert_eq!(parsed.episode, Some(1));
}

#[test]
fn test_metadata_extraction_folder_based() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    let folder_file = create_test_file(&temp_dir, "TV Shows/The Wire/Season 1/03 - The Buys.mkv");
    let metadata = extractor.extract_metadata(&folder_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    assert_eq!(parsed.show_name.as_deref(), Some("The Wire"));
    assert_eq!(parsed.season, Some(1));
    assert_eq!(parsed.episode, Some(3));
    assert_eq!(parsed.episode_title.as_deref(), Some("The Buys"));
}

#[test]
fn test_metadata_extraction_movie_in_tv_library() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    // Movie file without TV patterns in TV library
    let movie_file = create_test_file(&temp_dir, "TV Shows/Documentaries/Planet Earth (2006).mkv");
    let metadata = extractor.extract_metadata(&movie_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    // Should be detected as movie since no TV patterns
    assert_eq!(parsed.media_type, MediaType::Movie);
}

#[test]
fn test_metadata_extraction_tv_in_movie_library() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::Movies);
    
    // TV file in movie library
    let tv_file = create_test_file(&temp_dir, "Movies/Misplaced/S01E01 - Episode.mkv");
    let metadata = extractor.extract_metadata(&tv_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    // Should still be detected as TV due to strong pattern
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    assert_eq!(parsed.season, Some(1));
    assert_eq!(parsed.episode, Some(1));
}

#[test]
fn test_metadata_extraction_anime() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::with_library_type(LibraryType::TvShows);
    
    let anime_file = create_test_file(&temp_dir, "TV Shows/Anime/[HorribleSubs] Attack on Titan - 01 [720p].mkv");
    let metadata = extractor.extract_metadata(&anime_file).unwrap();
    
    assert!(metadata.parsed_info.is_some());
    let parsed = metadata.parsed_info.unwrap();
    assert_eq!(parsed.media_type, MediaType::TvEpisode);
    // Should parse the episode number
    assert!(parsed.episode.is_some());
}

#[test]
fn test_metadata_library_context_switching() {
    let temp_dir = TempDir::new().unwrap();
    let mut extractor = MetadataExtractor::new();
    
    let tv_file = create_test_file(&temp_dir, "Media/Breaking Bad/S01E01.mkv");
    
    // Without library context
    let metadata1 = extractor.extract_metadata(&tv_file).unwrap();
    assert!(metadata1.parsed_info.is_some());
    assert_eq!(metadata1.parsed_info.as_ref().unwrap().media_type, MediaType::TvEpisode);
    
    // Set to movie library
    extractor.set_library_type(Some(LibraryType::Movies));
    let metadata2 = extractor.extract_metadata(&tv_file).unwrap();
    assert!(metadata2.parsed_info.is_some());
    // Still TV due to strong pattern
    assert_eq!(metadata2.parsed_info.as_ref().unwrap().media_type, MediaType::TvEpisode);
    
    // Set to TV library
    extractor.set_library_type(Some(LibraryType::TvShows));
    let metadata3 = extractor.extract_metadata(&tv_file).unwrap();
    assert!(metadata3.parsed_info.is_some());
    assert_eq!(metadata3.parsed_info.as_ref().unwrap().media_type, MediaType::TvEpisode);
}
#[cfg(test)]
mod simple_tests {
    use ferrex_core::{
        api_types::MediaID,
        media::{EpisodeID, MovieID},
        watch_status::UpdateProgressRequest,
    };

    #[test]
    fn test_media_id_creation() {
        // Test creating movie ID
        let movie_id = MovieID::new("550e8400-e29b-41d4-a716-446655440000".to_string()).unwrap();
        let media_id = MediaID::Movie(movie_id);

        match media_id {
            MediaID::Movie(_) => assert!(true),
            _ => assert!(false, "Expected Movie variant"),
        }

        // Test creating episode ID
        let episode_id =
            EpisodeID::new("123e4567-e89b-12d3-a456-426614174000".to_string()).unwrap();
        let media_id = MediaID::Episode(episode_id);

        match media_id {
            MediaID::Episode(_) => assert!(true),
            _ => assert!(false, "Expected Episode variant"),
        }
    }

    #[test]
    fn test_update_progress_request() {
        let movie_id = MovieID::new("550e8400-e29b-41d4-a716-446655440000".to_string()).unwrap();
        let request = UpdateProgressRequest {
            media_id: MediaID::Movie(movie_id),
            position: 1800.0,
            duration: 3600.0,
        };

        assert_eq!(request.position, 1800.0);
        assert_eq!(request.duration, 3600.0);

        // Test progress percentage
        let percentage = (request.position / request.duration) * 100.0;
        assert_eq!(percentage, 50.0);
    }

    #[test]
    fn test_parse_media_id_format() {
        // Test parsing "movie:uuid" format
        let input = "movie:550e8400-e29b-41d4-a716-446655440000";
        let parts: Vec<&str> = input.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "movie");
        assert_eq!(parts[1], "550e8400-e29b-41d4-a716-446655440000");

        // Test parsing "episode:uuid" format
        let input = "episode:123e4567-e89b-12d3-a456-426614174000";
        let parts: Vec<&str> = input.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "episode");
        assert_eq!(parts[1], "123e4567-e89b-12d3-a456-426614174000");
    }
}

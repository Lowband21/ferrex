#[cfg(test)]
mod watch_status_tests {
    use ferrex_core::api_types::MediaID;
    use ferrex_core::media::{EpisodeID, MovieID};
    use ferrex_core::watch_status::*;
    use std::collections::HashSet;

    fn create_test_media_id(id: &str) -> MediaID {
        MediaID::Movie(ferrex_core::media::MovieID::new(id.to_string()).unwrap())
    }

    #[test]
    fn test_watch_progress_creation() {
        let progress = WatchProgress::new(0.5);
        assert_eq!(progress.as_percentage(), 0.5);

        // Test clamping
        let over_progress = WatchProgress::new(1.5);
        assert_eq!(over_progress.as_percentage(), 1.0);

        let under_progress = WatchProgress::new(-0.5);
        assert_eq!(under_progress.as_percentage(), 0.0);
    }

    #[test]
    fn test_watch_progress_completion() {
        let not_started = WatchProgress::new(0.0);
        assert!(!not_started.is_started());
        assert!(!not_started.is_completed());

        let in_progress = WatchProgress::new(0.5);
        assert!(in_progress.is_started());
        assert!(!in_progress.is_completed());

        let almost_done = WatchProgress::new(0.94);
        assert!(almost_done.is_started());
        assert!(!almost_done.is_completed());

        let completed = WatchProgress::new(0.96);
        assert!(completed.is_started());
        assert!(completed.is_completed());
    }

    #[test]
    fn test_user_watch_state_initialization() {
        let state = UserWatchState::new();
        assert!(state.in_progress.is_empty());
        assert!(state.completed.is_empty());
    }

    #[test]
    fn test_update_progress_new_item() {
        let mut state = UserWatchState::new();
        let media_id = create_test_media_id("movie123");

        // Start watching
        state.update_progress(media_id.clone(), 300.0, 7200.0); // 5 min into 2 hour movie

        assert_eq!(state.in_progress.len(), 1);
        assert_eq!(state.in_progress[0].media_id, media_id);
        assert_eq!(state.in_progress[0].position, 300.0);
        assert_eq!(state.in_progress[0].duration, 7200.0);
        assert!(state.in_progress[0].last_watched > 0);
    }

    #[test]
    fn test_update_progress_existing_item() {
        let mut state = UserWatchState::new();
        let media_id = create_test_media_id("movie123");

        // Initial progress
        state.update_progress(media_id.clone(), 300.0, 7200.0);
        let initial_timestamp = state.in_progress[0].last_watched;

        // Sleep briefly to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Update progress
        state.update_progress(media_id.clone(), 600.0, 7200.0);

        assert_eq!(state.in_progress.len(), 1);
        assert_eq!(state.in_progress[0].position, 600.0);
        assert!(state.in_progress[0].last_watched > initial_timestamp);
    }

    #[test]
    fn test_progress_to_completion() {
        let mut state = UserWatchState::new();
        let media_id = create_test_media_id("movie123");

        // Start watching
        state.update_progress(media_id.clone(), 300.0, 7200.0);
        assert_eq!(state.in_progress.len(), 1);
        assert!(!state.completed.contains(&media_id));

        // Complete watching (>95%)
        state.update_progress(media_id.clone(), 6900.0, 7200.0); // 96% complete

        assert_eq!(state.in_progress.len(), 0);
        assert!(state.completed.contains(&media_id));
    }

    #[test]
    fn test_continue_watching_order() {
        let mut state = UserWatchState::new();

        // Add multiple items with delays
        for i in 0..5 {
            let media_id = create_test_media_id(&format!("movie{}", i));
            state.update_progress(media_id, 300.0, 7200.0);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let continue_watching = state.get_continue_watching(3);
        assert_eq!(continue_watching.len(), 3);

        // Most recent should be first
        assert_eq!(
            continue_watching[0].media_id,
            create_test_media_id("movie4")
        );
        assert_eq!(
            continue_watching[1].media_id,
            create_test_media_id("movie3")
        );
        assert_eq!(
            continue_watching[2].media_id,
            create_test_media_id("movie2")
        );
    }

    #[test]
    fn test_in_progress_limit() {
        let mut state = UserWatchState::new();

        // Add 60 items
        for i in 0..60 {
            let media_id = create_test_media_id(&format!("movie{}", i));
            state.update_progress(media_id, 300.0, 7200.0);
        }

        // Should be limited to 50
        assert_eq!(state.in_progress.len(), 50);

        // Most recent 50 should be kept
        assert_eq!(
            state.in_progress[0].media_id,
            create_test_media_id("movie59")
        );
        assert_eq!(
            state.in_progress[49].media_id,
            create_test_media_id("movie10")
        );
    }

    #[test]
    fn test_clear_progress() {
        let mut state = UserWatchState::new();
        let media_id = create_test_media_id("movie123");

        // Add to in progress
        state.update_progress(media_id.clone(), 300.0, 7200.0);
        assert!(state.get_progress(&media_id).is_some());

        // Clear progress
        state.clear_progress(&media_id);
        assert!(state.get_progress(&media_id).is_none());
        assert!(!state
            .in_progress
            .iter()
            .any(|item| item.media_id == media_id));

        // Add to completed
        state.update_progress(media_id.clone(), 7000.0, 7200.0);
        assert!(state.is_completed(&media_id));

        // Clear should remove from completed too
        state.clear_progress(&media_id);
        assert!(!state.is_completed(&media_id));
    }

    #[test]
    fn test_get_progress_calculation() {
        let mut state = UserWatchState::new();
        let media_id = create_test_media_id("movie123");

        state.update_progress(media_id.clone(), 1800.0, 3600.0); // 30 min of 1 hour

        let progress = state.get_progress(&media_id).unwrap();
        assert_eq!(progress.as_percentage(), 0.5); // 50%
    }

    #[test]
    fn test_no_progress_for_zero_position() {
        let mut state = UserWatchState::new();
        let media_id = create_test_media_id("movie123");

        // Zero position should not create progress entry
        state.update_progress(media_id.clone(), 0.0, 7200.0);
        assert_eq!(state.in_progress.len(), 0);
        assert!(state.get_progress(&media_id).is_none());
    }

    #[test]
    fn test_update_progress_request() {
        let request = UpdateProgressRequest {
            media_id: create_test_media_id("movie123"),
            position: 1800.0,
            duration: 3600.0,
        };

        assert_eq!(request.position, 1800.0);
        assert_eq!(request.duration, 3600.0);
    }

    #[test]
    fn test_multiple_media_tracking() {
        let mut state = UserWatchState::new();

        let movie1 = create_test_media_id("movie1");
        let movie2 = create_test_media_id("movie2");
        let show1 =
            MediaID::Episode(ferrex_core::media::EpisodeID::new("show1".to_string()).unwrap());

        // Track multiple items
        state.update_progress(movie1.clone(), 1000.0, 7200.0);
        state.update_progress(movie2.clone(), 500.0, 5400.0);
        state.update_progress(show1.clone(), 1200.0, 2400.0);

        assert_eq!(state.in_progress.len(), 3);

        // Complete one
        state.update_progress(show1.clone(), 2300.0, 2400.0);

        assert_eq!(state.in_progress.len(), 2);
        assert!(state.is_completed(&show1));
        assert!(!state.is_completed(&movie1));
        assert!(!state.is_completed(&movie2));
    }
}

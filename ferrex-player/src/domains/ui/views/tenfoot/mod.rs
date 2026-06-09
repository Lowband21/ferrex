pub mod detail;
pub mod home;

#[cfg(test)]
mod detail_tests {
    use super::detail::{
        TenFootWatchInfo, bounded_two_row_window_start,
        primary_label_for_watch_info, start_over_available_for_watch_info,
        visible_panel_columns_for_width,
    };

    #[test]
    fn detail_two_row_window_tracks_focus_without_showing_entire_graph() {
        assert_eq!(bounded_two_row_window_start(0, Some(0), 20, 4), 0);
        assert_eq!(bounded_two_row_window_start(0, Some(7), 20, 4), 0);
        assert_eq!(bounded_two_row_window_start(0, Some(8), 20, 4), 4);
        assert_eq!(bounded_two_row_window_start(4, Some(3), 20, 4), 0);
        assert_eq!(bounded_two_row_window_start(100, Some(19), 20, 4), 12);
    }

    #[test]
    fn detail_visible_columns_scale_with_tv_widths() {
        assert_eq!(visible_panel_columns_for_width(1280.0), 3);
        assert!(visible_panel_columns_for_width(1920.0) >= 5);
        assert!(visible_panel_columns_for_width(3840.0) >= 10);
    }

    #[test]
    fn detail_primary_label_resumes_only_for_in_progress_media() {
        assert_eq!(
            primary_label_for_watch_info(TenFootWatchInfo {
                has_watch_state: true,
                in_progress: true,
                position: 10.0,
                duration: 100.0,
            }),
            "Resume"
        );
        assert_eq!(
            primary_label_for_watch_info(TenFootWatchInfo {
                has_watch_state: true,
                in_progress: false,
                position: 0.0,
                duration: 0.0,
            }),
            "Play"
        );
        assert_eq!(
            primary_label_for_watch_info(TenFootWatchInfo::default()),
            "Play"
        );
    }

    #[test]
    fn detail_start_over_requires_existing_watch_state() {
        assert!(start_over_available_for_watch_info(TenFootWatchInfo {
            has_watch_state: true,
            in_progress: false,
            position: 0.0,
            duration: 0.0,
        }));
        assert!(!start_over_available_for_watch_info(
            TenFootWatchInfo::default()
        ));
    }
}

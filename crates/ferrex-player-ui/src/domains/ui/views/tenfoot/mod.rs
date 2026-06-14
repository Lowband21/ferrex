pub mod detail;
pub mod home;
pub mod player_overlay;

#[cfg(test)]
mod detail_tests {
    use super::detail::{
        TenFootWatchInfo, bounded_panel_window_start,
        bounded_two_row_window_start, primary_label_for_watch_info,
        start_over_available_for_watch_info, visible_panel_columns_for_width,
    };
    use crate::{
        domains::ui::views::detail::{
            DetailInterfaceMode, DetailLayoutPlan,
            solve_detail_layout_from_runtime,
        },
        infra::{
            constants::layout::{calculations::ScaledLayout, grid},
            design_tokens::{ScalingContext, SizeProvider},
        },
    };

    fn layout_plan(width: f32, height: f32, scale: f32) -> DetailLayoutPlan {
        let sizes =
            SizeProvider::new(ScalingContext::new().with_user_scale(scale));
        let layout = ScaledLayout::new(sizes.scale, grid::EFFECTIVE_SPACING);
        solve_detail_layout_from_runtime(
            width,
            height,
            0.0,
            DetailInterfaceMode::TenFoot,
            &sizes,
            &layout,
        )
    }

    #[test]
    fn detail_panel_window_tracks_focus_for_solved_row_counts() {
        assert_eq!(bounded_two_row_window_start(0, Some(0), 20, 4), 0);
        assert_eq!(bounded_two_row_window_start(0, Some(7), 20, 4), 0);
        assert_eq!(bounded_two_row_window_start(0, Some(8), 20, 4), 4);
        assert_eq!(bounded_two_row_window_start(4, Some(3), 20, 4), 0);
        assert_eq!(bounded_two_row_window_start(100, Some(19), 20, 4), 12);

        assert_eq!(bounded_panel_window_start(0, Some(3), 20, 3, 1), 3);
        assert_eq!(bounded_panel_window_start(3, Some(2), 20, 3, 1), 0);
        assert_eq!(bounded_panel_window_start(99, Some(19), 20, 3, 1), 17);
    }

    #[test]
    fn detail_visible_columns_use_capped_tv_stage_widths() {
        let hd = layout_plan(1_280.0, 800.0, 1.0);
        let full_hd = layout_plan(1_920.0, 1_080.0, 1.0);
        let ultrawide = layout_plan(3_840.0, 1_600.0, 1.0);

        assert_eq!(visible_panel_columns_for_width(1_280.0, &hd), 3);
        assert_eq!(visible_panel_columns_for_width(1_920.0, &full_hd), 5);
        assert_eq!(
            visible_panel_columns_for_width(3_840.0, &ultrawide),
            visible_panel_columns_for_width(1_920.0, &full_hd)
        );
        assert!(
            ultrawide.content_width
                < ultrawide.viewport_width - ultrawide.page_padding_x * 2.0
        );
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

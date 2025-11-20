#[cfg(test)]
mod ui_widgets_tests {
    use ferrex_core::{SortBy, SortOrder};
    use ferrex_player::domains::ui::messages::Message;
    use ferrex_player::domains::ui::widgets::{filter_button, sort_dropdown, sort_order_toggle};
    use iced::Element;

    #[test]
    fn test_sort_dropdown_creates_element() {
        // Test that sort_dropdown creates a valid element for each SortBy variant
        let sort_options = vec![
            SortBy::DateAdded,
            SortBy::Title,
            SortBy::ReleaseDate,
            SortBy::Rating,
        ];

        for sort_by in sort_options {
            let element: Element<Message> = sort_dropdown(sort_by);
            // If this compiles and runs without panic, the element was created successfully
            drop(element);
        }
    }

    #[test]
    fn test_sort_order_toggle_creates_element() {
        // Test both sort order variants
        let element_asc: Element<Message> = sort_order_toggle(SortOrder::Ascending);
        drop(element_asc);

        let element_desc: Element<Message> = sort_order_toggle(SortOrder::Descending);
        drop(element_desc);
    }

    #[test]
    fn test_filter_button_states() {
        // Test filter button with no active filters
        let element_no_filters: Element<Message> = filter_button(0, false);
        drop(element_no_filters);

        // Test filter button with active filters
        let element_with_filters: Element<Message> = filter_button(3, false);
        drop(element_with_filters);

        // Test filter button when open
        let element_open: Element<Message> = filter_button(0, true);
        drop(element_open);
    }

    #[test]
    fn test_sort_option_display() {
        use ferrex_player::domains::ui::widgets::sort_dropdown::SortOption;

        // Test that SortOption displays correctly
        assert_eq!(SortOption::OPTIONS[0].label, "Date Added");
        assert_eq!(SortOption::OPTIONS[1].label, "File Created");
        assert_eq!(SortOption::OPTIONS[2].label, "Title");
        assert_eq!(SortOption::OPTIONS[3].label, "Release Year");

        // Test that display formatting works
        assert_eq!(format!("{}", SortOption::OPTIONS[0]), "Date Added");
    }
}

//! Search domain update logic (global, server-backed)

use iced::Task;
use iced::widget::operation::snap_to;
use std::time::Instant;

use super::messages::Message;
use super::types::{SEARCH_RESULTS_SCROLL_ID, SearchMode, SearchStrategy};
use crate::common::messages::{
    CrossDomainEvent, DomainMessage, DomainUpdateResult,
};
use crate::domains::ui::{messages as ui_messages, windows::focus};
use crate::infra::constants::layout::search as search_layout;
use crate::state::State;
use iced::widget::scrollable::RelativeOffset;

pub fn update(state: &mut State, message: Message) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("search_update");

    match message {
        Message::UpdateQuery(query) => {
            state.domains.search.state.escape_pending = false;
            state.domains.search.state.query = query.clone();
            state.domains.search.state.selected_index = None;

            if query.is_empty() {
                state.domains.search.state.clear();
                DomainUpdateResult::task(Task::none())
            } else {
                // Keep focus on search input and debounce the search
                DomainUpdateResult::task(Task::batch(vec![
                    focus::focus_active_search_input(state)
                        .map(|_| DomainMessage::NoOp),
                    Task::perform(
                        async move {
                            tokio::time::sleep(
                                tokio::time::Duration::from_millis(200),
                            )
                            .await;
                            query
                        },
                        |query| {
                            DomainMessage::Search(Message::SearchDebounced(
                                query,
                            ))
                        },
                    ),
                ]))
            }
        }

        Message::SearchDebounced(query) => {
            // Check if this is still the current query (user might have typed more)
            if state.domains.search.state.query == query {
                // Instant search - stay in dropdown mode
                handle_execute_search(state, false)
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::ExecuteSearch => {
            // Explicit search - switch to full screen
            handle_execute_search(state, true)
        }

        Message::ClearSearch => {
            state.domains.search.state.clear();
            DomainUpdateResult::task(Task::none())
        }

        Message::SelectResult(media_ref) => {
            // Emit cross-domain event to navigate to the selected media
            let event = CrossDomainEvent::NavigateToMedia(media_ref.clone());

            // Clear search after selection
            state.domains.search.state.clear();

            let navigation_task =
                Task::perform(async move { event }, |event| {
                    DomainMessage::Event(event)
                });
            DomainUpdateResult::task(Task::batch([
                navigation_task,
                Task::done(DomainMessage::Ui(
                    ui_messages::Message::CloseSearchWindow,
                )),
            ]))
        }

        Message::LoadMore => {
            // Increase displayed results count
            let current = state.domains.search.state.displayed_results;
            let page_size = state.domains.search.state.page_size;
            state.domains.search.state.displayed_results = current + page_size;
            DomainUpdateResult::task(Task::none())
        }

        Message::ToggleMode => {
            let new_mode = match state.domains.search.state.mode {
                SearchMode::Dropdown => SearchMode::FullScreen,
                SearchMode::FullScreen => SearchMode::Dropdown,
            };
            state.domains.search.state.set_mode(new_mode);
            DomainUpdateResult::task(Task::none())
        }

        Message::SetMode(mode) => {
            state.domains.search.state.set_mode(mode);
            DomainUpdateResult::task(Task::none())
        }

        Message::SelectPrevious => {
            if state.domains.search.state.escape_pending {
                state.domains.search.state.select_previous();
                let scroll_task = scroll_selected_into_view(state);
                return DomainUpdateResult::task(scroll_task);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SelectNext => {
            if state.domains.search.state.escape_pending {
                state.domains.search.state.select_next();
                let scroll_task = scroll_selected_into_view(state);
                return DomainUpdateResult::task(scroll_task);
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::SelectCurrent => {
            let selected_media = {
                let search_state = &mut state.domains.search.state;

                let result = if let Some(selected) =
                    search_state.get_selected().cloned()
                {
                    Some(selected)
                } else if let Some(first) =
                    search_state.results.first().cloned()
                {
                    search_state.selected_index = Some(0);
                    Some(first)
                } else {
                    None
                };

                search_state.escape_pending = false;

                result.map(|res| res.media_ref)
            };

            if let Some(media_ref) = selected_media {
                let mut result = handle_select_result(state, media_ref);
                result.task = Task::batch([
                    result.task,
                    Task::done(DomainMessage::Ui(
                        ui_messages::Message::CloseSearchWindow,
                    )),
                ]);
                result
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::HandleEscape => {
            if state.domains.search.state.escape_pending {
                state.domains.search.state.escape_pending = false;
                DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                    ui_messages::Message::CloseSearchWindow,
                )))
            } else {
                {
                    let search_state = &mut state.domains.search.state;
                    if search_state.selected_index.is_none()
                        && !search_state.results.is_empty()
                    {
                        search_state.selected_index = Some(0);
                    }
                    search_state.escape_pending = true;
                }
                DomainUpdateResult::task(scroll_selected_into_view(state))
            }
        }

        Message::ResultsReceived {
            query,
            results,
            total_count,
        } => {
            // Only update if this matches current query
            if state.domains.search.state.query == query {
                state.domains.search.state.results = results;
                state.domains.search.state.total_results = total_count;
                state.domains.search.state.displayed_results =
                    total_count.min(state.domains.search.state.page_size);
                state.domains.search.state.is_searching = false;
                state.domains.search.state.error = None;
                state.domains.search.state.escape_pending = false;
                state.domains.search.state.window_scroll_offset = 0.0;
                if state.search_window_id.is_some() {
                    state.domains.search.state.selected_index = None;
                }

                // Record metrics if available
                if let Some(metric) =
                    state.domains.search.state.last_metric.take()
                {
                    state
                        .domains
                        .search
                        .state
                        .decision_engine
                        .record_execution(metric);
                }

                // Keep focus on search input when results arrive
                DomainUpdateResult::task(Task::batch(vec![
                    focus::focus_active_search_input(state)
                        .map(|_| DomainMessage::NoOp),
                ]))
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::SearchError(error) => {
            state.domains.search.state.is_searching = false;
            state.domains.search.state.error = Some(error);
            DomainUpdateResult::task(Task::none())
        }

        Message::SetSearching(searching) => {
            state.domains.search.state.is_searching = searching;
            DomainUpdateResult::task(Task::none())
        }

        Message::RecordMetrics(metric) => {
            // Record performance metrics in the decision engine
            state
                .domains
                .search
                .state
                .decision_engine
                .record_execution(metric.clone());

            // Record network latency if it was a server search
            if metric.strategy == SearchStrategy::Server {
                if metric.success {
                    state
                        .domains
                        .search
                        .state
                        .decision_engine
                        .record_network_success(metric.execution_time);
                } else {
                    state
                        .domains
                        .search
                        .state
                        .decision_engine
                        .record_network_failure();
                }
            }

            DomainUpdateResult::task(Task::none())
        }

        Message::RequestMediaDetails(media_ref) => {
            // Request details from media domain
            let event = CrossDomainEvent::RequestMediaDetails(media_ref);
            DomainUpdateResult::task(Task::perform(
                async move { event },
                DomainMessage::Event,
            ))
        }

        Message::RefreshFromMediaStore => {
            // Media changed; re-run search if we have a query
            if !state.domains.search.state.query.is_empty() {
                handle_execute_search(state, false)
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }

        Message::_CalibrationComplete(results) => {
            // Store calibration results in the decision engine
            log::info!(
                "Search calibration complete - optimal strategy: {:?}",
                results.optimal_strategy
            );
            state
                .domains
                .search
                .state
                .decision_engine
                .set_calibration(results);
            DomainUpdateResult::task(Task::none())
        }

        Message::RunCalibration => {
            // Run calibration to determine optimal search strategy
            log::info!("Starting search calibration...");
            let service = state.domains.search.service.clone();

            DomainUpdateResult::task(Task::perform(
                async move {
                    super::calibrator::SearchCalibrator::calibrate(&service)
                        .await
                },
                |results| {
                    DomainMessage::Search(Message::_CalibrationComplete(
                        results,
                    ))
                },
            ))
        }
    }
}

fn handle_execute_search(
    state: &mut State,
    switch_to_fullscreen: bool,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::function_scope!("execute search");

    let query = state.domains.search.state.query.clone();

    if query.is_empty() {
        return DomainUpdateResult::task(Task::none());
    }

    // Only switch to full-screen mode if this is an explicit search (Enter/button press)
    if switch_to_fullscreen {
        state.domains.search.state.mode =
            crate::domains::search::types::SearchMode::FullScreen;
    }

    // Check cache first
    if let Some(cached) = state.domains.search.state.get_cached_results(&query)
    {
        let results = cached.results.clone();
        let total_count = cached.total_count;

        return DomainUpdateResult::task(Task::perform(
            async move { (query, results, total_count) },
            |(query, results, total_count)| {
                DomainMessage::Search(Message::ResultsReceived {
                    query,
                    results,
                    total_count,
                })
            },
        ));
    }

    // Strategy selection is reserved for future use; we currently use server search for best coverage
    let strategy = SearchStrategy::Server;

    state.domains.search.state.current_strategy = Some(strategy);
    state.domains.search.state.is_searching = true;
    state.domains.search.state.last_search_time = Some(Instant::now());

    // Execute search with metrics
    let service = state.domains.search.service.clone();
    let fields = state.domains.search.state.search_fields.clone();
    // Always search globally; ignore any library filter
    let library_id = None;
    let fuzzy = state.domains.search.state.fuzzy_matching;

    DomainUpdateResult::task(Task::perform(
        async move {
            match service
                .search(&query, &fields, strategy, library_id, fuzzy)
                .await
            {
                Ok(results) => {
                    let total_count = results.len();
                    (query, Ok((results, total_count)))
                }
                Err(error) => (query, Err(error)),
            }
        },
        move |(query, result)| match result {
            Ok((results, total_count)) => {
                DomainMessage::Search(Message::ResultsReceived {
                    query,
                    results,
                    total_count,
                })
            }
            Err(error) => DomainMessage::Search(Message::SearchError(error)),
        },
    ))
}

fn handle_select_result(
    state: &mut State,
    media_ref: crate::infra::api_types::Media,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("select search result");

    // Clear search after selection
    state.domains.search.state.clear();

    // Use cross-domain event for navigation
    let event = CrossDomainEvent::NavigateToMedia(media_ref);

    DomainUpdateResult::task(Task::perform(async move { event }, |event| {
        DomainMessage::Event(event)
    }))
}

fn scroll_selected_into_view(state: &mut State) -> Task<DomainMessage> {
    if state.search_window_id.is_none() {
        return Task::none();
    }

    let search_state = &mut state.domains.search.state;
    let Some(selected_index) = search_state.selected_index else {
        return Task::none();
    };

    let total = search_state.results.len();
    if total == 0 {
        search_state.window_scroll_offset = 0.0;
        return Task::none();
    }

    let viewport_height = search_layout::RESULTS_VIEWPORT_HEIGHT;
    let row_height = search_layout::RESULT_ROW_HEIGHT;
    let row_spacing = search_layout::RESULT_ROW_SPACING;
    let half_step = search_layout::RESULTS_HALF_STEP;

    if viewport_height <= 0.0 || half_step <= 0.0 {
        return Task::none();
    }

    let row_pitch = row_height + row_spacing;
    let mut content_height = row_height * (total as f32)
        + row_spacing * (total.saturating_sub(1) as f32);

    if search_state.total_results > 0 {
        content_height += row_spacing + search_layout::RESULTS_FOOTER_HEIGHT;
    }

    if content_height <= viewport_height {
        search_state.window_scroll_offset = 0.0;
        return Task::none();
    }

    let max_offset = (content_height - viewport_height).max(0.0);
    let mut offset = search_state.window_scroll_offset.clamp(0.0, max_offset);

    let row_top = (selected_index as f32) * row_pitch;
    let row_bottom = row_top + row_height;

    let viewport_top = offset;
    let viewport_bottom = offset + viewport_height;

    if row_top < viewport_top {
        let needed = viewport_top - row_top;
        let steps = (needed / half_step).ceil().max(1.0);
        offset = (offset - steps * half_step).max(0.0);
    } else if row_bottom > viewport_bottom {
        let needed = row_bottom - viewport_bottom;
        let steps = (needed / half_step).ceil().max(1.0);
        offset = (offset + steps * half_step).min(max_offset);
    }

    if half_step > 0.0 {
        offset = (offset / half_step).round() * half_step;
    }
    offset = offset.clamp(0.0, max_offset);

    if (offset - search_state.window_scroll_offset).abs() <= f32::EPSILON {
        return Task::none();
    }

    search_state.window_scroll_offset = offset;

    if max_offset <= f32::EPSILON {
        return Task::none();
    }

    let y = (offset / max_offset).clamp(0.0, 1.0);

    snap_to(SEARCH_RESULTS_SCROLL_ID, RelativeOffset { x: 0.0, y })
}

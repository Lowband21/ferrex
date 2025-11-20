//! Search domain update logic

use iced::Task;
use std::time::Instant;

use super::messages::Message;
use super::types::{SearchDecisionEngine, SearchMode, SearchStrategy};
use crate::common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult};
use crate::state_refactored::State;

pub fn update(state: &mut State, message: Message) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("search_update");

    match message {
        Message::UpdateQuery(query) => {
            state.domains.search.state.query = query.clone();
            state.domains.search.state.selected_index = None;

            if query.is_empty() {
                state.domains.search.state.clear();
                DomainUpdateResult::task(Task::none())
            } else {
                // Keep focus on search input and debounce the search
                use iced::widget::text_input;
                DomainUpdateResult::task(Task::batch(vec![
                    text_input::focus::<DomainMessage>(text_input::Id::new("search-input"))
                        .map(|_| DomainMessage::NoOp),
                    Task::perform(
                        async move {
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                            query
                        },
                        |query| DomainMessage::Search(Message::SearchDebounced(query)),
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

            DomainUpdateResult::task(Task::perform(async move { event }, |event| {
                DomainMessage::Event(event)
            }))
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
            state.domains.search.state.select_previous();
            DomainUpdateResult::task(Task::none())
        }

        Message::SelectNext => {
            state.domains.search.state.select_next();
            DomainUpdateResult::task(Task::none())
        }

        Message::SelectCurrent => {
            if let Some(result) = state.domains.search.state.get_selected().cloned() {
                handle_select_result(state, result.media_ref)
            } else {
                DomainUpdateResult::task(Task::none())
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

                // Record metrics if available
                if let Some(metric) = state.domains.search.state.last_metric.take() {
                    state
                        .domains
                        .search
                        .state
                        .decision_engine
                        .record_execution(metric);
                }

                // Keep focus on search input when results arrive
                use iced::widget::text_input;
                DomainUpdateResult::task(Task::batch(vec![text_input::focus::<DomainMessage>(
                    text_input::Id::new("search-input"),
                )
                .map(|_| DomainMessage::NoOp)]))
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
            DomainUpdateResult::task(Task::perform(async move { event }, |event| {
                DomainMessage::Event(event)
            }))
        }

        Message::RefreshFromMediaStore => {
            // Re-run search if we have a query
            if !state.domains.search.state.query.is_empty() {
                // MediaStore refresh - keep current mode
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
                async move { super::calibrator::SearchCalibrator::calibrate(&service).await },
                |results| DomainMessage::Search(Message::_CalibrationComplete(results)),
            ))
        }
    }
}

fn handle_execute_search(state: &mut State, switch_to_fullscreen: bool) -> DomainUpdateResult {
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
        state.domains.search.state.mode = crate::domains::search::types::SearchMode::FullScreen;
    }

    // Check cache first
    if let Some(cached) = state.domains.search.state.get_cached_results(&query) {
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

    // Determine search strategy using the enhanced decision engine
    let data_completeness = calculate_data_completeness(state);
    let network_available = state.domains.search.service.has_network();

    let strategy = state
        .domains
        .search
        .state
        .decision_engine
        .determine_strategy_enhanced(
            &query,
            data_completeness,
            &state.domains.search.state.search_fields,
            network_available,
        );

    log::info!(
        "Search strategy selected: {:?} (query: '{}', completeness: {:.1}%, network: {})",
        strategy,
        query,
        data_completeness * 100.0,
        network_available
    );

    state.domains.search.state.current_strategy = Some(strategy);
    state.domains.search.state.is_searching = true;
    state.domains.search.state.last_search_time = Some(Instant::now());

    // Execute search with metrics
    let service = state.domains.search.service.clone();
    let fields = state.domains.search.state.search_fields.clone();
    let library_id = state.domains.search.state.library_id;
    let fuzzy = state.domains.search.state.fuzzy_matching;
    let search_state = state.domains.search.state.clone();

    DomainUpdateResult::task(Task::perform(
        async move {
            // TODO: Switch to search_with_metrics when we implement proper message batching
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
            Ok((results, total_count)) => DomainMessage::Search(Message::ResultsReceived {
                query,
                results,
                total_count,
            }),
            Err(error) => DomainMessage::Search(Message::SearchError(error)),
        },
    ))
}

fn handle_select_result(
    state: &mut State,
    media_ref: crate::infrastructure::api_types::MediaReference,
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

/// Calculate how complete our local data is
fn calculate_data_completeness(state: &State) -> f32 {
    // Simple heuristic: check what percentage of media has TMDB details
    let store = match state.domains.media.state.media_store.read() {
        Ok(store) => store,
        Err(_) => return 0.0,
    };

    let total_media = store.len();
    if total_media == 0 {
        return 0.0;
    }

    let with_details = store
        .get_all_movies()
        .into_iter()
        .filter(|m| m.tmdb_id > 0)
        .count()
        + store
            .get_all_series()
            .into_iter()
            .filter(|s| s.tmdb_id > 0)
            .count();

    with_details as f32 / total_media as f32
}

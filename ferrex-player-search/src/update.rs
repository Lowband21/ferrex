//! Context-based search update logic.

use crate::{
    calibrator::SearchCalibrator,
    messages::SearchMessage,
    types::{SearchMode, SearchStrategy},
};
use ferrex_player_foundation::domain::{DomainTask, DomainUpdateResult};
use std::sync::Arc;
use std::time::Instant;

/// App-shell hooks required by the extracted search updater.
pub trait SearchUpdateContext {
    type AppMessage: Send + 'static;

    fn search_state(&self) -> &crate::SearchState;
    fn search_state_mut(&mut self) -> &mut crate::SearchState;
    fn search_service(&self) -> Arc<crate::SearchService>;
    fn search_message(message: SearchMessage) -> Self::AppMessage;
    fn close_search_message(&self) -> Option<Self::AppMessage> {
        None
    }
    fn navigate_to_media(
        &self,
        _media: ferrex_player_api::api_types::Media,
    ) -> Option<Self::AppMessage> {
        None
    }
    fn request_media_details(
        &self,
        _media: ferrex_player_api::api_types::Media,
    ) -> Option<Self::AppMessage> {
        None
    }
}

pub fn update<C>(
    context: &mut C,
    message: SearchMessage,
) -> DomainUpdateResult<DomainTask<C::AppMessage>, ()>
where
    C: SearchUpdateContext + 'static,
{
    match message {
        SearchMessage::UpdateQuery(query) => {
            let state = context.search_state_mut();
            state.escape_pending = false;
            state.query = query.clone();
            state.selected_index = None;

            if query.is_empty() {
                state.clear();
                DomainUpdateResult::task(DomainTask::none())
            } else {
                DomainUpdateResult::task(DomainTask::perform(
                    async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            200,
                        ))
                        .await;
                        query
                    },
                    |query| {
                        C::search_message(SearchMessage::SearchDebounced(query))
                    },
                ))
            }
        }
        SearchMessage::SearchDebounced(query) => {
            if context.search_state().query == query {
                execute_search(context, false)
            } else {
                DomainUpdateResult::task(DomainTask::none())
            }
        }
        SearchMessage::ExecuteSearch => execute_search(context, true),
        SearchMessage::ClearSearch => {
            context.search_state_mut().clear();
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::SelectResult(media_ref) => {
            context.search_state_mut().clear();
            let task = match (
                context.navigate_to_media(media_ref),
                context.close_search_message(),
            ) {
                (Some(nav), Some(close)) => DomainTask::batch(vec![
                    DomainTask::done(nav),
                    DomainTask::done(close),
                ]),
                (Some(nav), None) => DomainTask::done(nav),
                (None, Some(close)) => DomainTask::done(close),
                (None, None) => DomainTask::none(),
            };
            DomainUpdateResult::task(task)
        }
        SearchMessage::LoadMore => {
            let state = context.search_state_mut();
            state.displayed_results += state.page_size;
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::ToggleMode => {
            let new_mode = match context.search_state().mode {
                SearchMode::Dropdown => SearchMode::FullScreen,
                SearchMode::FullScreen => SearchMode::Dropdown,
            };
            context.search_state_mut().set_mode(new_mode);
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::SetMode(mode) => {
            context.search_state_mut().set_mode(mode);
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::ResultsReceived {
            query,
            results,
            total_count,
        } => {
            let state = context.search_state_mut();
            if state.query == query {
                state.results = results;
                state.total_results = total_count;
                state.displayed_results = total_count.min(state.page_size);
                state.is_searching = false;
                state.error = None;
                state.escape_pending = false;
                state.window_scroll_offset = 0.0;
                if let Some(metric) = state.last_metric.take() {
                    state.decision_engine.record_execution(metric);
                }
            }
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::SearchError(error) => {
            let state = context.search_state_mut();
            state.is_searching = false;
            state.error = Some(error);
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::SetSearching(searching) => {
            context.search_state_mut().is_searching = searching;
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::RecordMetrics(metric) => {
            let state = context.search_state_mut();
            state.decision_engine.record_execution(metric.clone());
            if metric.strategy == SearchStrategy::Server {
                if metric.success {
                    state
                        .decision_engine
                        .record_network_success(metric.execution_time);
                } else {
                    state.decision_engine.record_network_failure();
                }
            }
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::RefreshFromMediaStore => {
            if !context.search_state().query.is_empty() {
                execute_search(context, false)
            } else {
                DomainUpdateResult::task(DomainTask::none())
            }
        }
        SearchMessage::RequestMediaDetails(media_ref) => {
            DomainUpdateResult::task(
                context
                    .request_media_details(media_ref)
                    .map(DomainTask::done)
                    .unwrap_or_else(DomainTask::none),
            )
        }
        SearchMessage::_CalibrationComplete(results) => {
            context
                .search_state_mut()
                .decision_engine
                .set_calibration(results);
            DomainUpdateResult::task(DomainTask::none())
        }
        SearchMessage::RunCalibration => {
            let service = context.search_service();
            DomainUpdateResult::task(DomainTask::perform(
                async move { SearchCalibrator::calibrate(&service).await },
                |results| {
                    C::search_message(SearchMessage::_CalibrationComplete(
                        results,
                    ))
                },
            ))
        }
        SearchMessage::SelectPrevious
        | SearchMessage::SelectNext
        | SearchMessage::SelectCurrent
        | SearchMessage::HandleEscape
        | SearchMessage::TenFootKeyboardMove(_)
        | SearchMessage::TenFootKeyboardActivate
        | SearchMessage::TenFootKeyboardPress(_)
        | SearchMessage::ShowTenFootKeyboard
        | SearchMessage::HideTenFootKeyboard => {
            DomainUpdateResult::task(DomainTask::none())
        }
    }
}

fn execute_search<C>(
    context: &mut C,
    switch_to_fullscreen: bool,
) -> DomainUpdateResult<DomainTask<C::AppMessage>, ()>
where
    C: SearchUpdateContext + 'static,
{
    let query = context.search_state().query.clone();
    if query.is_empty() {
        return DomainUpdateResult::task(DomainTask::none());
    }

    if switch_to_fullscreen {
        context.search_state_mut().mode = SearchMode::FullScreen;
    }

    if let Some(cached) = context.search_state().get_cached_results(&query) {
        let results = cached.results.clone();
        let total_count = cached.total_count;
        return DomainUpdateResult::task(DomainTask::perform(
            async move { (query, results, total_count) },
            |(query, results, total_count)| {
                C::search_message(SearchMessage::ResultsReceived {
                    query,
                    results,
                    total_count,
                })
            },
        ));
    }

    let strategy = SearchStrategy::Server;
    {
        let state = context.search_state_mut();
        state.current_strategy = Some(strategy);
        state.is_searching = true;
        state.last_search_time = Some(Instant::now());
    }

    let service = context.search_service();
    let fields = context.search_state().search_fields.clone();
    let fuzzy = context.search_state().fuzzy_matching;

    DomainUpdateResult::task(DomainTask::perform(
        async move {
            match service.search(&query, &fields, strategy, None, fuzzy).await {
                Ok(results) => {
                    let total_count = results.len();
                    (query, Ok((results, total_count)))
                }
                Err(error) => (query, Err(error)),
            }
        },
        |(query, result)| match result {
            Ok((results, total_count)) => {
                C::search_message(SearchMessage::ResultsReceived {
                    query,
                    results,
                    total_count,
                })
            }
            Err(error) => {
                C::search_message(SearchMessage::SearchError(error.to_string()))
            }
        },
    ))
}

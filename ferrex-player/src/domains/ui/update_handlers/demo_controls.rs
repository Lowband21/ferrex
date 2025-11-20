use iced::Task;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::library,
    infra::api_types::DemoResetRequest,
    state::State,
};

pub fn augment_show_library_management_tasks(
    state: &mut State,
    mut tasks: Vec<Task<DomainMessage>>,
) -> Vec<Task<DomainMessage>> {
    let controls = &mut state.domains.library.state.demo_controls;
    controls.is_loading = true;
    controls.error = None;

    tasks.push(Task::done(DomainMessage::Library(
        library::messages::Message::FetchDemoStatus,
    )));
    tasks
}

pub fn handle_movies_input(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    state.domains.library.state.demo_controls.movies_input = value;
    state.domains.library.state.demo_controls.error = None;
    DomainUpdateResult::task(Task::none())
}

pub fn handle_series_input(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    state.domains.library.state.demo_controls.series_input = value;
    state.domains.library.state.demo_controls.error = None;
    DomainUpdateResult::task(Task::none())
}

pub fn handle_apply_sizing(state: &mut State) -> DomainUpdateResult {
    let controls = &mut state.domains.library.state.demo_controls;
    match build_demo_reset_request(
        &controls.movies_input,
        &controls.series_input,
    ) {
        Ok(request) => {
            controls.error = None;
            controls.is_updating = true;
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ApplyDemoSizing(request),
            )))
        }
        Err(err) => {
            controls.error = Some(err);
            DomainUpdateResult::task(Task::none())
        }
    }
}

pub fn handle_refresh_status(state: &mut State) -> DomainUpdateResult {
    let controls = &mut state.domains.library.state.demo_controls;
    controls.is_loading = true;
    controls.error = None;
    DomainUpdateResult::task(Task::done(DomainMessage::Library(
        library::messages::Message::FetchDemoStatus,
    )))
}

fn parse_demo_numeric(input: &str) -> Result<Option<usize>, &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let value: usize =
        trimmed.parse().map_err(|_| "must be a positive integer")?;

    if value == 0 {
        Err("must be greater than zero")
    } else {
        Ok(Some(value))
    }
}

fn build_demo_reset_request(
    movies_input: &str,
    series_input: &str,
) -> Result<DemoResetRequest, String> {
    let movie_count = parse_demo_numeric(movies_input)
        .map_err(|err| format!("Movies {err}"))?;
    let series_count = parse_demo_numeric(series_input)
        .map_err(|err| format!("Series {err}"))?;

    if movie_count.is_none() && series_count.is_none() {
        return Err("Specify a size for movies, series, or both".into());
    }

    Ok(DemoResetRequest {
        movie_count,
        series_count,
    })
}

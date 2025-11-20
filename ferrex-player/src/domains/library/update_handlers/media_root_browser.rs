use ferrex_core::player_prelude::MediaRootBrowseResponse;
use iced::Task;
use std::path::PathBuf;

use crate::{
    domains::library::{
        media_root_browser::{
            Message as BrowserMessage, State as BrowserState,
        },
        messages::Message,
    },
    state::State,
};

pub fn update(state: &mut State, message: BrowserMessage) -> Task<Message> {
    match message {
        BrowserMessage::Open => handle_open(state),
        BrowserMessage::Close => handle_close(state),
        BrowserMessage::Browse { path } => handle_browse(state, path),
        BrowserMessage::ListingLoaded(result) => {
            handle_listing_loaded(state, result)
        }
        BrowserMessage::PathSelected(path) => handle_path_selected(state, path),
    }
}

fn browser_state(state: &mut State) -> &mut BrowserState {
    &mut state.domains.library.state.media_root_browser
}

fn handle_open(state: &mut State) -> Task<Message> {
    {
        let browser = browser_state(state);
        browser.visible = true;
        browser.error = None;
        browser.is_loading = true;
        browser.entries.clear();
    }
    handle_browse(state, None)
}

fn handle_close(state: &mut State) -> Task<Message> {
    let browser = browser_state(state);
    browser.visible = false;
    browser.is_loading = false;
    browser.error = None;
    Task::none()
}

fn handle_browse(state: &mut State, path: Option<String>) -> Task<Message> {
    let Some(api) = state.domains.library.state.api_service.clone() else {
        let browser = browser_state(state);
        browser.error = Some(
            "Server connection not available; cannot browse media root.".into(),
        );
        browser.is_loading = false;
        return Task::none();
    };

    let browser = browser_state(state);
    browser.is_loading = true;
    browser.error = None;

    Task::perform(
        async move {
            api.browse_media_root(path.as_deref())
                .await
                .map_err(|e| e.to_string())
        },
        |result| {
            Message::MediaRootBrowser(BrowserMessage::ListingLoaded(result))
        },
    )
}

fn handle_listing_loaded(
    state: &mut State,
    result: Result<MediaRootBrowseResponse, String>,
) -> Task<Message> {
    let browser = browser_state(state);
    browser.is_loading = false;
    match result {
        Ok(response) => {
            browser.media_root = Some(response.media_root);
            browser.current_path = response.current_path;
            browser.parent_path = response.parent_path;
            browser.display_path = response.display_path;
            browser.breadcrumbs = response.breadcrumbs;
            browser.entries = response.entries;
            browser.error = None;
        }
        Err(err) => browser.error = Some(err),
    }
    Task::none()
}

fn handle_path_selected(
    state: &mut State,
    relative_path: String,
) -> Task<Message> {
    let media_root = browser_state(state).media_root.clone();

    let Some(root) = media_root else {
        browser_state(state).error = Some(
            "Unable to resolve server media root; refresh browser and try again."
                .into(),
        );
        return Task::none();
    };

    let Some(form_data) =
        state.domains.library.state.library_form_data.as_mut()
    else {
        browser_state(state).error =
            Some("Open the library form before selecting folders.".into());
        return Task::none();
    };

    let mut absolute = std::path::PathBuf::from(root.clone());
    if !relative_path.is_empty() {
        for segment in relative_path.split('/') {
            if segment.is_empty() {
                continue;
            }
            absolute.push(segment);
        }
    }
    let absolute_str = absolute.display().to_string();

    let mut paths: Vec<String> = form_data
        .paths
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !paths.iter().any(|existing| existing == &absolute_str) {
        paths.push(absolute_str.clone());
    }
    form_data.paths = paths.join(", ");

    state
        .domains
        .library
        .state
        .library_form_errors
        .retain(|err| err != "At least one path is required");

    state.domains.library.state.library_form_success =
        Some(format!("Added {}", absolute_str));

    let browser = browser_state(state);
    browser.visible = false;
    browser.error = None;
    browser.is_loading = false;

    Task::none()
}

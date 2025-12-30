use crate::infra::app_state::AppState;

use ferrex_core::types::{Library, LibraryId, LibraryReference};

pub fn is_demo_mode(state: &AppState) -> bool {
    #[cfg(feature = "demo")]
    {
        state.demo().is_some()
    }

    #[cfg(not(feature = "demo"))]
    {
        let _ = state;
        false
    }
}

pub fn is_demo_library(id: &LibraryId) -> bool {
    #[cfg(feature = "demo")]
    {
        ferrex_core::domain::demo::is_demo_library(id)
    }

    #[cfg(not(feature = "demo"))]
    {
        let _ = id;
        false
    }
}

pub fn filter_libraries(
    state: &AppState,
    libraries: Vec<Library>,
) -> Vec<Library> {
    if !is_demo_mode(state) {
        return libraries;
    }

    libraries
        .into_iter()
        .filter(|library| is_demo_library(&library.id))
        .collect()
}

pub fn filter_library_references(
    state: &AppState,
    libraries: Vec<LibraryReference>,
) -> Vec<LibraryReference> {
    if !is_demo_mode(state) {
        return libraries;
    }

    libraries
        .into_iter()
        .filter(|library| is_demo_library(&library.id))
        .collect()
}

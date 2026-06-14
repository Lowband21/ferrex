use ferrex_core::player_prelude::Library;
use ferrex_model::{LibraryType, library::LibraryLikeMut};
use ferrex_player_api::{services::api::ApiService, testing::TestApiService};
use ferrex_player_foundation::repository::{RepositoryError, RepositoryResult};
use ferrex_player_library::{
    LibrariesLoadState, LibraryDomainState,
    messages::LibraryMessage,
    repository::{Accessor, MediaRepo, ReadWrite},
    types::LibrariesBootstrapPayload,
    update::{LibraryUpdateContext, update_library},
    update_handlers::library_loaded::{
        LibrariesLoadedContext, handle_libraries_loaded,
    },
};
use parking_lot::RwLock;
use rkyv::{rancor::Error as RkyvError, util::AlignedVec};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

struct TestContext {
    authenticated: bool,
    user_id: Option<Uuid>,
    server_url: String,
    loading: bool,
    state: LibraryDomainState,
    repo: Arc<RwLock<Option<MediaRepo>>>,
    api: Arc<dyn ApiService>,
}

impl TestContext {
    fn new(authenticated: bool) -> Self {
        let repo = Arc::new(RwLock::new(None));
        let accessor = Accessor::<ReadWrite>::new(Arc::clone(&repo));
        Self {
            authenticated,
            user_id: None,
            server_url: "http://localhost:8000".to_string(),
            loading: true,
            state: LibraryDomainState::new(
                Some(Arc::new(TestApiService::default())),
                accessor,
            ),
            repo,
            api: Arc::new(TestApiService::default()),
        }
    }
}

impl LibrariesLoadedContext for TestContext {
    fn library_state_mut(&mut self) -> &mut LibraryDomainState {
        &mut self.state
    }

    fn install_libraries_bootstrap(
        &mut self,
        payload: LibrariesBootstrapPayload,
    ) -> RepositoryResult<()> {
        let bytes: AlignedVec = rkyv::to_bytes::<RkyvError>(&payload.libraries)
            .map_err(|err| {
                RepositoryError::SerializationError(err.to_string())
            })?;
        let repo = MediaRepo::new(bytes).map_err(|err| {
            RepositoryError::DeserializationError(err.to_string())
        })?;
        *self.repo.write() = Some(repo);
        Ok(())
    }

    fn mark_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    fn session_user_id(&self) -> Option<Uuid> {
        self.user_id
    }

    fn server_url(&self) -> &str {
        &self.server_url
    }
}

impl LibraryUpdateContext for TestContext {
    type AppMessage = LibraryMessage;

    fn library_message(message: LibraryMessage) -> Self::AppMessage {
        message
    }

    fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    fn api_service(&self) -> Arc<dyn ApiService> {
        Arc::clone(&self.api)
    }

    fn set_libraries_for_navigation(&mut self, libraries: &[Library]) {
        self.state.libraries = libraries.to_vec();
    }
}

#[test]
fn load_is_gated_when_not_authenticated() {
    let mut ctx = TestContext::new(false);
    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::NotStarted
    ));
}

#[test]
fn start_load_transitions_to_in_progress_when_authenticated() {
    let mut ctx = TestContext::new(true);
    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::InProgress
    ));
}

#[test]
fn duplicate_load_during_in_progress_is_idempotent() {
    let mut ctx = TestContext::new(true);
    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::InProgress
    ));
}

#[test]
fn failure_transitions_to_failed_and_allows_retry() {
    let mut ctx = TestContext::new(true);
    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    let _ = handle_libraries_loaded(&mut ctx, Err("network error".to_string()));

    match &ctx.state.load_state {
        LibrariesLoadState::Failed { last_error } => {
            assert!(last_error.contains("network error"));
        }
        other => panic!("expected failed state, got {other:?}"),
    }

    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::InProgress
    ));
}

#[test]
fn successful_load_seeds_repo_with_libraries_index() {
    let mut ctx = TestContext::new(true);
    let movies = Library::new(
        "Movies".to_string(),
        LibraryType::Movies,
        vec![PathBuf::from("/tmp")],
    );
    let series = Library::new(
        "Series".to_string(),
        LibraryType::Series,
        vec![PathBuf::from("/tmp")],
    );

    let payload = LibrariesBootstrapPayload {
        libraries: vec![movies.clone(), series.clone()],
        movie_batches: Vec::new(),
        series_bundles: Vec::new(),
    };

    let _ = handle_libraries_loaded(&mut ctx, Ok(payload));

    let library_ids = ctx
        .state
        .repo_accessor
        .libraries_index()
        .expect("libraries index should be readable");
    assert_eq!(library_ids.len(), 2);
    assert!(library_ids.contains(&movies.id.0));
    assert!(library_ids.contains(&series.id.0));
}

#[test]
fn succeeded_same_session_ignores_duplicate_load() {
    let mut ctx = TestContext::new(true);
    let user_id = Uuid::now_v7();
    ctx.user_id = Some(user_id);
    ctx.state.load_state = LibrariesLoadState::Succeeded {
        user_id: Some(user_id),
        server_url: ctx.server_url.clone(),
    };

    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::Succeeded { .. }
    ));
}

#[test]
fn succeeded_different_server_triggers_reload() {
    let mut ctx = TestContext::new(true);
    let user_id = Uuid::now_v7();
    ctx.user_id = Some(user_id);
    ctx.state.load_state = LibrariesLoadState::Succeeded {
        user_id: Some(user_id),
        server_url: ctx.server_url.clone(),
    };
    ctx.server_url = "http://localhost:3999".to_string();

    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::InProgress
    ));
}

#[test]
fn succeeded_different_user_triggers_reload() {
    let mut ctx = TestContext::new(true);
    let user_a = Uuid::now_v7();
    let user_b = Uuid::now_v7();
    ctx.user_id = Some(user_b);
    ctx.state.load_state = LibrariesLoadState::Succeeded {
        user_id: Some(user_a),
        server_url: ctx.server_url.clone(),
    };

    let _ = update_library(&mut ctx, LibraryMessage::LoadLibraries);
    assert!(matches!(
        ctx.state.load_state,
        LibrariesLoadState::InProgress
    ));
}

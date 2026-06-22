use std::sync::Arc;

use ferrex_player_api::{services::api::ApiService, testing::TestApiService};
use ferrex_player_ui::{
    domains::ui::{
        collections::load_collection_summaries,
        shell_ui::{Scope, UiShellMessage, update_shell_ui},
        tabs::{CollectionsLoadState, TabId, TabState},
        types::ViewState,
        views::collections::{
            collection_summary_row, view_collection_detail, view_collections,
        },
    },
    state::State,
};

fn test_state() -> State {
    State::new("http://localhost:3000".to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn selecting_collections_scope_activates_tab_and_starts_load() {
    let mut state = test_state();

    let result = update_shell_ui(
        &mut state,
        UiShellMessage::SelectScope(Scope::Collections),
    );

    assert!(result.events.is_empty());
    assert_eq!(state.domains.ui.state.scope, Scope::Collections);
    assert_eq!(state.tab_manager.active_tab_id(), TabId::Collections);
    assert!(matches!(state.domains.ui.state.view, ViewState::Library));

    let Some(TabState::Collections(tab)) =
        state.tab_manager.get_tab(TabId::Collections)
    else {
        panic!("collections tab should be created");
    };
    assert_eq!(tab.load_state, CollectionsLoadState::Loading);
}

#[tokio::test(flavor = "current_thread")]
async fn collections_list_view_uses_test_api_summaries() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api);
    let payload = load_collection_summaries(api_service).await.unwrap();

    assert_eq!(payload.summaries.len(), 1);
    let row = collection_summary_row(&payload.summaries[0]);
    assert_eq!(row.title, "Sample Collection");
    assert_eq!(row.kind, "Manual");
    assert_eq!(row.source, "Manual source");
    assert_eq!(row.media_scope, "Movies only");
    assert_eq!(row.item_count, "2 items");

    let mut state = test_state();
    assert!(state.tab_manager.set_active_tab(TabId::Collections));
    state.domains.ui.state.scope = Scope::Collections;
    if let TabState::Collections(tab) =
        state.tab_manager.get_or_create_tab(TabId::Collections)
    {
        tab.mark_loaded(payload.summaries, payload.page);
    }

    let _ = view_collections(&state);
}

#[tokio::test(flavor = "current_thread")]
async fn collection_detail_view_renders_loaded_stub_detail() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api.clone());
    let payload = load_collection_summaries(api_service.clone())
        .await
        .unwrap();
    let collection_id = payload.summaries[0].identity.id;
    let detail = api_service
        .get_collection_detail(
            collection_id,
            ferrex_core::api::types::collections::GetCollectionDetailRequest {
                include_rule: true,
                include_items_preview: true,
                include_shelf_placements: true,
            },
        )
        .await
        .unwrap()
        .collection;

    let mut state = test_state();
    state.domains.ui.state.scope = Scope::Collections;
    state.domains.ui.state.view = ViewState::CollectionDetail { collection_id };
    if let TabState::Collections(tab) =
        state.tab_manager.get_or_create_tab(TabId::Collections)
    {
        tab.mark_loaded(payload.summaries, payload.page);
        tab.mark_detail_loaded(detail);
    }

    let _ = view_collection_detail(&state, collection_id);
}

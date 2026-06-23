use std::sync::Arc;

use chrono::Utc;
use ferrex_core::{
    api::types::collections::{
        CollectionMaterializationState, CollectionMember,
        CollectionMemberAvailability, CollectionMemberAvailabilityStatus,
        CollectionPageInfo, DynamicCollectionRule,
    },
    player_prelude::{EpisodeID, MovieID, SeriesID},
};
use ferrex_model::MediaID;
use ferrex_player_api::{services::api::ApiService, testing::TestApiService};
use ferrex_player_ui::{
    domains::ui::{
        collections::{load_collection_items, load_collection_summaries},
        shell_ui::{Scope, UiShellMessage, update_shell_ui},
        tabs::{
            CollectionItemsLoadState, CollectionItemsState,
            CollectionRefreshState, CollectionsLoadState, TabId, TabState,
        },
        types::ViewState,
        views::collections::{
            CollectionItemAction, collection_item_rows,
            collection_items_view_model, collection_status_summary,
            collection_summary_row, view_collection_detail, view_collections,
        },
    },
    state::State,
};
use uuid::Uuid;

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

    let items = load_collection_items(api_service, collection_id, None)
        .await
        .unwrap();
    if let TabState::Collections(tab) =
        state.tab_manager.get_or_create_tab(TabId::Collections)
    {
        tab.mark_items_loaded(
            collection_id,
            items.items,
            items.page,
            items.materialization,
            false,
        );
    }

    let _ = view_collection_detail(&state, collection_id);
}

#[tokio::test(flavor = "current_thread")]
async fn collection_detail_loads_paginated_items_in_stable_order() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api);
    let payload = load_collection_summaries(api_service.clone())
        .await
        .unwrap();
    let collection_id = payload.summaries[0].identity.id;

    let items = load_collection_items(api_service, collection_id, None)
        .await
        .unwrap();

    assert_eq!(items.page.total, 2);
    assert_eq!(items.items.len(), 2);
    assert_eq!(items.items[0].position, 1);
    assert_eq!(items.items[0].title, "First Sample Movie");
    assert_eq!(items.items[1].position, 2);
    assert_eq!(items.items[1].title, "Second Sample Movie");

    let rows = collection_item_rows(&items.items);
    assert_eq!(
        rows.iter().map(|row| row.position).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(matches!(
        rows[0].action,
        Some(CollectionItemAction::ViewMovie(_))
    ));
}

#[test]
fn collection_items_model_hides_unavailable_members_and_preserves_actions() {
    let movie_id = MovieID(Uuid::from_u128(1));
    let series_id = SeriesID(Uuid::from_u128(2));
    let episode_id = EpisodeID(Uuid::from_u128(3));
    let missing_id = MovieID(Uuid::from_u128(4));

    let mut movie = CollectionMember::new(MediaID::Movie(movie_id), "Movie", 2);
    let series = CollectionMember::new(MediaID::Series(series_id), "Series", 3);
    let episode =
        CollectionMember::new(MediaID::Episode(episode_id), "Episode", 4);
    let mut missing =
        CollectionMember::new(MediaID::Movie(missing_id), "Missing", 1);
    missing.availability = CollectionMemberAvailability {
        status: CollectionMemberAvailabilityStatus::Missing,
        ..CollectionMemberAvailability::default()
    };
    movie.subtitle = Some("Custom movie subtitle".to_string());

    let state = CollectionItemsState {
        items: vec![series, missing, episode, movie],
        page: Some(CollectionPageInfo {
            next_cursor: Some("4".to_string()),
            limit: 50,
            total: 4,
        }),
        materialization: None,
        load_state: CollectionItemsLoadState::Loaded,
    };

    let model = collection_items_view_model(Some(&state), 4);

    assert_eq!(model.hidden_count, 1);
    assert!(model.can_load_more);
    assert_eq!(
        model
            .rows
            .iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Movie", "Series", "Episode"]
    );
    assert_eq!(model.rows[0].subtitle, "Custom movie subtitle");
    assert_eq!(
        model.rows[0].action,
        Some(CollectionItemAction::ViewMovie(movie_id))
    );
    assert_eq!(
        model.rows[1].action,
        Some(CollectionItemAction::ViewSeries(series_id))
    );
    assert_eq!(
        model.rows[2].action,
        Some(CollectionItemAction::ViewEpisode(episode_id))
    );
    assert!(model.hidden_summary.unwrap().contains("hidden"));
}

#[tokio::test(flavor = "current_thread")]
async fn collection_status_model_surfaces_rule_provenance_refresh_and_errors() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api);
    let payload = load_collection_summaries(api_service.clone())
        .await
        .unwrap();
    let collection_id = payload.summaries[0].identity.id;
    let mut detail = api_service
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

    detail.summary.kind =
        ferrex_core::api::types::collections::CollectionKind::DynamicRule;
    detail.summary.source =
        ferrex_core::api::types::collections::CollectionSource::DynamicRule;
    detail.summary.provenance.source = detail.summary.source;
    detail.summary.provenance.rule_hash = Some("rule-hash".to_string());
    detail.summary.provenance.last_refreshed_at = Some(Utc::now());
    detail.summary.materialization.state =
        CollectionMaterializationState::Failed;
    detail.summary.materialization.generated_at = Some(Utc::now());
    detail.summary.materialization.last_error =
        Some("provider unavailable".to_string());
    detail.rule = Some(DynamicCollectionRule::default());

    let status = collection_status_summary(
        &detail,
        None,
        Some(&CollectionRefreshState::Error("offline".to_string())),
    );

    assert!(status.refresh_available);
    assert_eq!(status.refresh_label.as_deref(), Some("Retry refresh"));
    assert_eq!(status.refresh_error.as_deref(), Some("offline"));
    assert!(status.source_summary.contains("Dynamic rule"));
    assert!(status.provenance_summary.contains("Rule hash rule-hash"));
    assert!(status.materialization_summary.contains("Evaluated at"));
    assert!(
        status
            .materialization_summary
            .contains("provider unavailable")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn collection_detail_view_renders_item_error_retry_state() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api);
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
        tab.mark_items_error(collection_id, "offline");
    }

    let _ = view_collection_detail(&state, collection_id);
}

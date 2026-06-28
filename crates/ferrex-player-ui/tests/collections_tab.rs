use std::sync::Arc;

use chrono::Utc;
use ferrex_core::{
    api::types::collections::{
        ArchiveCollectionRequest, CollectionDuplicatePolicy, CollectionKind,
        CollectionManualAddItem, CollectionManualAddStatus,
        CollectionManualOrder, CollectionMaterializationState,
        CollectionMediaKind, CollectionMediaScope, CollectionMember,
        CollectionMemberAvailability, CollectionMemberAvailabilityStatus,
        CollectionPageInfo, CollectionSource, CreateCollectionRequest,
        DynamicCollectionRule, ManualAddCollectionItemsRequest,
        ManualRemoveCollectionItemsRequest,
        ManualReorderCollectionItemsRequest, UpdateCollectionRequest,
    },
    player_prelude::{EpisodeID, LibraryId, MovieID, SeriesID},
};
use ferrex_model::MediaID;
use ferrex_player_api::{services::api::ApiService, testing::TestApiService};
use ferrex_player_ui::{
    domains::ui::{
        collections::{
            CollectionsMessage, add_manual_collection_item, archive_collection,
            create_manual_collection, load_collection_items,
            load_collection_summaries, remove_manual_collection_item,
            reorder_manual_collection_items, update_collection_metadata,
            update_collections_ui, validate_media_scope_for_picker,
        },
        shell_ui::{Scope, UiShellMessage, update_shell_ui},
        tabs::{
            CollectionItemMutationKind, CollectionItemsLoadState,
            CollectionItemsState, CollectionPickerItem, CollectionRefreshState,
            CollectionsLoadState, TabId, TabState,
        },
        types::ViewState,
        views::collections::{
            CollectionItemAction, collection_item_rows,
            collection_items_empty_state_copy, collection_items_view_model,
            collection_status_summary, collection_summary_row,
            view_collection_detail, view_collections,
        },
    },
    state::State,
};
use uuid::Uuid;

fn test_state() -> State {
    State::new("http://localhost:3000".to_string())
}

fn manual_create_request(title: &str) -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: title.to_string(),
        description: Some("Editable from desktop".to_string()),
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: Default::default(),
        scope: Default::default(),
        visibility: Default::default(),
        presentation: Default::default(),
        media_scope: CollectionMediaScope::Types {
            media_types: vec![CollectionMediaKind::Movie],
        },
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: None,
        rule: None,
    }
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
async fn collection_detail_browse_mode_enters_and_exits_editor() {
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
        assert!(!tab.is_detail_editing(collection_id));
        assert!(tab.edit_forms.contains_key(&collection_id));
    }

    let _ = view_collection_detail(&state, collection_id);

    let _ = update_collections_ui(
        &mut state,
        CollectionsMessage::EnterEditMode(collection_id),
    );
    let Some(TabState::Collections(tab)) =
        state.tab_manager.get_tab(TabId::Collections)
    else {
        panic!("collections tab should exist");
    };
    assert!(tab.is_detail_editing(collection_id));
    assert!(tab.edit_forms.contains_key(&collection_id));

    let _ = view_collection_detail(&state, collection_id);

    let _ = update_collections_ui(
        &mut state,
        CollectionsMessage::ExitEditMode(collection_id),
    );
    let Some(TabState::Collections(tab)) =
        state.tab_manager.get_tab(TabId::Collections)
    else {
        panic!("collections tab should exist");
    };
    assert!(!tab.is_detail_editing(collection_id));

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

#[tokio::test(flavor = "current_thread")]
async fn picker_scope_validation_surfaces_media_scope_errors() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api);
    let payload = load_collection_summaries(api_service).await.unwrap();
    let summary = &payload.summaries[0];

    let series_item = CollectionPickerItem {
        media_id: MediaID::Series(SeriesID(Uuid::from_u128(
            0x6490000000000010,
        ))),
        title: "Sample Series".to_string(),
        subtitle: None,
        media_kind: CollectionMediaKind::Series,
        library_id: None,
    };

    let error = validate_media_scope_for_picker(summary, &series_item)
        .expect_err("series should be rejected by movie-only scope");
    assert!(error.contains("accepts movies"));
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

#[test]
fn collection_item_empty_copy_covers_browse_edit_and_unavailable_states() {
    let loaded_empty = CollectionItemsState {
        items: Vec::new(),
        page: Some(CollectionPageInfo {
            next_cursor: None,
            limit: 50,
            total: 0,
        }),
        materialization: None,
        load_state: CollectionItemsLoadState::Loaded,
    };
    let model = collection_items_view_model(Some(&loaded_empty), 0);

    let browse = collection_items_empty_state_copy(
        &model,
        Some(&loaded_empty),
        false,
        true,
    )
    .expect("empty browse state should produce copy");
    assert_eq!(browse.title, "No items in this collection");
    assert!(browse.body.contains("Manage collection"));

    let edit = collection_items_empty_state_copy(
        &model,
        Some(&loaded_empty),
        true,
        true,
    )
    .expect("empty edit state should produce copy");
    assert!(edit.body.contains("Search for existing media"));

    let loading = collection_items_empty_state_copy(&model, None, false, true)
        .expect("missing item state should be treated as loading");
    assert_eq!(loading.title, "Loading collection items…");

    let mut unavailable = CollectionMember::new(
        MediaID::Movie(MovieID(Uuid::from_u128(5))),
        "Unavailable",
        1,
    );
    unavailable.availability = CollectionMemberAvailability {
        status: CollectionMemberAvailabilityStatus::Unavailable,
        ..CollectionMemberAvailability::default()
    };
    let unavailable_state = CollectionItemsState {
        items: vec![unavailable],
        page: Some(CollectionPageInfo {
            next_cursor: None,
            limit: 50,
            total: 1,
        }),
        materialization: None,
        load_state: CollectionItemsLoadState::Loaded,
    };
    let hidden_model = collection_items_view_model(Some(&unavailable_state), 1);
    let hidden = collection_items_empty_state_copy(
        &hidden_model,
        Some(&unavailable_state),
        false,
        true,
    )
    .expect("hidden-only state should produce copy");
    assert_eq!(hidden.title, "No available items to show");
    assert!(hidden.body.contains("Unavailable"));
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

#[tokio::test(flavor = "current_thread")]
async fn manual_collection_api_flows_cover_editing_actions() {
    let api = TestApiService::default();
    let api_service: Arc<dyn ApiService> = Arc::new(api);

    let created = create_manual_collection(
        api_service.clone(),
        manual_create_request("Road Trip Queue"),
    )
    .await
    .unwrap();
    let collection_id = created.summary.identity.id;
    assert_eq!(created.summary.title, "Road Trip Queue");
    assert_eq!(
        created.summary.media_scope,
        CollectionMediaScope::Types {
            media_types: vec![CollectionMediaKind::Movie],
        }
    );

    let updated = update_collection_metadata(
        api_service.clone(),
        collection_id,
        UpdateCollectionRequest {
            title: Some("Road Trip Queue (edited)".to_string()),
            description: Some("Now with ordering".to_string()),
            media_scope: Some(CollectionMediaScope::Types {
                media_types: vec![CollectionMediaKind::Movie],
            }),
            expected_revision: Some(created.summary.version.revision),
            ..UpdateCollectionRequest::default()
        },
    )
    .await
    .unwrap()
    .collection;
    assert_eq!(updated.summary.title, "Road Trip Queue (edited)");

    let first = MediaID::Movie(MovieID(Uuid::from_u128(0x6490000000000001)));
    let second = MediaID::Movie(MovieID(Uuid::from_u128(0x6490000000000002)));
    let add = add_manual_collection_item(
        api_service.clone(),
        collection_id,
        ManualAddCollectionItemsRequest {
            items: vec![
                CollectionManualAddItem {
                    media_id: first,
                    title_override: Some("First Road Movie".to_string()),
                    position: None,
                },
                CollectionManualAddItem {
                    media_id: second,
                    title_override: Some("Second Road Movie".to_string()),
                    position: None,
                },
            ],
            duplicate_policy: None,
            expected_revision: Some(updated.summary.version.revision),
        },
    )
    .await
    .unwrap();
    assert_eq!(add.results.len(), 2);
    assert!(
        add.results
            .iter()
            .all(|result| result.status == CollectionManualAddStatus::Added)
    );

    let duplicate = add_manual_collection_item(
        api_service.clone(),
        collection_id,
        ManualAddCollectionItemsRequest {
            items: vec![CollectionManualAddItem {
                media_id: first,
                title_override: Some("Duplicate Road Movie".to_string()),
                position: None,
            }],
            duplicate_policy: None,
            expected_revision: Some(add.version.revision),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        duplicate.results[0].status,
        CollectionManualAddStatus::DuplicateSkipped
    );
    assert!(
        duplicate.results[0]
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("already present")
    );

    let first_key = add.results[0].item_key.clone();
    let second_key = add.results[1].item_key.clone();
    let reordered = reorder_manual_collection_items(
        api_service.clone(),
        collection_id,
        ManualReorderCollectionItemsRequest {
            ordering: vec![
                CollectionManualOrder {
                    item_key: second_key.clone(),
                    position: 1,
                },
                CollectionManualOrder {
                    item_key: first_key.clone(),
                    position: 2,
                },
            ],
            expected_revision: Some(duplicate.version.revision),
        },
    )
    .await
    .unwrap();

    let items = load_collection_items(api_service.clone(), collection_id, None)
        .await
        .unwrap();
    assert_eq!(items.items[0].item_key, second_key);
    assert_eq!(items.items[1].item_key, first_key);

    let removed = remove_manual_collection_item(
        api_service.clone(),
        collection_id,
        ManualRemoveCollectionItemsRequest {
            item_keys: vec![first_key],
            expected_revision: Some(reordered.version.revision),
        },
    )
    .await
    .unwrap();
    assert_eq!(removed.removed_item_keys.len(), 1);

    let stale = update_collection_metadata(
        api_service.clone(),
        collection_id,
        UpdateCollectionRequest {
            title: Some("Stale edit".to_string()),
            expected_revision: Some(created.summary.version.revision),
            ..UpdateCollectionRequest::default()
        },
    )
    .await
    .unwrap_err();
    assert!(stale.contains("version conflict"));

    let archived = archive_collection(
        api_service,
        collection_id,
        ArchiveCollectionRequest {
            expected_revision: Some(removed.version.revision),
            ..ArchiveCollectionRequest::default()
        },
    )
    .await
    .unwrap();
    assert!(archived.archived_at.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn manual_collection_editing_view_renders_recovery_states() {
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
    let items = load_collection_items(api_service.clone(), collection_id, None)
        .await
        .unwrap();
    let first_key = items.items[0].item_key.clone();

    let mut state = test_state();
    state.domains.ui.state.scope = Scope::Collections;
    state.domains.ui.state.view = ViewState::CollectionDetail { collection_id };
    if let TabState::Collections(tab) =
        state.tab_manager.get_or_create_tab(TabId::Collections)
    {
        tab.mark_loaded(payload.summaries, payload.page);
        tab.create_form.is_open = true;
        tab.create_form.error = Some("Server is offline".to_string());
        tab.mark_detail_loaded(detail.clone());
        tab.enter_detail_edit_mode(collection_id);
        tab.mark_items_loaded(
            collection_id,
            items.items.clone(),
            items.page.clone(),
            items.materialization.clone(),
            false,
        );
        let form = tab.ensure_edit_form(collection_id);
        form.title = "Edited title".to_string();
        form.is_dirty = true;
        form.error = Some(
            "Collection version conflict: expected revision 0, found 1"
                .to_string(),
        );
        form.conflict = true;
        let picker = tab.picker_state_mut(collection_id);
        picker.query = "sample".to_string();
        picker.error = Some(
            "First Sample Movie is already in this collection.".to_string(),
        );
        picker.conflict = true;
        picker.results.push(CollectionPickerItem {
            media_id: MediaID::Movie(MovieID(Uuid::from_u128(
                0x6490000000000003,
            ))),
            title: "Third Sample Movie".to_string(),
            subtitle: Some("2026".to_string()),
            media_kind: CollectionMediaKind::Movie,
            library_id: Some(LibraryId(Uuid::from_u128(0x6490000000000004))),
        });
        let action = tab.item_action_state_mut(collection_id);
        action.in_flight =
            Some(CollectionItemMutationKind::Reordering(first_key.clone()));
        action.error = Some(
            "Collection version conflict: expected revision 0, found 1"
                .to_string(),
        );
        action.conflict = true;
    }

    let _ = view_collections(&state);
    let _ = view_collection_detail(&state, collection_id);

    let _ = update_collections_ui(
        &mut state,
        CollectionsMessage::ReloadAfterConflict(collection_id),
    );
    let _ = update_collections_ui(
        &mut state,
        CollectionsMessage::DetailLoaded {
            collection_id,
            result: Ok(detail),
        },
    );
    let _ = update_collections_ui(
        &mut state,
        CollectionsMessage::ItemsLoaded {
            collection_id,
            append: false,
            result: Ok(ferrex_player_ui::domains::ui::collections::CollectionItemsPayload {
                items: items.items,
                page: items.page,
                materialization: items.materialization,
            }),
        },
    );

    let Some(TabState::Collections(tab)) =
        state.tab_manager.get_tab(TabId::Collections)
    else {
        panic!("collections tab should exist");
    };
    let form = tab.edit_forms.get(&collection_id).unwrap();
    assert!(!form.conflict);
    assert!(
        tab.picker_states
            .get(&collection_id)
            .is_some_and(|picker| !picker.conflict)
    );
    assert!(
        tab.item_action_states
            .get(&collection_id)
            .is_some_and(|action| !action.conflict)
    );
}

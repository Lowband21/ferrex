use std::sync::Arc;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{
        messages::UiMessage,
        tabs::{
            CollectionDetailLoadState, CollectionItemsLoadState,
            CollectionsTabState, TabId, TabState,
        },
        types::ViewState,
    },
    state::State,
};
use ferrex_core::api::types::collections::{
    CollectionDetail, CollectionId, CollectionMaterializationStatus,
    CollectionMember, CollectionPageInfo, CollectionPagination,
    CollectionSummary, CollectionVersion, DEFAULT_COLLECTION_PAGE_LIMIT,
    GetCollectionDetailRequest, ListCollectionItemsRequest,
    ListCollectionsRequest, MAX_COLLECTION_PAGE_LIMIT,
    RefreshCollectionRuleRequest,
};
use ferrex_player_api::services::api::ApiService;
use iced::{Task, widget::scrollable::AbsoluteOffset};

#[derive(Debug, Clone)]
pub struct CollectionListPayload {
    pub summaries: Vec<CollectionSummary>,
    pub page: CollectionPageInfo,
}

#[derive(Debug, Clone)]
pub struct CollectionItemsPayload {
    pub items: Vec<CollectionMember>,
    pub page: CollectionPageInfo,
    pub materialization: CollectionMaterializationStatus,
}

#[derive(Debug, Clone)]
pub struct CollectionRefreshPayload {
    pub materialization: CollectionMaterializationStatus,
    pub version: CollectionVersion,
}

#[derive(Debug, Clone)]
pub enum CollectionsMessage {
    Refresh,
    Retry,
    Loaded(Result<CollectionListPayload, String>),
    DetailLoaded {
        collection_id: CollectionId,
        result: Result<CollectionDetail, String>,
    },
    RetryDetailItems(CollectionId),
    LoadMoreItems(CollectionId),
    ItemsLoaded {
        collection_id: CollectionId,
        append: bool,
        result: Result<CollectionItemsPayload, String>,
    },
    RefreshMaterialization(CollectionId),
    MaterializationRefreshed {
        collection_id: CollectionId,
        result: Result<CollectionRefreshPayload, String>,
    },
}

impl From<CollectionsMessage> for UiMessage {
    fn from(message: CollectionsMessage) -> Self {
        UiMessage::Collections(message)
    }
}

impl CollectionsMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Refresh => "UI::CollectionsRefresh",
            Self::Retry => "UI::CollectionsRetry",
            Self::Loaded(_) => "UI::CollectionsLoaded",
            Self::DetailLoaded { .. } => "UI::CollectionDetailLoaded",
            Self::RetryDetailItems(_) => "UI::CollectionItemsRetry",
            Self::LoadMoreItems(_) => "UI::CollectionItemsLoadMore",
            Self::ItemsLoaded { .. } => "UI::CollectionItemsLoaded",
            Self::RefreshMaterialization(_) => {
                "UI::CollectionRefreshMaterialization"
            }
            Self::MaterializationRefreshed { .. } => {
                "UI::CollectionMaterializationRefreshed"
            }
        }
    }
}

pub async fn load_collection_summaries(
    api_service: Arc<dyn ApiService>,
) -> Result<CollectionListPayload, String> {
    let mut summaries = Vec::new();
    let mut cursor = None;
    let mut last_page: CollectionPageInfo;
    let page_limit =
        DEFAULT_COLLECTION_PAGE_LIMIT.min(MAX_COLLECTION_PAGE_LIMIT);

    loop {
        let response = api_service
            .list_collections(ListCollectionsRequest {
                page: CollectionPagination {
                    cursor: cursor.clone(),
                    limit: page_limit,
                },
                include_archived: false,
                include_item_counts: true,
                ..ListCollectionsRequest::default()
            })
            .await
            .map_err(|error| error.to_string())?;

        cursor = response.page.next_cursor.clone();
        last_page = response.page;
        summaries.extend(response.collections);

        if cursor.is_none() {
            break;
        }
    }

    Ok(CollectionListPayload {
        summaries,
        page: last_page,
    })
}

pub async fn load_collection_detail(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
) -> Result<CollectionDetail, String> {
    api_service
        .get_collection_detail(
            collection_id,
            GetCollectionDetailRequest {
                include_rule: true,
                include_items_preview: true,
                include_shelf_placements: true,
            },
        )
        .await
        .map(|response| response.collection)
        .map_err(|error| error.to_string())
}

pub async fn load_collection_items(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    cursor: Option<String>,
) -> Result<CollectionItemsPayload, String> {
    let page_limit =
        DEFAULT_COLLECTION_PAGE_LIMIT.min(MAX_COLLECTION_PAGE_LIMIT);
    api_service
        .list_collection_items(
            collection_id,
            ListCollectionItemsRequest {
                page: CollectionPagination {
                    cursor,
                    limit: page_limit,
                },
                availability: None,
            },
        )
        .await
        .map(|response| CollectionItemsPayload {
            items: response.items,
            page: response.page,
            materialization: response.materialization,
        })
        .map_err(|error| error.to_string())
}

pub async fn refresh_collection_materialization(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    expected_rule_hash: Option<String>,
) -> Result<CollectionRefreshPayload, String> {
    api_service
        .refresh_collection_rule(
            collection_id,
            RefreshCollectionRuleRequest {
                force: false,
                expected_rule_hash,
            },
        )
        .await
        .map(|response| CollectionRefreshPayload {
            materialization: response.materialization,
            version: response.version,
        })
        .map_err(|error| error.to_string())
}

pub fn update_collections_ui(
    state: &mut State,
    message: CollectionsMessage,
) -> DomainUpdateResult {
    match message {
        CollectionsMessage::Refresh | CollectionsMessage::Retry => {
            DomainUpdateResult::task(start_collections_refresh(state))
        }
        CollectionsMessage::Loaded(result) => {
            let tab = collections_tab_mut(state);
            match result {
                Ok(payload) => tab.mark_loaded(payload.summaries, payload.page),
                Err(message) => tab.mark_error(message),
            }
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::DetailLoaded {
            collection_id,
            result,
        } => {
            let tab = collections_tab_mut(state);
            match result {
                Ok(detail) => tab.mark_detail_loaded(detail),
                Err(message) => tab.mark_detail_error(collection_id, message),
            }
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::RetryDetailItems(collection_id) => {
            DomainUpdateResult::task(start_collection_items_load(
                state,
                collection_id,
                None,
                false,
            ))
        }
        CollectionsMessage::LoadMoreItems(collection_id) => {
            DomainUpdateResult::task(start_collection_items_load_more(
                state,
                collection_id,
            ))
        }
        CollectionsMessage::ItemsLoaded {
            collection_id,
            append,
            result,
        } => {
            let tab = collections_tab_mut(state);
            match result {
                Ok(payload) => tab.mark_items_loaded(
                    collection_id,
                    payload.items,
                    payload.page,
                    payload.materialization,
                    append,
                ),
                Err(message) => tab.mark_items_error(collection_id, message),
            }
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::RefreshMaterialization(collection_id) => {
            DomainUpdateResult::task(start_collection_materialization_refresh(
                state,
                collection_id,
            ))
        }
        CollectionsMessage::MaterializationRefreshed {
            collection_id,
            result,
        } => match result {
            Ok(payload) => {
                collections_tab_mut(state).mark_refresh_succeeded(
                    collection_id,
                    payload.materialization,
                    payload.version,
                );
                DomainUpdateResult::task(start_collection_items_load(
                    state,
                    collection_id,
                    None,
                    false,
                ))
            }
            Err(message) => {
                collections_tab_mut(state)
                    .mark_refresh_error(collection_id, message);
                DomainUpdateResult::task(Task::none())
            }
        },
    }
}

pub fn ensure_collections_loaded(state: &mut State) -> Task<DomainMessage> {
    let should_load = collections_tab_mut(state).should_load_initial();
    if should_load {
        start_collections_refresh(state)
    } else {
        Task::none()
    }
}

pub fn start_collections_refresh(state: &mut State) -> Task<DomainMessage> {
    collections_tab_mut(state).mark_loading();
    let api_service = state.api_service.clone();

    Task::perform(load_collection_summaries(api_service), |result| {
        DomainMessage::Ui(CollectionsMessage::Loaded(result).into())
    })
}

pub fn open_collection_detail(
    state: &mut State,
    collection_id: CollectionId,
) -> DomainUpdateResult {
    if !matches!(
        state.domains.ui.state.view,
        ViewState::CollectionDetail { collection_id: current }
            if current == collection_id
    ) {
        state
            .domains
            .ui
            .state
            .navigation_history
            .push(state.domains.ui.state.view.clone());
    }

    let new_view = ViewState::CollectionDetail { collection_id };
    state
        .domains
        .ui
        .state
        .background_shader_state
        .reset_to_library_colors();
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &new_view,
            state.window_size.width,
            state.window_size.height,
            None,
        );
    state.domains.ui.state.view = new_view;

    DomainUpdateResult::task(ensure_collection_detail_loaded(
        state,
        collection_id,
    ))
}

pub fn ensure_collection_detail_loaded(
    state: &mut State,
    collection_id: CollectionId,
) -> Task<DomainMessage> {
    let (should_load_detail, should_load_items) = {
        let tab = collections_tab_mut(state);
        let detail_state = tab.detail_state(collection_id);
        let items_state = tab.item_state(collection_id);
        (
            matches!(
                detail_state,
                CollectionDetailLoadState::NotLoaded
                    | CollectionDetailLoadState::Error(_)
            ),
            matches!(
                items_state.load_state,
                CollectionItemsLoadState::NotLoaded
                    | CollectionItemsLoadState::Error(_)
            ),
        )
    };

    let mut tasks = Vec::new();
    if should_load_detail {
        tasks.push(start_collection_detail_load(state, collection_id));
    }
    if should_load_items {
        tasks.push(start_collection_items_load(
            state,
            collection_id,
            None,
            false,
        ));
    }

    Task::batch(tasks)
}

pub fn start_collection_detail_load(
    state: &mut State,
    collection_id: CollectionId,
) -> Task<DomainMessage> {
    collections_tab_mut(state).mark_detail_loading(collection_id);
    let api_service = state.api_service.clone();

    Task::perform(
        load_collection_detail(api_service, collection_id),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::DetailLoaded {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    )
}

pub fn start_collection_items_load(
    state: &mut State,
    collection_id: CollectionId,
    cursor: Option<String>,
    append: bool,
) -> Task<DomainMessage> {
    collections_tab_mut(state).mark_items_loading(collection_id, append);
    let api_service = state.api_service.clone();

    Task::perform(
        load_collection_items(api_service, collection_id, cursor),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::ItemsLoaded {
                    collection_id,
                    append,
                    result,
                }
                .into(),
            )
        },
    )
}

pub fn start_collection_items_load_more(
    state: &mut State,
    collection_id: CollectionId,
) -> Task<DomainMessage> {
    let maybe_cursor = {
        let items_state = collections_tab_mut(state).item_state(collection_id);
        if items_state.load_state.is_loading() {
            None
        } else {
            items_state.next_cursor()
        }
    };

    match maybe_cursor {
        Some(cursor) => start_collection_items_load(
            state,
            collection_id,
            Some(cursor),
            true,
        ),
        None => Task::none(),
    }
}

pub fn start_collection_materialization_refresh(
    state: &mut State,
    collection_id: CollectionId,
) -> Task<DomainMessage> {
    let expected_rule_hash = collections_tab(state)
        .and_then(|tab| tab.summary(collection_id))
        .and_then(|summary| {
            summary
                .materialization
                .rule_hash
                .clone()
                .or_else(|| summary.provenance.rule_hash.clone())
        });

    collections_tab_mut(state).mark_refreshing(collection_id);
    let api_service = state.api_service.clone();

    Task::perform(
        refresh_collection_materialization(
            api_service,
            collection_id,
            expected_rule_hash,
        ),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::MaterializationRefreshed {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    )
}

pub fn restore_collections_scroll(state: &State) -> Task<DomainMessage> {
    let Some(TabState::Collections(collections)) =
        state.tab_manager.get_tab(TabId::Collections)
    else {
        return Task::none();
    };

    let position = state
        .domains
        .ui
        .state
        .scroll_manager
        .get_tab_scroll(&TabId::Collections)
        .map(|scroll| scroll.position)
        .unwrap_or(0.0);

    iced::widget::operation::scroll_to::<DomainMessage>(
        collections.scrollable_id.clone(),
        AbsoluteOffset {
            x: 0.0,
            y: position,
        },
    )
}

pub fn collections_tab(state: &State) -> Option<&CollectionsTabState> {
    match state.tab_manager.get_tab(TabId::Collections) {
        Some(TabState::Collections(tab)) => Some(tab),
        _ => None,
    }
}

fn collections_tab_mut(state: &mut State) -> &mut CollectionsTabState {
    match state.tab_manager.get_or_create_tab(TabId::Collections) {
        TabState::Collections(tab) => tab,
        _ => {
            unreachable!("collections tab id must resolve to collections state")
        }
    }
}

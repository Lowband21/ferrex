use std::sync::Arc;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{
        messages::UiMessage,
        tabs::{
            CollectionDetailLoadState, CollectionsTabState, TabId, TabState,
        },
        types::ViewState,
    },
    state::State,
};
use ferrex_core::api::types::collections::{
    CollectionDetail, CollectionId, CollectionPageInfo, CollectionPagination,
    CollectionSummary, DEFAULT_COLLECTION_PAGE_LIMIT,
    GetCollectionDetailRequest, ListCollectionsRequest,
    MAX_COLLECTION_PAGE_LIMIT,
};
use ferrex_player_api::services::api::ApiService;
use iced::{Task, widget::scrollable::AbsoluteOffset};

#[derive(Debug, Clone)]
pub struct CollectionListPayload {
    pub summaries: Vec<CollectionSummary>,
    pub page: CollectionPageInfo,
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
    let detail_state = collections_tab_mut(state).detail_state(collection_id);
    match detail_state {
        CollectionDetailLoadState::Loaded(_)
        | CollectionDetailLoadState::Loading => Task::none(),
        CollectionDetailLoadState::NotLoaded
        | CollectionDetailLoadState::Error(_) => {
            start_collection_detail_load(state, collection_id)
        }
    }
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

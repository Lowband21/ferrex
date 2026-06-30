use std::sync::Arc;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{
        messages::UiMessage,
        tabs::{
            CollectionDetailLoadState, CollectionEditFormState,
            CollectionItemMutationKind, CollectionItemsLoadState,
            CollectionMediaScopeChoice, CollectionPickerItem,
            CollectionsLoadState, CollectionsTabState, TabId, TabState,
        },
        types::ViewState,
    },
    state::State,
};
use ferrex_core::{
    api::types::collections::{
        ArchiveCollectionRequest, ArchiveCollectionResponse, CollectionDetail,
        CollectionDuplicatePolicy, CollectionId, CollectionKind,
        CollectionManualAddItem, CollectionManualAddResult,
        CollectionManualAddStatus, CollectionManualOrder,
        CollectionMaterializationStatus, CollectionMediaKind,
        CollectionMediaScope, CollectionMember, CollectionMemberKey,
        CollectionPageInfo, CollectionPagination, CollectionSource,
        CollectionSummary, CollectionVersion, CreateCollectionRequest,
        DEFAULT_COLLECTION_PAGE_LIMIT, GetCollectionDetailRequest,
        ListCollectionItemsRequest, ListCollectionsRequest,
        MAX_COLLECTION_PAGE_LIMIT, ManualAddCollectionItemsRequest,
        ManualAddCollectionItemsResponse, ManualRemoveCollectionItemsRequest,
        ManualRemoveCollectionItemsResponse,
        ManualReorderCollectionItemsRequest,
        ManualReorderCollectionItemsResponse, RefreshCollectionRuleRequest,
        UpdateCollectionRequest, UpdateCollectionResponse,
    },
    query::types::SearchField,
};
use ferrex_model::{Media, MediaID};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_search::{SearchResponse, SearchService, SearchStrategy};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionItemMoveDirection {
    Up,
    Down,
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
    ToggleCreateForm,
    CreateTitleChanged(String),
    CreateDescriptionChanged(String),
    CreateScopeChanged(CollectionMediaScopeChoice),
    SubmitCreate,
    CreateCompleted(Result<CollectionDetail, String>),
    EnterEditMode(CollectionId),
    ExitEditMode(CollectionId),
    EditTitleChanged(CollectionId, String),
    EditDescriptionChanged(CollectionId, String),
    EditScopeChanged(CollectionId, CollectionMediaScopeChoice),
    SaveMetadata(CollectionId),
    MetadataSaved {
        collection_id: CollectionId,
        result: Result<UpdateCollectionResponse, String>,
    },
    Archive(CollectionId),
    ArchiveCompleted {
        collection_id: CollectionId,
        result: Result<ArchiveCollectionResponse, String>,
    },
    ReloadAfterConflict(CollectionId),
    PickerQueryChanged(CollectionId, String),
    SearchPicker(CollectionId),
    PickerSearchLoaded {
        collection_id: CollectionId,
        result: Result<Vec<CollectionPickerItem>, String>,
    },
    AddPickerItem {
        collection_id: CollectionId,
        item: CollectionPickerItem,
    },
    PickerItemAdded {
        collection_id: CollectionId,
        result: Result<ManualAddCollectionItemsResponse, String>,
    },
    RemoveItem {
        collection_id: CollectionId,
        item_key: CollectionMemberKey,
    },
    ItemRemoved {
        collection_id: CollectionId,
        result: Result<ManualRemoveCollectionItemsResponse, String>,
    },
    MoveItem {
        collection_id: CollectionId,
        item_key: CollectionMemberKey,
        direction: CollectionItemMoveDirection,
    },
    ItemReordered {
        collection_id: CollectionId,
        result: Result<ManualReorderCollectionItemsResponse, String>,
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
            Self::ToggleCreateForm => "UI::CollectionToggleCreateForm",
            Self::CreateTitleChanged(_) => "UI::CollectionCreateTitleChanged",
            Self::CreateDescriptionChanged(_) => {
                "UI::CollectionCreateDescriptionChanged"
            }
            Self::CreateScopeChanged(_) => "UI::CollectionCreateScopeChanged",
            Self::SubmitCreate => "UI::CollectionCreateSubmit",
            Self::CreateCompleted(_) => "UI::CollectionCreateCompleted",
            Self::EnterEditMode(_) => "UI::CollectionEnterEditMode",
            Self::ExitEditMode(_) => "UI::CollectionExitEditMode",
            Self::EditTitleChanged(_, _) => "UI::CollectionEditTitleChanged",
            Self::EditDescriptionChanged(_, _) => {
                "UI::CollectionEditDescriptionChanged"
            }
            Self::EditScopeChanged(_, _) => "UI::CollectionEditScopeChanged",
            Self::SaveMetadata(_) => "UI::CollectionSaveMetadata",
            Self::MetadataSaved { .. } => "UI::CollectionMetadataSaved",
            Self::Archive(_) => "UI::CollectionArchive",
            Self::ArchiveCompleted { .. } => "UI::CollectionArchiveCompleted",
            Self::ReloadAfterConflict(_) => "UI::CollectionReloadAfterConflict",
            Self::PickerQueryChanged(_, _) => {
                "UI::CollectionPickerQueryChanged"
            }
            Self::SearchPicker(_) => "UI::CollectionPickerSearch",
            Self::PickerSearchLoaded { .. } => {
                "UI::CollectionPickerSearchLoaded"
            }
            Self::AddPickerItem { .. } => "UI::CollectionPickerAddItem",
            Self::PickerItemAdded { .. } => "UI::CollectionPickerItemAdded",
            Self::RemoveItem { .. } => "UI::CollectionRemoveItem",
            Self::ItemRemoved { .. } => "UI::CollectionItemRemoved",
            Self::MoveItem { .. } => "UI::CollectionMoveItem",
            Self::ItemReordered { .. } => "UI::CollectionItemReordered",
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

pub async fn create_manual_collection(
    api_service: Arc<dyn ApiService>,
    request: CreateCollectionRequest,
) -> Result<CollectionDetail, String> {
    api_service
        .create_collection(request)
        .await
        .map(|response| response.collection)
        .map_err(|error| error.to_string())
}

pub async fn update_collection_metadata(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    request: UpdateCollectionRequest,
) -> Result<UpdateCollectionResponse, String> {
    api_service
        .update_collection(collection_id, request)
        .await
        .map_err(|error| error.to_string())
}

pub async fn archive_collection(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    request: ArchiveCollectionRequest,
) -> Result<ArchiveCollectionResponse, String> {
    api_service
        .archive_collection(collection_id, request)
        .await
        .map_err(|error| error.to_string())
}

pub async fn add_manual_collection_item(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    request: ManualAddCollectionItemsRequest,
) -> Result<ManualAddCollectionItemsResponse, String> {
    api_service
        .manual_add_collection_items(collection_id, request)
        .await
        .map_err(|error| error.to_string())
}

pub async fn remove_manual_collection_item(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    request: ManualRemoveCollectionItemsRequest,
) -> Result<ManualRemoveCollectionItemsResponse, String> {
    api_service
        .manual_remove_collection_items(collection_id, request)
        .await
        .map_err(|error| error.to_string())
}

pub async fn reorder_manual_collection_items(
    api_service: Arc<dyn ApiService>,
    collection_id: CollectionId,
    request: ManualReorderCollectionItemsRequest,
) -> Result<ManualReorderCollectionItemsResponse, String> {
    api_service
        .manual_reorder_collection_items(collection_id, request)
        .await
        .map_err(|error| error.to_string())
}

pub async fn search_collection_picker(
    search_service: Arc<SearchService>,
    query: String,
) -> Result<Vec<CollectionPickerItem>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    search_service
        .search(
            &query,
            &[SearchField::Title],
            SearchStrategy::Server,
            None,
            true,
        )
        .await
        .map(|results| {
            results
                .into_iter()
                .map(collection_picker_item_from_search)
                .collect()
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
                Ok(detail) => {
                    tab.mark_detail_loaded(detail);
                    if let Some(form) = tab.edit_forms.get_mut(&collection_id)
                        && form.conflict
                    {
                        form.saving = false;
                        form.archiving = false;
                        form.error = None;
                        form.conflict = false;
                    }
                }
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
                Ok(payload) => {
                    tab.mark_items_loaded(
                        collection_id,
                        payload.items,
                        payload.page,
                        payload.materialization,
                        append,
                    );
                    if !append {
                        if let Some(picker) =
                            tab.picker_states.get_mut(&collection_id)
                            && picker.conflict
                        {
                            picker.adding = None;
                            picker.error = None;
                            picker.conflict = false;
                        }
                        if let Some(action) =
                            tab.item_action_states.get_mut(&collection_id)
                            && action.conflict
                        {
                            action.in_flight = None;
                            action.error = None;
                            action.conflict = false;
                        }
                    }
                }
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
        CollectionsMessage::ToggleCreateForm => {
            let form = &mut collections_tab_mut(state).create_form;
            form.is_open = !form.is_open;
            form.error = None;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::CreateTitleChanged(value) => {
            let form = &mut collections_tab_mut(state).create_form;
            form.title = value;
            form.error = None;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::CreateDescriptionChanged(value) => {
            let form = &mut collections_tab_mut(state).create_form;
            form.description = value;
            form.error = None;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::CreateScopeChanged(scope) => {
            let form = &mut collections_tab_mut(state).create_form;
            form.media_scope = scope;
            form.error = None;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::SubmitCreate => submit_create(state),
        CollectionsMessage::CreateCompleted(result) => {
            handle_create_completed(state, result)
        }
        CollectionsMessage::EnterEditMode(collection_id) => {
            collections_tab_mut(state).enter_detail_edit_mode(collection_id);
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::ExitEditMode(collection_id) => {
            collections_tab_mut(state).exit_detail_edit_mode(collection_id);
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::EditTitleChanged(collection_id, value) => {
            let form =
                collections_tab_mut(state).ensure_edit_form(collection_id);
            form.title = value;
            form.is_dirty = true;
            form.error = None;
            form.conflict = false;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::EditDescriptionChanged(collection_id, value) => {
            let form =
                collections_tab_mut(state).ensure_edit_form(collection_id);
            form.description = value;
            form.is_dirty = true;
            form.error = None;
            form.conflict = false;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::EditScopeChanged(collection_id, scope) => {
            let form =
                collections_tab_mut(state).ensure_edit_form(collection_id);
            form.media_scope = scope;
            form.is_dirty = true;
            form.error = None;
            form.conflict = false;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::SaveMetadata(collection_id) => {
            submit_metadata_save(state, collection_id)
        }
        CollectionsMessage::MetadataSaved {
            collection_id,
            result,
        } => handle_metadata_saved(state, collection_id, result),
        CollectionsMessage::Archive(collection_id) => {
            submit_archive(state, collection_id)
        }
        CollectionsMessage::ArchiveCompleted {
            collection_id,
            result,
        } => handle_archive_completed(state, collection_id, result),
        CollectionsMessage::ReloadAfterConflict(collection_id) => {
            mark_conflict_reloading(state, collection_id);
            DomainUpdateResult::task(Task::batch(vec![
                start_collection_detail_load(state, collection_id),
                start_collection_items_load(state, collection_id, None, false),
            ]))
        }
        CollectionsMessage::PickerQueryChanged(collection_id, value) => {
            let picker =
                collections_tab_mut(state).picker_state_mut(collection_id);
            picker.query = value;
            picker.error = None;
            picker.conflict = false;
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::SearchPicker(collection_id) => {
            submit_picker_search(state, collection_id)
        }
        CollectionsMessage::PickerSearchLoaded {
            collection_id,
            result,
        } => {
            let picker =
                collections_tab_mut(state).picker_state_mut(collection_id);
            picker.searching = false;
            match result {
                Ok(results) => {
                    picker.results = results;
                    picker.error = None;
                }
                Err(message) => picker.error = Some(message),
            }
            DomainUpdateResult::task(Task::none())
        }
        CollectionsMessage::AddPickerItem {
            collection_id,
            item,
        } => submit_add_picker_item(state, collection_id, item),
        CollectionsMessage::PickerItemAdded {
            collection_id,
            result,
        } => handle_picker_item_added(state, collection_id, result),
        CollectionsMessage::RemoveItem {
            collection_id,
            item_key,
        } => submit_remove_item(state, collection_id, item_key),
        CollectionsMessage::ItemRemoved {
            collection_id,
            result,
        } => handle_item_removed(state, collection_id, result),
        CollectionsMessage::MoveItem {
            collection_id,
            item_key,
            direction,
        } => submit_move_item(state, collection_id, item_key, direction),
        CollectionsMessage::ItemReordered {
            collection_id,
            result,
        } => handle_item_reordered(state, collection_id, result),
    }
}

fn submit_create(state: &mut State) -> DomainUpdateResult {
    let request = {
        let tab = collections_tab_mut(state);
        let form = &mut tab.create_form;
        let title = form.title.trim().to_string();
        if title.is_empty() {
            form.error = Some("A title is required.".to_string());
            return DomainUpdateResult::task(Task::none());
        }

        form.submitting = true;
        form.error = None;
        CreateCollectionRequest {
            title,
            description: optional_trimmed(&form.description),
            kind: CollectionKind::Manual,
            source: CollectionSource::Manual,
            media_scope: form.media_scope.as_scope(),
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
            ..default_manual_collection_request()
        }
    };
    let api_service = state.api_service.clone();

    DomainUpdateResult::task(Task::perform(
        create_manual_collection(api_service, request),
        |result| {
            DomainMessage::Ui(
                CollectionsMessage::CreateCompleted(result).into(),
            )
        },
    ))
}

fn handle_create_completed(
    state: &mut State,
    result: Result<CollectionDetail, String>,
) -> DomainUpdateResult {
    match result {
        Ok(detail) => {
            let collection_id = detail.summary.identity.id;
            {
                let tab = collections_tab_mut(state);
                tab.create_form.submitting = false;
                tab.create_form.reset_after_success();
                tab.create_form.is_open = false;
                if !tab
                    .summaries
                    .iter()
                    .any(|summary| summary.identity.id == collection_id)
                {
                    tab.summaries.push(detail.summary.clone());
                    tab.load_state = CollectionsLoadState::Loaded;
                }
                tab.mark_detail_loaded(detail);
            }
            open_collection_detail(state, collection_id)
        }
        Err(message) => {
            let form = &mut collections_tab_mut(state).create_form;
            form.submitting = false;
            form.error = Some(message);
            DomainUpdateResult::task(Task::none())
        }
    }
}

fn submit_metadata_save(
    state: &mut State,
    collection_id: CollectionId,
) -> DomainUpdateResult {
    if !is_manual_collection_in_state(state, collection_id) {
        collections_tab_mut(state)
            .ensure_edit_form(collection_id)
            .error =
            Some("Only manual collections can be edited here.".to_string());
        return DomainUpdateResult::task(Task::none());
    }

    let (expected_revision, current_scope_choice, current_description) =
        collections_tab(state)
            .and_then(|tab| collection_summary_for(tab, collection_id))
            .map(|summary| {
                (
                    Some(summary.version.revision),
                    Some(CollectionMediaScopeChoice::from_scope(
                        &summary.media_scope,
                    )),
                    summary.description.clone(),
                )
            })
            .unwrap_or((None, None, None));
    let request = {
        let tab = collections_tab_mut(state);
        let form = tab.ensure_edit_form(collection_id);
        let title = form.title.trim().to_string();
        if title.is_empty() {
            form.error = Some("A title is required.".to_string());
            return DomainUpdateResult::task(Task::none());
        }

        form.saving = true;
        form.error = None;
        form.conflict = false;
        let description = form.description.trim().to_string();
        let description = (current_description.as_deref().unwrap_or("")
            != description)
            .then_some(description);
        let media_scope = (current_scope_choice != Some(form.media_scope))
            .then(|| form.media_scope.as_scope());
        UpdateCollectionRequest {
            title: Some(title),
            description,
            media_scope,
            expected_revision,
            ..UpdateCollectionRequest::default()
        }
    };
    let api_service = state.api_service.clone();

    DomainUpdateResult::task(Task::perform(
        update_collection_metadata(api_service, collection_id, request),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::MetadataSaved {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    ))
}

fn handle_metadata_saved(
    state: &mut State,
    collection_id: CollectionId,
    result: Result<UpdateCollectionResponse, String>,
) -> DomainUpdateResult {
    let tab = collections_tab_mut(state);
    match result {
        Ok(response) => {
            let detail = response.collection;
            tab.mark_detail_loaded(detail.clone());
            tab.edit_forms.insert(
                collection_id,
                CollectionEditFormState::from_summary(&detail.summary),
            );
        }
        Err(message) => {
            let conflict = is_version_conflict(&message);
            let form = tab.ensure_edit_form(collection_id);
            form.saving = false;
            form.error = Some(message);
            form.conflict = conflict;
        }
    }
    DomainUpdateResult::task(Task::none())
}

fn submit_archive(
    state: &mut State,
    collection_id: CollectionId,
) -> DomainUpdateResult {
    if !is_manual_collection_in_state(state, collection_id) {
        collections_tab_mut(state)
            .ensure_edit_form(collection_id)
            .error =
            Some("Only manual collections can be archived here.".to_string());
        return DomainUpdateResult::task(Task::none());
    }

    let expected_revision =
        collection_expected_revision_in_state(state, collection_id);
    collections_tab_mut(state)
        .ensure_edit_form(collection_id)
        .archiving = true;
    let api_service = state.api_service.clone();
    let request = ArchiveCollectionRequest {
        archived: true,
        reason: Some("Archived from the desktop collection editor".into()),
        expected_revision,
    };

    DomainUpdateResult::task(Task::perform(
        archive_collection(api_service, collection_id, request),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::ArchiveCompleted {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    ))
}

fn handle_archive_completed(
    state: &mut State,
    collection_id: CollectionId,
    result: Result<ArchiveCollectionResponse, String>,
) -> DomainUpdateResult {
    match result {
        Ok(_response) => {
            collections_tab_mut(state).remove_collection(collection_id);
            state.domains.ui.state.view = ViewState::Library;
            DomainUpdateResult::task(start_collections_refresh(state))
        }
        Err(message) => {
            let conflict = is_version_conflict(&message);
            let form =
                collections_tab_mut(state).ensure_edit_form(collection_id);
            form.archiving = false;
            form.error = Some(message);
            form.conflict = conflict;
            DomainUpdateResult::task(Task::none())
        }
    }
}

fn mark_conflict_reloading(state: &mut State, collection_id: CollectionId) {
    let tab = collections_tab_mut(state);
    if let Some(form) = tab.edit_forms.get_mut(&collection_id) {
        form.error = None;
        form.conflict = true;
        form.is_dirty = false;
    }
    if let Some(picker) = tab.picker_states.get_mut(&collection_id) {
        picker.error = None;
        picker.conflict = true;
    }
    if let Some(action) = tab.item_action_states.get_mut(&collection_id) {
        action.error = None;
        action.conflict = true;
    }
}

fn submit_picker_search(
    state: &mut State,
    collection_id: CollectionId,
) -> DomainUpdateResult {
    let query = {
        let picker = collections_tab_mut(state).picker_state_mut(collection_id);
        let query = picker.query.trim().to_string();
        if query.is_empty() {
            picker.error = Some("Enter a title to search.".to_string());
            return DomainUpdateResult::task(Task::none());
        }
        picker.searching = true;
        picker.error = None;
        picker.conflict = false;
        query
    };
    let search_service = state.domains.search.service.clone();

    DomainUpdateResult::task(Task::perform(
        search_collection_picker(search_service, query),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::PickerSearchLoaded {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    ))
}

fn submit_add_picker_item(
    state: &mut State,
    collection_id: CollectionId,
    item: CollectionPickerItem,
) -> DomainUpdateResult {
    let expected_revision =
        match validate_picker_item(state, collection_id, &item) {
            Ok(expected_revision) => expected_revision,
            Err(message) => {
                collections_tab_mut(state)
                    .picker_state_mut(collection_id)
                    .error = Some(message);
                return DomainUpdateResult::task(Task::none());
            }
        };

    collections_tab_mut(state)
        .picker_state_mut(collection_id)
        .adding = Some(item.media_id);
    let api_service = state.api_service.clone();
    let request = ManualAddCollectionItemsRequest {
        items: vec![CollectionManualAddItem {
            media_id: item.media_id,
            title_override: Some(item.title),
            position: None,
        }],
        duplicate_policy: None,
        expected_revision,
    };

    DomainUpdateResult::task(Task::perform(
        add_manual_collection_item(api_service, collection_id, request),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::PickerItemAdded {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    ))
}

fn handle_picker_item_added(
    state: &mut State,
    collection_id: CollectionId,
    result: Result<ManualAddCollectionItemsResponse, String>,
) -> DomainUpdateResult {
    match result {
        Ok(response) => {
            let feedback = manual_add_feedback(&response.results);
            let tab = collections_tab_mut(state);
            tab.apply_collection_version(collection_id, response.version);
            let picker = tab.picker_state_mut(collection_id);
            picker.adding = None;
            picker.error = feedback;
            picker.conflict = false;
            DomainUpdateResult::task(reload_collection_after_manual_mutation(
                state,
                collection_id,
            ))
        }
        Err(message) => {
            let conflict = is_version_conflict(&message);
            let picker =
                collections_tab_mut(state).picker_state_mut(collection_id);
            picker.adding = None;
            picker.error = Some(message);
            picker.conflict = conflict;
            DomainUpdateResult::task(Task::none())
        }
    }
}

fn submit_remove_item(
    state: &mut State,
    collection_id: CollectionId,
    item_key: CollectionMemberKey,
) -> DomainUpdateResult {
    let expected_revision =
        match validate_manual_item_mutation(state, collection_id) {
            Ok(expected_revision) => expected_revision,
            Err(message) => {
                collections_tab_mut(state)
                    .item_action_state_mut(collection_id)
                    .error = Some(message);
                return DomainUpdateResult::task(Task::none());
            }
        };

    collections_tab_mut(state)
        .item_action_state_mut(collection_id)
        .in_flight =
        Some(CollectionItemMutationKind::Removing(item_key.clone()));
    let api_service = state.api_service.clone();
    let request = ManualRemoveCollectionItemsRequest {
        item_keys: vec![item_key],
        expected_revision,
    };

    DomainUpdateResult::task(Task::perform(
        remove_manual_collection_item(api_service, collection_id, request),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::ItemRemoved {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    ))
}

fn handle_item_removed(
    state: &mut State,
    collection_id: CollectionId,
    result: Result<ManualRemoveCollectionItemsResponse, String>,
) -> DomainUpdateResult {
    match result {
        Ok(response) => {
            let missing = response.missing_item_keys.clone();
            let tab = collections_tab_mut(state);
            tab.apply_collection_version(collection_id, response.version);
            let action = tab.item_action_state_mut(collection_id);
            action.in_flight = None;
            action.error = if missing.is_empty() {
                None
            } else {
                Some(format!(
                    "{} item{} could not be removed because the server no longer has them.",
                    missing.len(),
                    if missing.len() == 1 { "" } else { "s" }
                ))
            };
            action.conflict = false;
            DomainUpdateResult::task(reload_collection_after_manual_mutation(
                state,
                collection_id,
            ))
        }
        Err(message) => {
            let conflict = is_version_conflict(&message);
            let action =
                collections_tab_mut(state).item_action_state_mut(collection_id);
            action.in_flight = None;
            action.error = Some(message);
            action.conflict = conflict;
            DomainUpdateResult::task(Task::none())
        }
    }
}

fn submit_move_item(
    state: &mut State,
    collection_id: CollectionId,
    item_key: CollectionMemberKey,
    direction: CollectionItemMoveDirection,
) -> DomainUpdateResult {
    let expected_revision =
        match validate_manual_item_mutation(state, collection_id) {
            Ok(expected_revision) => expected_revision,
            Err(message) => {
                collections_tab_mut(state)
                    .item_action_state_mut(collection_id)
                    .error = Some(message);
                return DomainUpdateResult::task(Task::none());
            }
        };

    let ordering =
        match reorder_for_move(state, collection_id, &item_key, direction) {
            Ok(ordering) => ordering,
            Err(message) => {
                collections_tab_mut(state)
                    .item_action_state_mut(collection_id)
                    .error = Some(message);
                return DomainUpdateResult::task(Task::none());
            }
        };

    collections_tab_mut(state)
        .item_action_state_mut(collection_id)
        .in_flight = Some(CollectionItemMutationKind::Reordering(item_key));
    let api_service = state.api_service.clone();
    let request = ManualReorderCollectionItemsRequest {
        ordering,
        expected_revision,
    };

    DomainUpdateResult::task(Task::perform(
        reorder_manual_collection_items(api_service, collection_id, request),
        move |result| {
            DomainMessage::Ui(
                CollectionsMessage::ItemReordered {
                    collection_id,
                    result,
                }
                .into(),
            )
        },
    ))
}

fn handle_item_reordered(
    state: &mut State,
    collection_id: CollectionId,
    result: Result<ManualReorderCollectionItemsResponse, String>,
) -> DomainUpdateResult {
    match result {
        Ok(response) => {
            let tab = collections_tab_mut(state);
            tab.apply_collection_version(collection_id, response.version);
            let action = tab.item_action_state_mut(collection_id);
            action.in_flight = None;
            action.error = None;
            action.conflict = false;
            DomainUpdateResult::task(reload_collection_after_manual_mutation(
                state,
                collection_id,
            ))
        }
        Err(message) => {
            let conflict = is_version_conflict(&message);
            let action =
                collections_tab_mut(state).item_action_state_mut(collection_id);
            action.in_flight = None;
            action.error = Some(message);
            action.conflict = conflict;
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

fn default_manual_collection_request() -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: String::new(),
        description: None,
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: Default::default(),
        scope: Default::default(),
        visibility: Default::default(),
        presentation: Default::default(),
        media_scope: CollectionMediaScope::All,
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: None,
        rule: None,
    }
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn is_manual_collection(summary: &CollectionSummary) -> bool {
    matches!(summary.kind, CollectionKind::Manual)
        && matches!(summary.source, CollectionSource::Manual)
        && summary.timestamps.archived_at.is_none()
}

fn is_manual_collection_in_state(
    state: &State,
    collection_id: CollectionId,
) -> bool {
    collections_tab(state)
        .and_then(|tab| collection_summary_for(tab, collection_id))
        .is_some_and(is_manual_collection)
}

fn collection_expected_revision_in_state(
    state: &State,
    collection_id: CollectionId,
) -> Option<u64> {
    collections_tab(state)
        .and_then(|tab| collection_summary_for(tab, collection_id))
        .map(|summary| summary.version.revision)
}

fn collection_summary_for(
    tab: &CollectionsTabState,
    collection_id: CollectionId,
) -> Option<&CollectionSummary> {
    tab.detail_states
        .get(&collection_id)
        .and_then(|state| match state {
            CollectionDetailLoadState::Loaded(detail) => Some(&detail.summary),
            CollectionDetailLoadState::NotLoaded
            | CollectionDetailLoadState::Loading
            | CollectionDetailLoadState::Error(_) => None,
        })
        .or_else(|| tab.summary(collection_id))
}

fn validate_picker_item(
    state: &State,
    collection_id: CollectionId,
    item: &CollectionPickerItem,
) -> Result<Option<u64>, String> {
    let tab = collections_tab(state)
        .ok_or_else(|| "Collections are not loaded yet.".to_string())?;
    let summary = collection_summary_for(tab, collection_id)
        .ok_or_else(|| "Collection detail is not loaded yet.".to_string())?;
    if !is_manual_collection(summary) {
        return Err("Only manual collections can accept added media.".into());
    }
    validate_media_scope_for_picker(summary, item)?;

    let item_key = CollectionMemberKey::for_media(&item.media_id);
    if tab.item_states.get(&collection_id).is_some_and(|state| {
        state.items.iter().any(|member| member.item_key == item_key)
    }) {
        return Err(format!("{} is already in this collection.", item.title));
    }

    Ok(Some(summary.version.revision))
}

fn validate_manual_item_mutation(
    state: &State,
    collection_id: CollectionId,
) -> Result<Option<u64>, String> {
    let tab = collections_tab(state)
        .ok_or_else(|| "Collections are not loaded yet.".to_string())?;
    let summary = collection_summary_for(tab, collection_id)
        .ok_or_else(|| "Collection detail is not loaded yet.".to_string())?;
    if !is_manual_collection(summary) {
        return Err(
            "Only manual collections can change item membership.".into()
        );
    }
    Ok(Some(summary.version.revision))
}

pub fn validate_media_scope_for_picker(
    summary: &CollectionSummary,
    item: &CollectionPickerItem,
) -> Result<(), String> {
    match &summary.media_scope {
        CollectionMediaScope::All => Ok(()),
        CollectionMediaScope::Types { media_types } => {
            if media_types.is_empty() || media_types.contains(&item.media_kind)
            {
                Ok(())
            } else {
                Err(format!(
                    "{} cannot be added because this collection accepts {}.",
                    item.title,
                    media_scope_label_for_error(&summary.media_scope)
                ))
            }
        }
        CollectionMediaScope::Library {
            library_id,
            media_types,
        } => {
            if item
                .library_id
                .is_some_and(|item_library| item_library != *library_id)
            {
                return Err(format!(
                    "{} belongs to a different library than this collection.",
                    item.title
                ));
            }
            if media_types.is_empty() || media_types.contains(&item.media_kind)
            {
                Ok(())
            } else {
                Err(format!(
                    "{} cannot be added because this collection accepts {}.",
                    item.title,
                    media_scope_label_for_error(&summary.media_scope)
                ))
            }
        }
        CollectionMediaScope::ExplicitItems { item_keys } => {
            let item_key = CollectionMemberKey::for_media(&item.media_id);
            if item_keys.contains(&item_key) {
                Ok(())
            } else {
                Err(format!(
                    "{} is not part of this explicit-item collection scope.",
                    item.title
                ))
            }
        }
    }
}

fn media_scope_label_for_error(scope: &CollectionMediaScope) -> String {
    match scope {
        CollectionMediaScope::All => "all media".to_string(),
        CollectionMediaScope::Types { media_types } => {
            media_kind_list(media_types)
        }
        CollectionMediaScope::Library { media_types, .. } => {
            if media_types.is_empty() {
                "items from its library".to_string()
            } else {
                media_kind_list(media_types)
            }
        }
        CollectionMediaScope::ExplicitItems { .. } => {
            "preselected explicit items".to_string()
        }
    }
}

fn media_kind_list(media_types: &[CollectionMediaKind]) -> String {
    if media_types.is_empty() {
        return "all media types".to_string();
    }
    media_types
        .iter()
        .map(|kind| match kind {
            CollectionMediaKind::Movie => "movies",
            CollectionMediaKind::Series => "series",
            CollectionMediaKind::Season => "seasons",
            CollectionMediaKind::Episode => "episodes",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn reload_collection_after_manual_mutation(
    state: &mut State,
    collection_id: CollectionId,
) -> Task<DomainMessage> {
    Task::batch(vec![
        start_collection_detail_load(state, collection_id),
        start_collection_items_load(state, collection_id, None, false),
    ])
}

fn reorder_for_move(
    state: &State,
    collection_id: CollectionId,
    item_key: &CollectionMemberKey,
    direction: CollectionItemMoveDirection,
) -> Result<Vec<CollectionManualOrder>, String> {
    let item_state = collections_tab(state)
        .and_then(|tab| tab.item_states.get(&collection_id))
        .ok_or_else(|| {
            "Load collection items before reordering.".to_string()
        })?;
    if item_state.has_more() {
        return Err(
            "Load all items before reordering so the saved order is stable."
                .to_string(),
        );
    }

    let mut items = item_state.items.clone();
    items.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.item_key.cmp(&right.item_key))
    });
    let index = items
        .iter()
        .position(|member| &member.item_key == item_key)
        .ok_or_else(|| "That item is no longer loaded.".to_string())?;
    let target = match direction {
        CollectionItemMoveDirection::Up if index == 0 => {
            return Err("This item is already first.".to_string());
        }
        CollectionItemMoveDirection::Up => index - 1,
        CollectionItemMoveDirection::Down if index + 1 >= items.len() => {
            return Err("This item is already last.".to_string());
        }
        CollectionItemMoveDirection::Down => index + 1,
    };
    items.swap(index, target);

    Ok(items
        .into_iter()
        .enumerate()
        .map(|(index, member)| CollectionManualOrder {
            item_key: member.item_key,
            position: (index as u32).saturating_add(1),
        })
        .collect())
}

fn manual_add_feedback(
    results: &[CollectionManualAddResult],
) -> Option<String> {
    let messages = results
        .iter()
        .filter(|result| result.status != CollectionManualAddStatus::Added)
        .map(|result| {
            result
                .message
                .clone()
                .unwrap_or_else(|| match result.status {
                    CollectionManualAddStatus::Added => "Added".to_string(),
                    CollectionManualAddStatus::AlreadyPresent
                    | CollectionManualAddStatus::DuplicateSkipped => {
                        "Item is already present in this collection".to_string()
                    }
                    CollectionManualAddStatus::Unavailable => {
                        "Item is unavailable for this collection".to_string()
                    }
                })
        })
        .collect::<Vec<_>>();

    (!messages.is_empty()).then(|| messages.join("; "))
}

fn is_version_conflict(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("version conflict")
        || lower.contains("expected revision")
        || lower.contains("stale")
}

fn collection_picker_item_from_search(
    response: SearchResponse,
) -> CollectionPickerItem {
    let media_id = media_id_from_media(&response.media_ref);
    CollectionPickerItem {
        media_id,
        title: response.title,
        subtitle: response.subtitle,
        media_kind: CollectionMediaKind::from(&media_id),
        library_id: response.library_id,
    }
}

fn media_id_from_media(media: &Media) -> MediaID {
    match media {
        Media::Movie(movie) => MediaID::Movie(movie.id),
        Media::Series(series) => MediaID::Series(series.id),
        Media::Season(season) => MediaID::Season(season.id),
        Media::Episode(episode) => MediaID::Episode(episode.id),
    }
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

use chrono::Utc;
use ferrex_player_api::api_types::{
    CollectionArtwork, CollectionDuplicatePolicy, CollectionId,
    CollectionIdentity, CollectionKind, CollectionMaterializationStatus,
    CollectionMediaKind, CollectionMediaScope, CollectionOwner,
    CollectionPresentationMode, CollectionProvenance, CollectionScope,
    CollectionSource, CollectionSummary, CollectionTheme, CollectionTimestamps,
    CollectionVersion, CollectionVisibility, IntelligenceError,
    IntelligenceErrorCode, IntelligenceModelStatus, IntelligenceProviderState,
    IntelligenceProviderStatus, IntelligenceRunPurpose, IntelligenceRunStatus,
    IntelligenceRunStatusResponse, IntelligenceSummary, MediaID, MovieID,
    SMART_SHELF_DRAFT_SCHEMA_VERSION, SmartShelfDraftAlternate,
    SmartShelfDraftContent, SmartShelfDraftItem, SmartShelfDraftResponse,
    SmartShelfDraftSource, SmartShelfDraftValidation, SmartShelfErrorCode,
    SmartShelfSaveResponse, SmartShelfStartResponse,
};
use ferrex_player_intelligence::{
    SmartShelfFailure, SmartShelfFailureCode, SmartShelfMessage,
    SmartShelfPhase, SmartShelfSaveConflictRecovery, SmartShelfSaveStatus,
};
use ferrex_player_ui::{
    domains::{
        search::SearchPresentation,
        ui::{
            collections::CollectionsMessage,
            shell_ui::Scope,
            smart_shelf::{
                SmartShelfUiMessage, save_conflict_recovery_label,
                update_smart_shelf_ui,
            },
            tabs::TabId,
            views::{
                collections::view_collections,
                header::view_header,
                smart_shelf::{
                    smart_shelf_composer_summary,
                    smart_shelf_draft_review_summary,
                    smart_shelf_progress_summary,
                    smart_shelf_save_review_summary, view_smart_shelf_surface,
                },
            },
        },
    },
    state::State,
};
use uuid::Uuid;

fn test_state() -> State {
    State::new("http://localhost:3000".to_string())
}

fn movie(n: u128) -> MediaID {
    MediaID::Movie(MovieID(Uuid::from_u128(n)))
}

fn source(media_id: MediaID, label: &str) -> SmartShelfDraftSource {
    SmartShelfDraftSource {
        label: Some(label.to_string()),
        media_id: Some(media_id),
        artifact_id: None,
        field: Some("watch_state".to_string()),
        evidence: Some(IntelligenceSummary::new(format!(
            "{label} grounded this selection"
        ))),
    }
}

fn provider_ready() -> IntelligenceProviderStatus {
    IntelligenceProviderStatus {
        enabled: true,
        provider_name: "test-provider".to_string(),
        base_url: "https://llm.test".to_string(),
        api_key_configured: true,
        default_model: Some("test-model".to_string()),
        state: IntelligenceProviderState::Ready,
        models: vec![IntelligenceModelStatus {
            name: "test-model".to_string(),
            selected: true,
            available: true,
            supports_tools: true,
            context_window_tokens: Some(8192),
        }],
        checked_at_epoch_seconds: Some(Utc::now().timestamp()),
        error: None,
    }
}

fn provider_unavailable() -> IntelligenceProviderStatus {
    IntelligenceProviderStatus {
        enabled: true,
        provider_name: "test-provider".to_string(),
        base_url: "https://llm.test".to_string(),
        api_key_configured: false,
        default_model: None,
        state: IntelligenceProviderState::NotConfigured,
        models: Vec::new(),
        checked_at_epoch_seconds: Some(Utc::now().timestamp()),
        error: Some(IntelligenceError {
            code: IntelligenceErrorCode::ProviderNotConfigured,
            message: "Configure a provider before generating shelves"
                .to_string(),
            retryable: false,
            details: serde_json::Value::Null,
        }),
    }
}

fn start_response(run_id: Uuid) -> SmartShelfStartResponse {
    SmartShelfStartResponse {
        run_id,
        status: IntelligenceRunStatus::Running,
        provider: Some("test-provider".to_string()),
        model: Some("test-model".to_string()),
        queued_at_epoch_seconds: Some(Utc::now().timestamp()),
        draft_schema_version: SMART_SHELF_DRAFT_SCHEMA_VERSION,
    }
}

fn running_status(run_id: Uuid) -> IntelligenceRunStatusResponse {
    IntelligenceRunStatusResponse {
        run_id,
        purpose: IntelligenceRunPurpose::Recommendation,
        status: IntelligenceRunStatus::Running,
        terminal: false,
        current_phase: Some("ranking grounded candidates".to_string()),
        provider: Some("test-provider".to_string()),
        model: Some("test-model".to_string()),
        queued_at_epoch_seconds: Some(Utc::now().timestamp()),
        started_at_epoch_seconds: Some(Utc::now().timestamp()),
        completed_at_epoch_seconds: None,
        current_step: Some(2),
        max_steps: Some(4),
        draft_artifact_ids: Vec::new(),
        output_summary: None,
        error: None,
    }
}

fn draft_response(artifact_id: Uuid) -> SmartShelfDraftResponse {
    let first = movie(1);
    let second = movie(2);
    let alternate = movie(3);

    SmartShelfDraftResponse {
        artifact_id,
        run_id: Some(Uuid::from_u128(10)),
        owner_user_id: None,
        title: "Rainy night smart shelf".to_string(),
        summary: Some(IntelligenceSummary::new("A cozy grounded shelf")),
        draft: Some(SmartShelfDraftContent {
            schema_version: SMART_SHELF_DRAFT_SCHEMA_VERSION,
            title: "Rainy night smart shelf".to_string(),
            description: Some(
                "Atmospheric movies for a stormy evening".to_string(),
            ),
            interpreted_intent: Some("cozy rainy-night shelf".to_string()),
            requested_constraints: serde_json::json!({
                "mood": "cozy_rainy_night",
                "avoid_duplicates": true
            }),
            items: vec![
                SmartShelfDraftItem {
                    ordinal: 1,
                    media_id: first,
                    title: Some("First Rain Movie".to_string()),
                    subtitle: Some("Movie · 1999".to_string()),
                    year: Some(1999),
                    reason: Some("Matches the rainy-night mood".to_string()),
                    sources: vec![source(first, "Mood match")],
                    locked: false,
                    replacement_of: None,
                },
                SmartShelfDraftItem {
                    ordinal: 2,
                    media_id: second,
                    title: Some("Second Comfort Movie".to_string()),
                    subtitle: Some("Movie · 2005".to_string()),
                    year: Some(2005),
                    reason: Some("Balances the shelf with comfort".to_string()),
                    sources: vec![source(second, "Watch history")],
                    locked: false,
                    replacement_of: None,
                },
            ],
            alternates: vec![SmartShelfDraftAlternate {
                target_ordinal: Some(1),
                media_id: alternate,
                title: Some("Alternate Storm Movie".to_string()),
                subtitle: Some("Movie · 2010".to_string()),
                year: Some(2010),
                reason: Some(
                    "Similar atmosphere with fresher pacing".to_string(),
                ),
                sources: vec![source(alternate, "Related metadata")],
            }],
        }),
        validation: SmartShelfDraftValidation {
            valid: true,
            issues: Vec::new(),
        },
        saved_collection_id: None,
    }
}

fn collection_summary(collection_id: CollectionId) -> CollectionSummary {
    let now = Utc::now();
    CollectionSummary {
        identity: CollectionIdentity::for_id(collection_id),
        title: "Rainy night smart shelf".to_string(),
        description: Some(
            "Atmospheric movies for a stormy evening".to_string(),
        ),
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: CollectionOwner::default(),
        scope: CollectionScope::User,
        visibility: CollectionVisibility::Private,
        presentation: CollectionPresentationMode::Shelf,
        media_scope: CollectionMediaScope::Types {
            media_types: vec![CollectionMediaKind::Movie],
        },
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: CollectionArtwork::default(),
        theme: CollectionTheme::default(),
        provenance: CollectionProvenance::default(),
        version: CollectionVersion {
            revision: 1,
            etag: Some("collection-test-1".to_string()),
            ..CollectionVersion::default()
        },
        timestamps: CollectionTimestamps {
            created_at: now,
            updated_at: now,
            archived_at: None,
        },
        item_count: 2,
        materialization: CollectionMaterializationStatus::default(),
    }
}

fn load_ready_draft(state: &mut State, artifact_id: Uuid) {
    update_smart_shelf_ui(state, SmartShelfUiMessage::OpenComposer);
    update_smart_shelf_ui(
        state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::ProviderStatusLoaded(
            provider_ready(),
        )),
    );
    update_smart_shelf_ui(
        state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::DraftLoaded(
            draft_response(artifact_id),
        )),
    );
}

#[tokio::test(flavor = "current_thread")]
async fn entry_ctas_open_composer_without_replacing_exact_search() {
    let mut state = test_state();
    assert!(matches!(
        state.domains.search.state.presentation,
        SearchPresentation::Hidden
    ));

    let _ = view_header(&state);
    state.domains.ui.state.scope = Scope::Collections;
    state.tab_manager.set_active_tab(TabId::Collections);
    let _ = view_collections(&state);

    update_smart_shelf_ui(&mut state, SmartShelfUiMessage::OpenComposer);

    assert!(state.domains.ui.state.smart_shelf.open);
    assert!(matches!(
        state.domains.search.state.presentation,
        SearchPresentation::Hidden
    ));
    let summary = smart_shelf_composer_summary(&state);
    assert!(summary.template_labels.len() >= 3);
    assert!(summary.media_scope.contains("All libraries"));
    assert!(view_smart_shelf_surface(&state).is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn provider_unavailable_renders_fallback_and_retry_state() {
    let mut state = test_state();
    update_smart_shelf_ui(&mut state, SmartShelfUiMessage::OpenComposer);
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::ProviderStatusLoaded(
            provider_unavailable(),
        )),
    );

    let surface = &state.domains.ui.state.smart_shelf;
    assert_eq!(surface.reducer.phase, SmartShelfPhase::ProviderUnavailable);
    assert!(surface.provider_fallback.is_some());
    assert!(
        smart_shelf_composer_summary(&state)
            .provider_status
            .contains("Unavailable")
    );
    assert!(view_smart_shelf_surface(&state).is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn progress_ui_model_reports_phase_skeleton_cancel_retry_and_edit_prompt()
{
    let mut state = test_state();
    let run_id = Uuid::from_u128(20);
    update_smart_shelf_ui(&mut state, SmartShelfUiMessage::OpenComposer);
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::ProviderStatusLoaded(
            provider_ready(),
        )),
    );
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::PromptChanged(
            "rain shelf".to_string(),
        )),
    );
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::StartAccepted(
            start_response(run_id),
        )),
    );
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::RunProgressLoaded(
            running_status(run_id),
        )),
    );

    let summary = smart_shelf_progress_summary(
        &state.domains.ui.state.smart_shelf.reducer,
    );
    assert_eq!(summary.status, "Running");
    assert_eq!(summary.phase, "ranking grounded candidates");
    assert_eq!(summary.step, "Step 2/4");
    assert!(summary.skeleton_rows >= 3);
    assert!(summary.can_cancel);
    assert!(!summary.can_retry);
    assert!(!summary.can_edit_prompt);
    assert!(view_smart_shelf_surface(&state).is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn draft_ready_renders_ordered_cards_and_replacement_flow() {
    let mut state = test_state();
    let artifact_id = Uuid::from_u128(30);
    load_ready_draft(&mut state, artifact_id);

    let summary = smart_shelf_draft_review_summary(
        &state.domains.ui.state.smart_shelf.reducer,
    )
    .expect("draft summary");
    assert_eq!(summary.item_count, 2);
    assert_eq!(summary.alternate_count, 1);
    assert!(summary.can_save);

    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::ReplaceWithAlternate {
            target_media_id: movie(1),
            alternate_media_id: movie(3),
        }),
    );

    let draft = state
        .domains
        .ui
        .state
        .smart_shelf
        .reducer
        .draft
        .as_ref()
        .expect("draft");
    assert_eq!(draft.items[0].media_id, movie(3));
    assert_eq!(draft.items[0].replacement_of, Some(movie(1)));
    assert!(draft.dirty);
    assert_eq!(draft.replacements_count(), 1);
    assert!(view_smart_shelf_surface(&state).is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn save_confirmation_makes_private_scope_and_conflict_copy_explicit() {
    let mut state = test_state();
    let artifact_id = Uuid::from_u128(40);
    load_ready_draft(&mut state, artifact_id);

    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::SaveRequested),
    );

    let smart = &state.domains.ui.state.smart_shelf.reducer;
    assert_eq!(smart.save.status, SmartShelfSaveStatus::Confirming);
    let summary = smart_shelf_save_review_summary(smart).expect("save summary");
    assert_eq!(summary.title, "Rainy night smart shelf");
    assert!(summary.description.contains("Atmospheric"));
    assert!(summary.scope.contains("Accepted 2 items"));
    assert_eq!(summary.visibility, "Private manual collection");
    assert!(summary.conflict_help.contains("Duplicate media"));
    assert!(summary.conflict_help.contains("API conflicts"));
    assert!(view_smart_shelf_surface(&state).is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn save_error_surfaces_recoverable_conflict_actions() {
    let mut state = test_state();
    let artifact_id = Uuid::from_u128(50);
    load_ready_draft(&mut state, artifact_id);
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::SaveConfirmed),
    );

    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::SaveFailed(
            SmartShelfFailure::new(
                SmartShelfFailureCode::SmartShelf(
                    SmartShelfErrorCode::CollectionConflict,
                ),
                "Smart-shelf conflict: draft version changed",
                true,
            ),
        )),
    );

    let smart = &state.domains.ui.state.smart_shelf.reducer;
    assert_eq!(smart.phase, SmartShelfPhase::SaveConflict);
    let summary = smart_shelf_save_review_summary(smart).expect("save summary");
    assert_eq!(summary.status, "Needs recovery");
    assert!(summary.error.expect("error").contains("version"));
    let actions = smart
        .save
        .conflict
        .as_ref()
        .expect("conflict")
        .recovery_actions
        .iter()
        .map(|action| save_conflict_recovery_label(*action))
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec!["Reload draft", "Edit selection", "Retry save", "Discard"]
    );
    assert!(actions.contains(&save_conflict_recovery_label(
        SmartShelfSaveConflictRecovery::RetrySave
    )));
    assert!(view_smart_shelf_surface(&state).is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn successful_save_closes_surface_and_navigates_to_collection_detail() {
    let mut state = test_state();
    let artifact_id = Uuid::from_u128(60);
    let collection_id = CollectionId::from(Uuid::from_u128(61));
    load_ready_draft(&mut state, artifact_id);
    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::SaveConfirmed),
    );

    update_smart_shelf_ui(
        &mut state,
        SmartShelfUiMessage::Reducer(SmartShelfMessage::SaveSucceeded(
            SmartShelfSaveResponse {
                draft_artifact_id: artifact_id,
                collection_id: collection_id.clone(),
                collection: collection_summary(collection_id.clone()),
                item_count: 2,
                saved_at_epoch_seconds: Some(Utc::now().timestamp()),
            },
        )),
    );

    assert!(!state.domains.ui.state.smart_shelf.open);
    assert!(matches!(
        state.domains.ui.state.view,
        ferrex_player_ui::domains::ui::types::ViewState::CollectionDetail { collection_id: ref navigated }
            if navigated == &collection_id
    ));
    assert!(view_smart_shelf_surface(&state).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn collection_refresh_message_keeps_smart_shelf_state_independent() {
    let mut state = test_state();
    update_smart_shelf_ui(&mut state, SmartShelfUiMessage::OpenComposer);
    let before = state.domains.ui.state.smart_shelf.open;
    let _ = ferrex_player_ui::domains::ui::collections::update_collections_ui(
        &mut state,
        CollectionsMessage::Refresh,
    );
    assert_eq!(state.domains.ui.state.smart_shelf.open, before);
}

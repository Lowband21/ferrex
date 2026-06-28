use std::collections::HashSet;

use ferrex_player_app::infra::{
    api_types::*, services::api::ApiService, testing::stubs::TestApiService,
};
use uuid::Uuid;

fn fixed_uuid(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

fn media_id(value: u128) -> MediaID {
    MediaID::Movie(MovieID(fixed_uuid(value)))
}

fn smart_shelf_run_status(
    run_id: Uuid,
    status: IntelligenceRunStatus,
    step: u32,
    draft_artifact_ids: Vec<Uuid>,
) -> IntelligenceRunStatusResponse {
    IntelligenceRunStatusResponse {
        run_id,
        purpose: IntelligenceRunPurpose::Recommendation,
        status,
        terminal: matches!(
            status,
            IntelligenceRunStatus::Succeeded
                | IntelligenceRunStatus::Failed
                | IntelligenceRunStatus::Cancelled
        ),
        current_phase: Some(format!("mvp-step-{step}")),
        provider: Some("deterministic-fake-provider".into()),
        model: Some("fake-smart-shelf-model".into()),
        queued_at_epoch_seconds: Some(1),
        started_at_epoch_seconds: (step > 0).then_some(2),
        completed_at_epoch_seconds: matches!(
            status,
            IntelligenceRunStatus::Succeeded
        )
        .then_some(3),
        current_step: Some(step),
        max_steps: Some(3),
        draft_artifact_ids,
        output_summary: Some(IntelligenceSummary::new(
            "Deterministic MVP fixture produced a smart-shelf draft.",
        )),
        error: None,
    }
}

fn smart_shelf_draft(
    artifact_id: Uuid,
    run_id: Uuid,
    selected_media_id: MediaID,
    alternate_media_id: MediaID,
) -> SmartShelfDraftResponse {
    let selected_source = SmartShelfDraftSource {
        label: Some("Library metadata".into()),
        media_id: Some(selected_media_id),
        artifact_id: Some(artifact_id),
        field: Some("genres".into()),
        evidence: Some(IntelligenceSummary::new("Grounded selected item")),
    };
    let alternate_source = SmartShelfDraftSource {
        label: Some("Library metadata".into()),
        media_id: Some(alternate_media_id),
        artifact_id: Some(artifact_id),
        field: Some("overview".into()),
        evidence: Some(IntelligenceSummary::new("Grounded alternate item")),
    };
    let content = SmartShelfDraftContent {
        schema_version: SMART_SHELF_DRAFT_SCHEMA_VERSION,
        title: "Deterministic rainy-night shelf".into(),
        description: Some("Fake-provider MVP integration fixture".into()),
        interpreted_intent: Some("Grounded rainy-night recommendations".into()),
        requested_constraints: serde_json::json!({"fixture": "smart-shelf-mvp"}),
        items: vec![SmartShelfDraftItem {
            ordinal: 1,
            media_id: selected_media_id,
            title: Some("Aurora Transit".into()),
            subtitle: Some("Movie · deterministic fixture".into()),
            year: Some(2024),
            reason: Some("Grounded by deterministic library metadata".into()),
            sources: vec![selected_source],
            locked: false,
            replacement_of: None,
        }],
        alternates: vec![SmartShelfDraftAlternate {
            target_ordinal: Some(1),
            media_id: alternate_media_id,
            title: Some("Copper Harbor".into()),
            subtitle: Some("Alternate movie".into()),
            year: Some(2022),
            reason: Some("Grounded alternate for replacement QA".into()),
            sources: vec![alternate_source],
        }],
    };
    let validation = validate_smart_shelf_draft_items(
        &content.items,
        &HashSet::from([selected_media_id, alternate_media_id]),
    );

    SmartShelfDraftResponse {
        artifact_id,
        run_id: Some(run_id),
        owner_user_id: Some(fixed_uuid(0x65700000000000000000000000000100)),
        title: content.title.clone(),
        summary: Some(IntelligenceSummary::new(
            "A deterministic fake-provider draft",
        )),
        draft: Some(content),
        validation,
        saved_collection_id: None,
    }
}

#[tokio::test]
async fn smart_shelf_mvp_start_draft_save_opens_collection_detail_fixture() {
    let service = TestApiService::default();
    let run_id = fixed_uuid(0x65700000000000000000000000000200);
    let artifact_id = fixed_uuid(0x65700000000000000000000000000201);
    let collection_id =
        CollectionId::from(fixed_uuid(0x65700000000000000000000000000202));
    let selected_media_id = media_id(0x65700000000000000000000000000203);
    let alternate_media_id = media_id(0x65700000000000000000000000000204);

    service.queue_smart_shelf_start(SmartShelfStartResponse {
        run_id,
        status: IntelligenceRunStatus::Queued,
        provider: Some("deterministic-fake-provider".into()),
        model: Some("fake-smart-shelf-model".into()),
        queued_at_epoch_seconds: Some(1),
        draft_schema_version: SMART_SHELF_DRAFT_SCHEMA_VERSION,
    });
    service.set_intelligence_run_progress(
        run_id,
        vec![
            smart_shelf_run_status(
                run_id,
                IntelligenceRunStatus::Queued,
                0,
                Vec::new(),
            ),
            smart_shelf_run_status(
                run_id,
                IntelligenceRunStatus::Succeeded,
                3,
                vec![artifact_id],
            ),
        ],
    );
    service.upsert_smart_shelf_draft(smart_shelf_draft(
        artifact_id,
        run_id,
        selected_media_id,
        alternate_media_id,
    ));
    service.set_next_smart_shelf_save_collection_id(collection_id);

    let start = service
        .start_smart_shelf(SmartShelfStartRequest {
            prompt: "Grounded rainy-night picks".into(),
            library_id: None,
            media_kinds: vec![IntelligenceMediaKind::Movie],
            item_count: 6,
            template_id: Some("rainy-night".into()),
            locked_media_ids: Vec::new(),
            idempotency_key: Some("smart-shelf-mvp-fixture".into()),
            model: Some("fake-smart-shelf-model".into()),
            caps: IntelligenceCaps::default(),
            constraints: serde_json::json!({"fixture": "smart-shelf-mvp"}),
            metadata: serde_json::json!({"test": "start-draft-save-detail"}),
        })
        .await
        .expect("start smart shelf fixture");
    assert_eq!(start.run_id, run_id);

    let queued = service
        .fetch_intelligence_run_status(run_id)
        .await
        .expect("queued status");
    assert_eq!(queued.status, IntelligenceRunStatus::Queued);

    let succeeded = service
        .fetch_intelligence_run_status(run_id)
        .await
        .expect("succeeded status");
    assert_eq!(succeeded.status, IntelligenceRunStatus::Succeeded);
    assert_eq!(succeeded.draft_artifact_ids, vec![artifact_id]);

    let draft = service
        .fetch_smart_shelf_draft(artifact_id)
        .await
        .expect("draft fixture");
    assert!(draft.validation.valid);
    assert_eq!(
        draft
            .draft
            .as_ref()
            .expect("draft content")
            .alternates
            .len(),
        1
    );

    let saved = service
        .save_smart_shelf(artifact_id, SmartShelfSaveRequest::default())
        .await
        .expect("save smart shelf fixture");
    assert_eq!(saved.collection_id, collection_id);
    assert_eq!(saved.item_count, 1);

    let detail = service
        .get_collection_detail(
            collection_id,
            GetCollectionDetailRequest {
                include_rule: true,
                include_items_preview: true,
                include_shelf_placements: false,
            },
        )
        .await
        .expect("saved collection detail");
    assert_eq!(
        detail.collection.summary.provenance.generated_by.as_deref(),
        Some("smart_shelf")
    );
    assert_eq!(detail.collection.items_preview.len(), 1);
    assert_eq!(
        detail.collection.summary.title,
        "Deterministic rainy-night shelf"
    );
}

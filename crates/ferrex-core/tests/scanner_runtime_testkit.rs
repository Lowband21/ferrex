//! Scanner runtime testkit smoke coverage.

#[path = "support/scanner_runtime.rs"]
mod scanner_runtime;

use anyhow::Result;
use ferrex_core::domain::scan::actors::analyze::MediaAnalyzeActor;
use ferrex_core::domain::scan::actors::index::IndexerActor;
use ferrex_core::domain::scan::actors::metadata::{
    MetadataActor, MetadataCommand,
};
use ferrex_core::domain::scan::orchestration::context::FolderScanContext;
use ferrex_core::domain::scan::orchestration::events::{
    JobEventPayload, JobEventPublisher,
};
use ferrex_core::domain::scan::orchestration::job::{
    JobKind, JobPriority, ScanReason,
};
use ferrex_core::domain::scan::orchestration::runtime::InProcJobEventBus;
use ferrex_core::domain::scan::orchestration::series::SeriesMetadataProvider;

use scanner_runtime::{
    DEFAULT_WAIT_TIMEOUT, DeterministicSeriesProvider, TempSeriesLibrary,
    WaitConfig, index_upsert_job_from_ready, indexing_outcome_from_discovered,
    job_event_enqueued, media_analyze_job_from_discovered,
    metadata_enrich_job_from_analyzed, scan_event_indexed,
    scan_event_media_discovered, wait_for_job_event,
};

#[tokio::test]
async fn deterministic_series_fixture_builds_contexts_and_events() -> Result<()>
{
    let fixture = TempSeriesLibrary::builder().build()?;

    assert!(fixture.episode_file.exists());
    assert_eq!(
        fixture.relative_episode_path(),
        std::path::PathBuf::from("Show")
            .join("Season 1")
            .join("S01E01.mkv")
    );

    let config = fixture.library_actor_config(8);
    assert_eq!(config.library.id, fixture.library.id);
    assert_eq!(config.root_paths, vec![fixture.library_root.clone()]);

    assert!(matches!(
        fixture.series_context(),
        FolderScanContext::Series(_)
    ));
    assert!(matches!(
        fixture.season_context(),
        FolderScanContext::Season(_)
    ));

    let discovered = fixture.episode_discovered(ScanReason::BulkSeed);
    assert_eq!(discovered.path_norm, fixture.episode_file_norm);

    let media_event = scan_event_media_discovered(discovered.clone());
    assert!(matches!(
        media_event,
        ferrex_core::domain::scan::orchestration::events::ScanEvent::MediaFileDiscovered(_)
    ));

    let indexed_event =
        scan_event_indexed(indexing_outcome_from_discovered(&discovered));
    assert!(matches!(
        indexed_event,
        ferrex_core::domain::scan::orchestration::events::ScanEvent::Indexed(_)
    ));

    Ok(())
}

#[tokio::test]
async fn fake_pipeline_actors_forward_series_metadata_and_index_paths()
-> Result<()> {
    let fixture = TempSeriesLibrary::builder().build()?;
    let discovered = fixture.episode_discovered(ScanReason::BulkSeed);

    let analyzed = scanner_runtime::PassthroughAnalyzeActor
        .analyze(media_analyze_job_from_discovered(&discovered))
        .await?;
    let metadata_job =
        metadata_enrich_job_from_analyzed(&analyzed, ScanReason::BulkSeed);
    let ready = scanner_runtime::PassthroughMetadataActor
        .enrich(MetadataCommand {
            job: metadata_job,
            analyzed,
        })
        .await?;

    let indexer = scanner_runtime::RecordingIndexerActor::default();
    let outcome = indexer
        .index(ferrex_core::domain::scan::actors::index::IndexCommand {
            job: index_upsert_job_from_ready(&ready, &discovered.path_norm),
            ready,
        })
        .await?;
    assert_eq!(outcome.path_norm, discovered.path_norm);
    assert_eq!(indexer.outcomes().await.len(), 1);

    let provider = DeterministicSeriesProvider;
    let series_job = fixture.series_resolve_job(ScanReason::BulkSeed);
    let resolution = provider
        .resolve_series(
            series_job.library_id,
            &series_job.series_root_path,
            series_job.hint.as_ref().expect("fixture hint"),
            &series_job.folder_name,
        )
        .await?;
    assert_eq!(resolution.series_ref.title.as_deref(), Some("Show"));

    Ok(())
}

#[tokio::test]
async fn bounded_broadcast_wait_returns_matching_job_event() -> Result<()> {
    let fixture = TempSeriesLibrary::builder().build()?;
    let bus = InProcJobEventBus::new(8);
    let mut rx = bus.subscribe();
    let event = job_event_enqueued(
        fixture.library.id,
        JobKind::FolderScan,
        JobPriority::P1,
        &fixture.show_root_norm,
    );

    bus.publish(event).await?;
    let received = wait_for_job_event(
        &mut rx,
        WaitConfig::new(DEFAULT_WAIT_TIMEOUT, DEFAULT_WAIT_TIMEOUT),
        |event| {
            matches!(
                event.payload,
                JobEventPayload::Enqueued {
                    kind: JobKind::FolderScan,
                    ..
                }
            )
        },
    )
    .await?;

    assert!(matches!(
        received.payload,
        JobEventPayload::Enqueued {
            kind: JobKind::FolderScan,
            ..
        }
    ));

    Ok(())
}

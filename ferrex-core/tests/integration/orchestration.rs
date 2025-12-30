use sqlx::PgPool;
use sqlx::Row;
use chrono::Utc;
use tokio::task::JoinHandle;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ferrex_core::domain::scan::orchestration::persistence::PostgresQueueService;
use ferrex_core::domain::scan::orchestration::context::{
    ScanHierarchy, ScanNodeKind,
};
use ferrex_core::domain::scan::orchestration::job::{
    EnqueueRequest, FolderScanJob, IndexUpsertJob, JobKind, JobPayload,
    JobPriority, MediaAnalyzeJob, MediaFingerprint, MetadataEnrichJob,
    ScanReason,
};
use ferrex_core::domain::scan::orchestration::lease::{DequeueRequest, LeaseRenewal, QueueSelector};
use ferrex_core::domain::scan::orchestration::queue::QueueService;
use tokio::sync::Mutex;
use uuid::Uuid;

#[sqlx::test]
async fn e2e_bulk_enqueue_dequeue_complete(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone()).await.expect("svc init");

    // Seed one library
    let lib = seed_library(&pool).await;
    let lib_id = ferrex_core::LibraryID(lib);

    // Enqueue a bunch of jobs
    let n = 30usize;
    for i in 0..n {
        let fs = FolderScanJob {
            library_id: lib_id,
            folder_path_norm: format!("/bulk/path/{}", i),
            hierarchy: ScanHierarchy::default(),
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        };
        svc.enqueue(EnqueueRequest::new(JobPriority::P1, JobPayload::FolderScan(fs)))
            .await
            .expect("enqueue");
    }

    // Spawn a few workers that just complete work
    let workers = 4usize;
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for w in 0..workers {
        let svc_cl = svc.clone();
        let pool_cl = pool.clone();
        handles.push(tokio::spawn(async move {
            loop {
                let dq = DequeueRequest {
                    kind: JobKind::FolderScan,
                    worker_id: format!("e2e-w{}", w),
                    lease_ttl: chrono::Duration::seconds(10),
                    selector: None,
                };
                match svc_cl.dequeue(dq).await.expect("dequeue") {
                    Some(lease) => {
                        let _ = svc_cl.complete(lease.lease_id).await;
                    }
                    None => {
                        break;
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    // Verify all jobs completed
    let rows = sqlx::query(
        "SELECT state, COUNT(*)::bigint as cnt FROM orchestrator_jobs GROUP BY state"
    )
    .fetch_all(&pool)
    .await
    .expect("counts");

    let mut completed = 0i64; let mut others = 0i64;
    for r in rows {
        let state: String = r.get("state");
        let cnt: i64 = r.get::<Option<i64>, _>("cnt").unwrap_or(0);
        if state.as_str() == "completed" { completed = cnt; } else { others += cnt; }
    }
    assert_eq!(completed as usize, n);
    assert_eq!(others, 0);
}

#[sqlx::test]
async fn crash_simulation_expired_leases_recovered(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone()).await.expect("svc init");

    let lib = seed_library(&pool).await;
    let lib_id = ferrex_core::LibraryID(lib);

    // Enqueue and lease 5 jobs
    let m = 5usize;
    for i in 0..m {
        let fs = FolderScanJob {
            library_id: lib_id,
            folder_path_norm: format!("/crash/path/{}", i),
            hierarchy: ScanHierarchy::default(),
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        };
        svc.enqueue(EnqueueRequest::new(JobPriority::P1, JobPayload::FolderScan(fs)))
            .await
            .expect("enqueue");
    }

    // Lease all
    let mut leased_ids = Vec::new();
    for i in 0..m {
        let dq = DequeueRequest { kind: JobKind::FolderScan, worker_id: format!("cr-{}", i), lease_ttl: chrono::Duration::seconds(30), selector: None };
        let lease = svc.dequeue(dq).await.expect("dequeue ok").expect("lease");
        leased_ids.push(lease.job.id.0);
    }

    // Simulate crash by expiring all leases
    for id in leased_ids.iter() {
        sqlx::query("UPDATE orchestrator_jobs SET lease_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(*id)
            .execute(&pool)
            .await
            .expect("expire");
    }

    // Scanner recovers
    let resurrected = svc.scan_expired_leases().await.expect("scan");
    assert_eq!(resurrected as usize, m);

    // Now process to completion
    loop {
        let dq = DequeueRequest { kind: JobKind::FolderScan, worker_id: "cr-worker".into(), lease_ttl: chrono::Duration::seconds(10), selector: None };
        match svc.dequeue(dq).await.expect("dequeue") {
            Some(lease) => { svc.complete(lease.lease_id).await.expect("complete"); }
            None => break,
        }
    }

    let remaining: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*)::bigint FROM orchestrator_jobs WHERE state <> 'completed'")
        .fetch_one(&pool)
        .await
        .expect("remaining");
    assert_eq!(remaining, 0, "all jobs should be completed");
}

#[sqlx::test]
async fn selector_prefers_match_otherwise_fifo(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone()).await.expect("svc init");

    let lib_x = ferrex_core::LibraryID(seed_library(&pool).await);
    let lib_y = ferrex_core::LibraryID(seed_library(&pool).await);

    let job_a = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(FolderScanJob {
                library_id: lib_x,
                folder_path_norm: "/selector/a".into(),
                hierarchy: ScanHierarchy::default(),
                scan_reason: ScanReason::UserRequested,
                enqueue_time: Utc::now(),
                device_id: None,
            }),
        ))
        .await
        .expect("enqueue A");

    let job_b = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(FolderScanJob {
                library_id: lib_y,
                folder_path_norm: "/selector/b".into(),
                hierarchy: ScanHierarchy::default(),
                scan_reason: ScanReason::UserRequested,
                enqueue_time: Utc::now(),
                device_id: None,
            }),
        ))
        .await
        .expect("enqueue B");

    let job_c = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P0,
            JobPayload::FolderScan(FolderScanJob {
                library_id: lib_x,
                folder_path_norm: "/selector/c".into(),
                hierarchy: ScanHierarchy::default(),
                scan_reason: ScanReason::UserRequested,
                enqueue_time: Utc::now(),
                device_id: None,
            }),
        ))
        .await
        .expect("enqueue C");

    let select_hit = DequeueRequest {
        kind: JobKind::FolderScan,
        worker_id: "selector-hit".into(),
        lease_ttl: chrono::Duration::seconds(30),
        selector: Some(QueueSelector {
            library_id: lib_x,
            priority: JobPriority::P0,
        }),
    };
    let lease_hit = svc
        .dequeue(select_hit)
        .await
        .expect("dequeue hit")
        .expect("expected a matching job");
    assert_eq!(lease_hit.job.id.0, job_c.job_id.0, "selector should target job C");

    let ids = vec![job_a.job_id.0, job_b.job_id.0, job_c.job_id.0];
    let rows = sqlx::query!(
        r#"
        SELECT id, state
        FROM orchestrator_jobs
        WHERE id = ANY($1)
        "#,
        &ids
    )
    .fetch_all(&pool)
    .await
    .expect("states after selector hit");

    let mut states = HashMap::new();
    for row in rows {
        states.insert(row.id, row.state);
    }

    assert_eq!(states.get(&job_c.job_id.0).map(String::as_str), Some("leased"));
    assert_eq!(states.get(&job_a.job_id.0).map(String::as_str), Some("ready"));
    assert_eq!(states.get(&job_b.job_id.0).map(String::as_str), Some("ready"));

    let select_miss = DequeueRequest {
        kind: JobKind::FolderScan,
        worker_id: "selector-miss".into(),
        lease_ttl: chrono::Duration::seconds(30),
        selector: Some(QueueSelector {
            library_id: ferrex_core::LibraryID::new(),
            priority: JobPriority::P2,
        }),
    };

    let lease_fallback = svc
        .dequeue(select_miss)
        .await
        .expect("dequeue fallback")
        .expect("expected FIFO fallback");

    assert_eq!(lease_fallback.job.id.0, job_a.job_id.0, "fallback should return job A in FIFO order");

    let rows_after = sqlx::query!(
        r#"
        SELECT id, state
        FROM orchestrator_jobs
        WHERE id = ANY($1)
        "#,
        &ids
    )
    .fetch_all(&pool)
    .await
    .expect("states after fallback");

    let mut states_after = HashMap::new();
    for row in rows_after {
        states_after.insert(row.id, row.state);
    }

    assert_eq!(states_after.get(&job_a.job_id.0).map(String::as_str), Some("leased"));
    assert_eq!(states_after.get(&job_b.job_id.0).map(String::as_str), Some("ready"));
    assert_eq!(states_after.get(&job_c.job_id.0).map(String::as_str), Some("leased"));
}

#[sqlx::test]
async fn end_to_end_batch_mixed(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone()).await.expect("svc init");

    // Seed library
    let lib = seed_library(&pool).await;
    let lib_id = ferrex_core::LibraryID(lib);

    // Build 20 jobs (5 per kind), with 3 duplicates for merge coverage
    let mut enqueues = vec![];
    for i in 0..5 {
        let fs = FolderScanJob {
            library_id: lib_id,
            folder_path_norm: format!("/e2e/mixed/folder_{}", i),
            hierarchy: ScanHierarchy::default(),
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        };
        enqueues.push(EnqueueRequest::new(JobPriority::P1, JobPayload::FolderScan(fs)));
    }
    // Duplicate folder_1
    enqueues.push(EnqueueRequest::new(
        JobPriority::P1,
        JobPayload::FolderScan(FolderScanJob {
            library_id: lib_id,
            folder_path_norm: "/e2e/mixed/folder_1".into(),
            hierarchy: ScanHierarchy::default(),
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        }),
    ));

    for i in 0..5 {
        let fp = MediaFingerprint {
            device_id: Some("it-dev".into()),
            inode: Some(100 + i),
            size: 2048 + i as u64,
            mtime: 1_700_000_100 + i as i64,
            weak_hash: Some(format!("h-{i}")),
        };
        let ma = MediaAnalyzeJob {
            library_id: lib_id,
            path_norm: format!("/e2e/mixed/analyze_{i}.mkv"),
            fingerprint: fp,
            discovered_at: Utc::now(),
            media_id: MediaID::new(VideoMediaType::Movie),
            variant: VideoMediaType::Movie,
            hierarchy: ScanHierarchy::default(),
            node: ScanNodeKind::Unknown,
            scan_reason: ScanReason::BulkSeed,
        };
        enqueues.push(EnqueueRequest::new(JobPriority::P2, JobPayload::MediaAnalyze(ma)));
    }
    // Duplicate analyze_2 fingerprint
    let ma_dupe = MediaAnalyzeJob {
        library_id: lib_id,
        path_norm: "/e2e/mixed/analyze_2_dupe.mkv".into(),
        fingerprint: MediaFingerprint {
            device_id: Some("it-dev".into()),
            inode: Some(102),
            size: 2050,
            mtime: 1_700_000_102,
            weak_hash: Some("h-2".into()),
        },
        discovered_at: Utc::now(),
        media_id: MediaID::new(VideoMediaType::Movie),
        variant: VideoMediaType::Movie,
        hierarchy: ScanHierarchy::default(),
        node: ScanNodeKind::Unknown,
        scan_reason: ScanReason::BulkSeed,
    };
    enqueues.push(EnqueueRequest::new(JobPriority::P2, JobPayload::MediaAnalyze(ma_dupe)));

    for i in 0..5 {
        let md = MetadataEnrichJob {
            library_id: lib_id,
            media_id: MediaID::new(VideoMediaType::Movie),
            variant: VideoMediaType::Movie,
            hierarchy: ScanHierarchy::default(),
            node: ScanNodeKind::Unknown,
            path_norm: format!("/meta/cand-{i}"),
            fingerprint: MediaFingerprint::default(),
            scan_reason: ScanReason::BulkSeed,
        };
        enqueues.push(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::MetadataEnrich(md),
        ));
    }
    // Duplicate candidate
    let md_dupe = MetadataEnrichJob {
        library_id: lib_id,
        media_id: MediaID::new(VideoMediaType::Movie),
        variant: VideoMediaType::Movie,
        hierarchy: ScanHierarchy::default(),
        node: ScanNodeKind::Unknown,
        path_norm: "/meta/cand-4".into(),
        fingerprint: MediaFingerprint::default(),
        scan_reason: ScanReason::BulkSeed,
    };
    enqueues.push(EnqueueRequest::new(
        JobPriority::P0,
        JobPayload::MetadataEnrich(md_dupe),
    ));

    for i in 0..5 {
        let ix = IndexUpsertJob {
            library_id: lib_id,
            media_id: MediaID::new(VideoMediaType::Movie),
            variant: VideoMediaType::Movie,
            hierarchy: ScanHierarchy::default(),
            node: ScanNodeKind::Unknown,
            path_norm: format!("/e2e/mixed/index_{i}.json"),
            idempotency_key: format!("index:{}:/e2e/mixed/index_{i}.json", lib_id),
        };
        enqueues.push(EnqueueRequest::new(JobPriority::P3, JobPayload::IndexUpsert(ix)));
    }

    for req in enqueues.into_iter().take(20) {
        let _ = svc.enqueue(req).await.expect("enqueue");
    }

    // After 3 dupes, expect 17 rows
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orchestrator_jobs")
        .fetch_one(&pool)
        .await
        .expect("count jobs");
    assert_eq!(total, 17);

    // Worker pool: process jobs, mix complete/fail and dead-letter after 2 fails
    let attempts: Arc<Mutex<HashMap<Uuid, u32>>> = Arc::new(Mutex::new(HashMap::new()));
    let seen: Arc<Mutex<HashSet<Uuid>>> = Arc::new(Mutex::new(HashSet::new()));
    let done = Arc::new(tokio::sync::atomic::AtomicUsize::new(0));
    let kinds = [JobKind::FolderScan, JobKind::MediaAnalyze, JobKind::MetadataEnrich, JobKind::IndexUpsert];

    let mut handles = vec![];
    for w in 0..4 {
        let svc_w = svc.clone();
        let attempts_w = Arc::clone(&attempts);
        let seen_w = Arc::clone(&seen);
        let done_w = Arc::clone(&done);
        handles.push(tokio::spawn(async move {
            loop {
                if done_w.load(std::sync::atomic::Ordering::Relaxed) >= 17 { break; }
                let mut progressed = false;
                for kind in kinds {
                    let dq = DequeueRequest { kind, worker_id: format!("mix-w{w}"), lease_ttl: chrono::Duration::milliseconds(500), selector: None };
                    if let Ok(Some(lease)) = svc_w.dequeue(dq).await {
                        progressed = true;
                        {
                            let mut s = seen_w.lock().await;
                            assert!(s.insert(lease.lease_id.0), "duplicate lease seen");
                        }
                        let _ = svc_w.renew(LeaseRenewal { lease_id: lease.lease_id, worker_id: format!("mix-w{w}"), extend_by: chrono::Duration::milliseconds(200) }).await;
                        // Decide action by job id parity
                        let jid = lease.job.id.0;
                        let even = (u128::from_be_bytes(*jid.as_bytes()) & 1) == 0;
                        if even {
                            let _ = svc_w.complete(lease.lease_id).await;
                            done_w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            let mut map = attempts_w.lock().await;
                            let e = map.entry(jid).or_insert(0);
                            if *e < 2 { *e += 1; drop(map); let _ = svc_w.fail(lease.lease_id, true, Some("transient".into())).await; }
                            else { drop(map); let _ = svc_w.dead_letter(lease.lease_id, Some("exhausted".into())).await; done_w.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
                        }
                    }
                }
                if !progressed { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
            }
        }));
    }
    for h in handles { let _ = h.await; }

    let row = sqlx::query(
        "SELECT SUM(CASE WHEN state='completed' THEN 1 ELSE 0 END)::bigint as c, SUM(CASE WHEN state='dead_letter' THEN 1 ELSE 0 END)::bigint as d FROM orchestrator_jobs"
    )
    .fetch_one(&pool)
    .await
    .expect("agg");
    let completed: i64 = row.get::<Option<i64>, _>("c").unwrap_or(0);
    let dlq: i64 = row.get::<Option<i64>, _>("d").unwrap_or(0);
    assert_eq!(completed + dlq, 17, "all unique jobs should be terminal");

    svc.metrics_snapshot().await.expect("metrics snapshot ok");
}

#[ignore = "measurement only; does not assert behavior"]
#[sqlx::test]
async fn bench_stub_latency_logging(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone()).await.expect("svc init");
    let lib = seed_library(&pool).await;
    let lib_id = ferrex_core::LibraryID(lib);

    // Enqueue a small batch and measure dequeue->complete latency
    let n = 10;
    for i in 0..n {
        let fs = FolderScanJob { library_id: lib_id, folder_path_norm: format!("/bench/{}", i), hierarchy: ScanHierarchy::default(), scan_reason: ScanReason::UserRequested, enqueue_time: Utc::now(), device_id: None };
        svc.enqueue(EnqueueRequest::new(JobPriority::P1, JobPayload::FolderScan(fs))).await.expect("enqueue");
    }

    let mut latencies = Vec::new();
    loop {
        let dq = DequeueRequest { kind: JobKind::FolderScan, worker_id: "bench".into(), lease_ttl: chrono::Duration::seconds(5), selector: None };
        match svc.dequeue(dq).await.expect("dequeue") {
            Some(lease) => {
                let start = Utc::now();
                svc.complete(lease.lease_id).await.expect("complete");
                let ms = (Utc::now() - start).num_milliseconds();
                latencies.push(ms);
            }
            None => break,
        }
    }
    latencies.sort();
    let p95 = if latencies.is_empty() { 0 } else { latencies[(latencies.len() as f64 * 0.95) as usize.min(latencies.len()-1)] };
    tracing::info!("bench_stub p95_ms={} avg_ms={}", p95, if latencies.is_empty() {0.0} else {latencies.iter().sum::<i64>() as f64 / latencies.len() as f64});
}

async fn seed_library(pool: &PgPool) -> uuid::Uuid {
    let id = uuid::Uuid::now_v7();
    let name = format!("Integration Test Library {}", id);
    sqlx::query(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW(), NOW())"
    )
    .bind(id)
    .bind(name)
    .bind(vec!["/integration/test"])
    .bind("movies")
    .execute(pool)
    .await
    .expect("seed library");
    id
}

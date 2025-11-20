use ferrex_core::error::MediaError;
use ferrex_core::orchestration::QueueService;
use ferrex_core::types::LibraryID;
use sqlx::PgPool;
use sqlx::Row;

use ferrex_core::orchestration::persistence::PostgresQueueService;

#[sqlx::test]
async fn postgres_queue_service_initializes(pool: PgPool) {
    // Should succeed without panic and return a service instance
    let _svc = PostgresQueueService::new(pool)
        .await
        .expect("queue service should connect and validate schema");
}

use chrono::Utc;
use ferrex_core::orchestration::job::{
    EnqueueRequest, FolderScanJob, JobPayload, JobPriority, ScanReason,
};
use ferrex_core::orchestration::lease::DequeueRequest;
use uuid::Uuid;

async fn seed_library(pool: &PgPool) -> Uuid {
    let library_id = Uuid::now_v7();
    let unique_name = format!("Test Library - Queue {}", library_id);
    sqlx::query(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW(), NOW())"
    )
    .bind(library_id)
    .bind(unique_name)
    .bind(vec!["/test/queue"])
    .bind("movies")
    .execute(pool)
    .await
    .expect("seed library");
    library_id
}

#[sqlx::test]
async fn enqueue_unique_job_creates_ready_row(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library referenced by job
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // Build a simple FolderScan job
    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/movies".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let req = EnqueueRequest::new(JobPriority::P1, JobPayload::FolderScan(fs));

    let handle = svc.enqueue(req.clone()).await.expect("enqueue ok");
    assert!(handle.accepted, "should be a new job");

    // Verify row exists and is ready
    let row = sqlx::query("SELECT state FROM orchestrator_jobs WHERE id = $1")
        .bind(handle.job_id.0)
        .fetch_one(&pool)
        .await
        .expect("fetch job row");
    let state: String = row.get("state");
    assert_eq!(state.as_str(), "ready");
}

#[sqlx::test]
async fn enqueue_duplicate_returns_merged_handle(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // Build identical FolderScan job twice
    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/movies".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let req1 = EnqueueRequest::new(JobPriority::P2, JobPayload::FolderScan(fs.clone()));
    let req2 = EnqueueRequest::new(JobPriority::P2, JobPayload::FolderScan(fs));

    let h1 = svc.enqueue(req1.clone()).await.expect("enqueue 1 ok");
    assert!(h1.accepted);

    let h2 = svc.enqueue(req2.clone()).await.expect("enqueue 2 ok");
    assert!(!h2.accepted, "second enqueue should merge");
    assert_eq!(h2.job_id.0, h1.job_id.0, "merged should point to same id");

    // Ensure only one active row with this dedupe_key
    let dedupe = req2.dedupe_key().to_string();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM orchestrator_jobs WHERE dedupe_key = $1 AND state IN ('ready','deferred','leased')"
    )
    .bind(dedupe)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(count, 1, "only one active row should exist");
}

#[sqlx::test]
async fn dequeue_leases_one_and_leaves_others_ready(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // Enqueue two jobs
    let fs1 = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/a".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let fs2 = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/b".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let _ = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(fs1),
        ))
        .await
        .expect("enqueue fs1");
    let _ = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(fs2),
        ))
        .await
        .expect("enqueue fs2");

    // Dequeue one
    let dq = DequeueRequest {
        kind: ferrex_core::orchestration::job::JobKind::FolderScan,
        worker_id: "w1".to_string(),
        lease_ttl: chrono::Duration::seconds(15),
        selector: None,
    };
    let lease = svc.dequeue(dq).await.expect("dequeue ok");
    assert!(lease.is_some());

    // Verify one leased and at least one ready remains
    let rows =
        sqlx::query("SELECT state, COUNT(*)::bigint as cnt FROM orchestrator_jobs GROUP BY state")
            .fetch_all(&pool)
            .await
            .expect("state counts");

    let mut leased = 0i64;
    let mut ready = 0i64;
    for r in rows {
        let state: String = r.get("state");
        let cnt: i64 = r.get::<Option<i64>, _>("cnt").unwrap_or(0);
        match state.as_str() {
            "leased" => leased = cnt,
            "ready" => ready = cnt,
            _ => {}
        }
    }
    assert_eq!(leased, 1, "one job should be leased");
    assert!(ready >= 1, "at least one job should remain ready");
}

#[sqlx::test]
async fn concurrent_dequeues_do_not_double_lease(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library and enqueue exactly one job
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);
    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/only".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let _ = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(fs),
        ))
        .await
        .expect("enqueue fs");

    // Two concurrent dequeues
    let dq1 = DequeueRequest {
        kind: ferrex_core::orchestration::job::JobKind::FolderScan,
        worker_id: "wA".to_string(),
        lease_ttl: chrono::Duration::seconds(10),
        selector: None,
    };
    let dq2 = DequeueRequest {
        kind: ferrex_core::orchestration::job::JobKind::FolderScan,
        worker_id: "wB".to_string(),
        lease_ttl: chrono::Duration::seconds(10),
        selector: None,
    };

    let svc1 = svc.clone();
    let svc2 = svc.clone();

    let t1 = tokio::spawn(async move { svc1.dequeue(dq1).await });
    let t2 = tokio::spawn(async move { svc2.dequeue(dq2).await });

    let (r1, r2) = tokio::join!(t1, t2);

    let l1 = r1.expect("task1 join").expect("dequeue1 result");
    let l2 = r2.expect("task2 join").expect("dequeue2 result");

    // Exactly one should be Some
    let some_count = l1.is_some() as i32 + l2.is_some() as i32;
    assert_eq!(some_count, 1, "only one lease should be granted");
}

#[sqlx::test]
async fn enqueue_merge_elevates_priority_and_makes_available(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // P2 enqueue
    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/tv".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let req_p2 = EnqueueRequest::new(JobPriority::P2, JobPayload::FolderScan(fs.clone()));
    let h1 = svc.enqueue(req_p2.clone()).await.expect("enqueue p2 ok");
    assert!(h1.accepted);

    // Defer the existing job artificially
    sqlx::query!(
        "UPDATE orchestrator_jobs SET state='deferred', available_at = NOW() + INTERVAL '60 seconds' WHERE id = $1",
        h1.job_id.0
    )
    .execute(&pool)
    .await
    .expect("defer job");

    // Enqueue P0 (higher priority) with same dedupe
    let req_p0 = EnqueueRequest::new(JobPriority::P0, JobPayload::FolderScan(fs));
    let h2 = svc.enqueue(req_p0.clone()).await.expect("enqueue p0 ok");
    assert!(!h2.accepted, "second enqueue should merge");
    assert_eq!(h2.job_id.0, h1.job_id.0, "merge points to same job");

    // Verify priority elevated and available_at not in the future
    let row = sqlx::query("SELECT priority, available_at FROM orchestrator_jobs WHERE id = $1")
        .bind(h1.job_id.0)
        .fetch_one(&pool)
        .await
        .expect("fetch updated job");

    let pri: i16 = row.get::<i16, _>("priority");
    assert_eq!(pri, 0, "priority should be elevated to 0");

    let now = Utc::now();
    let available: chrono::DateTime<chrono::Utc> = row.get("available_at");
    assert!(
        available <= now,
        "available_at should be bumped to NOW (not in future)"
    );
}

#[sqlx::test]
async fn enqueue_duplicate_no_elevation_keeps_priority(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // Enqueue P0 first
    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/no-elev".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let req_p0 = EnqueueRequest::new(JobPriority::P0, JobPayload::FolderScan(fs.clone()));
    let h1 = svc.enqueue(req_p0.clone()).await.expect("enqueue p0 ok");
    assert!(h1.accepted);

    let before = sqlx::query("SELECT priority, updated_at FROM orchestrator_jobs WHERE id = $1")
        .bind(h1.job_id.0)
        .fetch_one(&pool)
        .await
        .expect("fetch before");
    let before_pri: i16 = before.get::<i16, _>("priority");
    let before_updated: chrono::DateTime<chrono::Utc> = before.get("updated_at");

    // Enqueue lower priority duplicate (P2)
    let req_p2 = EnqueueRequest::new(JobPriority::P2, JobPayload::FolderScan(fs));
    let h2 = svc.enqueue(req_p2.clone()).await.expect("enqueue p2 ok");
    assert!(!h2.accepted);

    let after = sqlx::query("SELECT priority, updated_at FROM orchestrator_jobs WHERE id = $1")
        .bind(h1.job_id.0)
        .fetch_one(&pool)
        .await
        .expect("fetch after");
    let after_pri: i16 = after.get::<i16, _>("priority");

    assert_eq!(before_pri, 0, "initial priority should be 0");
    assert_eq!(after_pri, 0, "priority should remain 0 (no elevation)");
    let after_updated: chrono::DateTime<chrono::Utc> = after.get("updated_at");
    assert_eq!(
        after_updated, before_updated,
        "no update should have occurred (rows_affected=0)"
    );
}

#[sqlx::test]
async fn lease_renewal_success_and_post_expiry_failure(pool: PgPool) {
    use ferrex_core::orchestration::lease::{DequeueRequest, LeaseRenewal};

    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    // Seed library
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // Enqueue single job
    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/renew".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let _h = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(fs),
        ))
        .await
        .expect("enqueue");

    // Dequeue with very short TTL
    let dq = DequeueRequest {
        kind: ferrex_core::orchestration::job::JobKind::FolderScan,
        worker_id: "renew-worker".into(),
        lease_ttl: chrono::Duration::milliseconds(500),
        selector: None,
    };
    let lease = svc
        .dequeue(dq)
        .await
        .expect("dequeue ok")
        .expect("lease some");

    // Initial renewals should be 0
    assert_eq!(lease.renewals, 0);
    let first_expiry = lease.expires_at;

    // Renew early by 300ms
    let renewal = LeaseRenewal {
        lease_id: lease.lease_id,
        worker_id: "renew-worker".into(),
        extend_by: chrono::Duration::milliseconds(300),
    };
    let renewed = svc.renew(renewal).await.expect("renew ok");

    assert!(
        renewed.expires_at > first_expiry,
        "expiry should be extended"
    );
    assert_eq!(renewed.renewals, 1, "renewals should increment locally");

    // Force expiry deterministically (avoid flakiness due to sleeps)
    sqlx::query(
        "UPDATE orchestrator_jobs SET lease_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(renewed.job.id.0)
    .execute(&pool)
    .await
    .expect("force lease expiry");

    // Attempt another renewal should fail with NotFound
    let renewal2 = ferrex_core::orchestration::lease::LeaseRenewal {
        lease_id: renewed.lease_id,
        worker_id: "renew-worker".into(),
        extend_by: chrono::Duration::milliseconds(200),
    };
    let err = svc
        .renew(renewal2)
        .await
        .expect_err("renew after expiry must fail");
    match err {
        MediaError::NotFound(_) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[sqlx::test]
async fn job_completion_sets_terminal_state(pool: PgPool) {
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/complete".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let _ = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(fs),
        ))
        .await
        .expect("enqueue");

    let dq = DequeueRequest {
        kind: ferrex_core::orchestration::job::JobKind::FolderScan,
        worker_id: "worker".into(),
        lease_ttl: chrono::Duration::seconds(5),
        selector: None,
    };
    let lease = svc.dequeue(dq).await.expect("dequeue ok").expect("lease");

    svc.complete(lease.lease_id).await.expect("complete ok");

    let row = sqlx::query("SELECT state, lease_owner, lease_id, lease_expires_at FROM orchestrator_jobs WHERE id = $1")
        .bind(lease.job.id.0)
        .fetch_one(&pool)
        .await
        .expect("fetch");

    let state: String = row.get("state");
    let lease_owner: Option<String> = row.get("lease_owner");
    let lease_id: Option<uuid::Uuid> = row.get("lease_id");
    let lease_expires_at: Option<chrono::DateTime<chrono::Utc>> = row.get("lease_expires_at");

    assert_eq!(state.as_str(), "completed");
    assert!(lease_owner.is_none());
    assert!(lease_id.is_none());
    assert!(lease_expires_at.is_none());
}

#[sqlx::test]
async fn job_failure_backoff_and_dead_letter(pool: PgPool) {
    use ferrex_core::orchestration::lease::DequeueRequest;

    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");
    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    let fs = FolderScanJob {
        library_id: lib_id,
        folder_path_norm: "/media/fail".to_string(),
        parent_context: None,
        scan_reason: ScanReason::UserRequested,
        enqueue_time: Utc::now(),
        device_id: None,
    };
    let _ = svc
        .enqueue(EnqueueRequest::new(
            JobPriority::P1,
            JobPayload::FolderScan(fs),
        ))
        .await
        .expect("enqueue");

    // Fail 5 times with retryable=true and check backoff ranges
    for attempt in 1..=5 {
        let dq = DequeueRequest {
            kind: ferrex_core::orchestration::job::JobKind::FolderScan,
            worker_id: format!("w-{}", attempt),
            lease_ttl: chrono::Duration::milliseconds(100),
            selector: None,
        };
        let lease = svc.dequeue(dq).await.expect("dequeue ok").expect("lease");

        // Fail with retryable true
        svc.fail(lease.lease_id, true, Some(format!("err-{}", attempt)))
            .await
            .expect("fail ok");

        // Verify state ready and available_at backoff within approx range
        let row = sqlx::query(
            "SELECT state, attempts, available_at FROM orchestrator_jobs WHERE id = $1",
        )
        .bind(lease.job.id.0)
        .fetch_one(&pool)
        .await
        .expect("fetch");
        let state: String = row.get("state");
        assert_eq!(state.as_str(), "ready");
        let atts: i32 = row.get::<i32, _>("attempts");
        assert_eq!(atts, attempt as i32, "attempts should increment");

        let base = (1i64 << atts.min(27)) as f64; // 2^attempts, large guard
        let base_capped = base.min(120.0);
        let now = Utc::now();
        let available_at: chrono::DateTime<chrono::Utc> = row.get("available_at");
        let diff = (available_at - now).num_milliseconds();
        // diff can be slightly negative if test timing; clamp
        let diff_ms = diff.max(0) as f64;
        let lower = base_capped * 0.75 * 1000.0;
        let upper = base_capped * 1.25 * 1000.0 + 300.0; // allow scheduling jitter/time to run
        assert!(
            diff_ms >= lower && diff_ms <= upper,
            "backoff {}ms not in [{}, {}] for attempts {}",
            diff_ms,
            lower,
            upper,
            atts
        );

        // Force availability for next iteration to avoid sleeping
        sqlx::query("UPDATE orchestrator_jobs SET available_at = NOW() WHERE id = $1")
            .bind(lease.job.id.0)
            .execute(&pool)
            .await
            .expect("force availability");
    }

    // Drive attempts to dead_letter at > max attempts
    for attempt in 6..=11 {
        let dq = DequeueRequest {
            kind: ferrex_core::orchestration::job::JobKind::FolderScan,
            worker_id: format!("w-{}", attempt),
            lease_ttl: chrono::Duration::milliseconds(50),
            selector: None,
        };
        let lease = svc.dequeue(dq).await.expect("dequeue ok");
        if let Some(lease) = lease {
            let retryable = true;
            svc.fail(lease.lease_id, retryable, Some("boom".into()))
                .await
                .expect("fail ok");

            // Force availability in case next loop needs to dequeue again (until DLQ)
            sqlx::query("UPDATE orchestrator_jobs SET available_at = NOW() WHERE id = $1")
                .bind(lease.job.id.0)
                .execute(&pool)
                .await
                .expect("force availability");
        }
    }

    let row = sqlx::query(
        "SELECT state, attempts FROM orchestrator_jobs ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch final");
    let state: String = row.get("state");
    assert_eq!(state.as_str(), "dead_letter");
}

#[sqlx::test]
async fn scan_expired_leases_transitions_ready_and_dlq(pool: PgPool) {
    use ferrex_core::orchestration::lease::DequeueRequest;
    let svc = PostgresQueueService::new(pool.clone())
        .await
        .expect("svc init");

    let lib = seed_library(&pool).await;
    let lib_id = LibraryID(lib);

    // Enqueue and lease 3 jobs
    for i in 0..3 {
        let fs = FolderScanJob {
            library_id: lib_id,
            folder_path_norm: format!("/media/exp-{}", i),
            parent_context: None,
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        };
        let _ = svc
            .enqueue(EnqueueRequest::new(
                JobPriority::P1,
                JobPayload::FolderScan(fs),
            ))
            .await
            .expect("enqueue");
    }

    let mut leases = vec![];
    for i in 0..3 {
        let dq = DequeueRequest {
            kind: ferrex_core::orchestration::job::JobKind::FolderScan,
            worker_id: format!("sx-{}", i),
            lease_ttl: chrono::Duration::seconds(30),
            selector: None,
        };
        let l = svc.dequeue(dq).await.expect("dequeue ok").expect("lease");
        leases.push(l);
    }

    // Expire all 3; set attempts to create both ready and DLQ outcomes
    // First two: attempts=0 (will become ready), third: attempts=10 (will DLQ)
    for (idx, l) in leases.iter().enumerate() {
        if idx < 2 {
            sqlx::query(
                "UPDATE orchestrator_jobs SET lease_expires_at = NOW() - INTERVAL '5 seconds', attempts = 0 WHERE id = $1"
            )
            .bind(l.job.id.0)
            .execute(&pool)
            .await
            .expect("expire ready");
        } else {
            sqlx::query(
                "UPDATE orchestrator_jobs SET lease_expires_at = NOW() - INTERVAL '5 seconds', attempts = 10 WHERE id = $1"
            )
            .bind(l.job.id.0)
            .execute(&pool)
            .await
            .expect("expire dlq");
        }
    }

    // Run scanner
    let resurrected = svc.scan_expired_leases().await.expect("scan ok");
    assert_eq!(resurrected, 2, "two jobs should be resurrected to ready");

    // Verify counts
    let rows =
        sqlx::query("SELECT state, COUNT(*)::bigint as cnt FROM orchestrator_jobs GROUP BY state")
            .fetch_all(&pool)
            .await
            .expect("counts");
    let mut ready = 0i64;
    let mut dlq = 0i64;
    let mut leased = 0i64;
    for r in rows {
        let state: String = r.get("state");
        let cnt: i64 = r.get::<Option<i64>, _>("cnt").unwrap_or(0);
        match state.as_str() {
            "ready" => ready = cnt,
            "dead_letter" => dlq = cnt,
            "leased" => leased = cnt,
            _ => {}
        }
    }
    assert_eq!(ready, 2);
    assert_eq!(dlq, 1);
    assert_eq!(leased, 0);

    // Metrics snapshot should succeed (logs only)
    svc.metrics_snapshot().await.expect("metrics snapshot ok");
}

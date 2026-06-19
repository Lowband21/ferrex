-- Allow manifest root/partition scans to be scheduled through the durable orchestrator queue.
ALTER TABLE orchestrator_jobs
    DROP CONSTRAINT IF EXISTS orchestrator_jobs_kind_check;

ALTER TABLE orchestrator_jobs
    ADD CONSTRAINT orchestrator_jobs_kind_check CHECK (kind >= 0 AND kind <= 7);

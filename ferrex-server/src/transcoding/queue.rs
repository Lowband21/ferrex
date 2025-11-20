use super::job::{JobMessage, JobPriority, JobResponse, TranscodingJob, TranscodingStatus};
use anyhow::Result;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, info};

/// Priority queue item wrapper
#[derive(Debug, Clone)]
struct PriorityJob {
    job: TranscodingJob,
    sequence: u64, // For stable sorting of same priority
}

impl PartialEq for PriorityJob {
    fn eq(&self, other: &Self) -> bool {
        self.job.priority == other.job.priority && self.sequence == other.sequence
    }
}

impl Eq for PriorityJob {}

impl PartialOrd for PriorityJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first, then earlier sequence
        match self.job.priority.cmp(&other.job.priority) {
            std::cmp::Ordering::Equal => other.sequence.cmp(&self.sequence),
            ordering => ordering,
        }
    }
}

/// Job queue manager with priority support
pub struct JobQueue {
    /// Pending jobs in priority order
    pending: Arc<RwLock<BinaryHeap<PriorityJob>>>,
    /// All jobs by ID
    pub(super) jobs: Arc<RwLock<HashMap<String, TranscodingJob>>>,
    /// Channel for job submissions
    pub(super) submit_tx: mpsc::Sender<(TranscodingJob, oneshot::Sender<Result<String>>)>,
    /// Channel for job requests from workers
    pub(super) job_request_tx: mpsc::Sender<oneshot::Sender<Option<TranscodingJob>>>,
    /// Channel for commands (exposed for workers)
    pub(super) command_tx: mpsc::Sender<(JobMessage, oneshot::Sender<JobResponse>)>,
    /// Sequence counter for stable sorting
    sequence: Arc<RwLock<u64>>,
    /// Maximum queue size
    max_queue_size: usize,
}

impl JobQueue {
    pub fn new(max_queue_size: usize) -> (Self, JobQueueHandle) {
        let (submit_tx, submit_rx) = mpsc::channel(100);
        let (job_request_tx, job_request_rx) = mpsc::channel(100);
        let (command_tx, command_rx) = mpsc::channel(100);

        let queue = Self {
            pending: Arc::new(RwLock::new(BinaryHeap::new())),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            submit_tx: submit_tx.clone(),
            job_request_tx: job_request_tx.clone(),
            command_tx: command_tx.clone(),
            sequence: Arc::new(RwLock::new(0)),
            max_queue_size,
        };

        let handle = JobQueueHandle {
            submit_tx,
            command_tx,
        };

        // Spawn queue manager task
        let queue_clone = queue.clone();
        tokio::spawn(async move {
            queue_clone
                .run(submit_rx, job_request_rx, command_rx)
                .await;
        });

        (queue, handle)
    }

    /// Main queue processing loop
    async fn run(
        self,
        mut submit_rx: mpsc::Receiver<(TranscodingJob, oneshot::Sender<Result<String>>)>,
        mut job_request_rx: mpsc::Receiver<oneshot::Sender<Option<TranscodingJob>>>,
        mut command_rx: mpsc::Receiver<(JobMessage, oneshot::Sender<JobResponse>)>,
    ) {
        info!("Job queue manager started");

        loop {
            tokio::select! {
                // Handle job submissions
                Some((job, response_tx)) = submit_rx.recv() => {
                    let result = self.submit_job(job).await;
                    let _ = response_tx.send(result);
                }

                // Handle job requests from workers
                Some(response_tx) = job_request_rx.recv() => {
                    let job = self.get_next_job().await;
                    let _ = response_tx.send(job);
                }

                // Handle commands
                Some((command, response_tx)) = command_rx.recv() => {
                    let response = self.handle_command(command).await;
                    let _ = response_tx.send(response);
                }

                else => break,
            }
        }

        info!("Job queue manager stopped");
    }

    /// Submit a new job to the queue
    async fn submit_job(&self, mut job: TranscodingJob) -> Result<String> {
        // Check queue size
        {
            let pending = self.pending.read().await;
            if pending.len() >= self.max_queue_size {
                return Err(anyhow::anyhow!("Queue is full"));
            }
        }

        // Update job status
        job.status = TranscodingStatus::Queued;
        let job_id = job.id.clone();

        // Get next sequence number
        let sequence = {
            let mut seq = self.sequence.write().await;
            let current = *seq;
            *seq += 1;
            current
        };

        // Add to pending queue
        {
            let mut pending = self.pending.write().await;
            pending.push(PriorityJob {
                job: job.clone(),
                sequence,
            });
        }

        // Add to jobs map
        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(job_id.clone(), job);
        }

        debug!("Job {} submitted to queue", job_id);
        Ok(job_id)
    }

    /// Get next job for processing
    async fn get_next_job(&self) -> Option<TranscodingJob> {
        let mut pending = self.pending.write().await;
        
        if let Some(priority_job) = pending.pop() {
            let mut job = priority_job.job;
            job.status = TranscodingStatus::Processing { progress: 0.0 };
            
            // Update job in map
            let mut jobs = self.jobs.write().await;
            if let Some(stored_job) = jobs.get_mut(&job.id) {
                stored_job.status = job.status.clone();
            }
            
            debug!("Job {} dequeued for processing", job.id);
            Some(job)
        } else {
            None
        }
    }

    /// Handle commands
    async fn handle_command(&self, command: JobMessage) -> JobResponse {
        match command {
            JobMessage::Cancel(job_id) => {
                self.cancel_job(&job_id).await
            }
            JobMessage::GetStatus(job_id) => {
                let jobs = self.jobs.read().await;
                JobResponse::Status(jobs.get(&job_id).cloned())
            }
            JobMessage::UpdatePriority { job_id, priority } => {
                self.update_priority(&job_id, priority).await
            }
            JobMessage::UpdateStatus { job_id, status } => {
                self.update_job_status(&job_id, status).await
            }
            _ => JobResponse::Error("Unsupported command".to_string()),
        }
    }

    /// Cancel a job
    async fn cancel_job(&self, job_id: &str) -> JobResponse {
        let mut jobs = self.jobs.write().await;
        
        if let Some(job) = jobs.get_mut(job_id) {
            match &job.status {
                TranscodingStatus::Pending | TranscodingStatus::Queued => {
                    job.status = TranscodingStatus::Cancelled;
                    
                    // Remove from pending queue
                    let mut pending = self.pending.write().await;
                    let new_heap: BinaryHeap<_> = pending
                        .drain()
                        .filter(|pj| pj.job.id != job_id)
                        .collect();
                    *pending = new_heap;
                    
                    JobResponse::Cancelled
                }
                _ => JobResponse::Error("Job cannot be cancelled in current state".to_string()),
            }
        } else {
            JobResponse::Error("Job not found".to_string())
        }
    }

    /// Update job priority
    async fn update_priority(&self, job_id: &str, new_priority: JobPriority) -> JobResponse {
        let mut jobs = self.jobs.write().await;
        
        if let Some(job) = jobs.get_mut(job_id) {
            match &job.status {
                TranscodingStatus::Pending | TranscodingStatus::Queued => {
                    job.priority = new_priority;
                    
                    // Rebuild priority queue
                    let mut pending = self.pending.write().await;
                    let mut items: Vec<_> = pending.drain().collect();
                    
                    for item in &mut items {
                        if item.job.id == job_id {
                            item.job.priority = new_priority;
                        }
                    }
                    
                    *pending = items.into_iter().collect();
                    
                    JobResponse::Status(Some(job.clone()))
                }
                _ => JobResponse::Error("Cannot update priority of running job".to_string()),
            }
        } else {
            JobResponse::Error("Job not found".to_string())
        }
    }

    /// Update job status
    async fn update_job_status(&self, job_id: &str, status: TranscodingStatus) -> JobResponse {
        let mut jobs = self.jobs.write().await;
        
        if let Some(job) = jobs.get_mut(job_id) {
            job.update_status(status);
            JobResponse::Status(Some(job.clone()))
        } else {
            JobResponse::Error("Job not found".to_string())
        }
    }

    /// Get queue statistics
    pub async fn get_stats(&self) -> QueueStats {
        let pending_count = self.pending.read().await.len();
        let jobs = self.jobs.read().await;
        
        let mut stats = QueueStats::default();
        stats.pending = pending_count;
        
        for job in jobs.values() {
            match &job.status {
                TranscodingStatus::Processing { .. } => stats.processing += 1,
                TranscodingStatus::Completed => stats.completed += 1,
                TranscodingStatus::Failed { .. } => stats.failed += 1,
                TranscodingStatus::Cancelled => stats.cancelled += 1,
                _ => {}
            }
        }
        
        stats.total = jobs.len();
        stats
    }
}

impl Clone for JobQueue {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
            jobs: self.jobs.clone(),
            submit_tx: self.submit_tx.clone(),
            job_request_tx: self.job_request_tx.clone(),
            command_tx: self.command_tx.clone(),
            sequence: self.sequence.clone(),
            max_queue_size: self.max_queue_size,
        }
    }
}

/// Handle for interacting with the job queue
#[derive(Clone)]
pub struct JobQueueHandle {
    submit_tx: mpsc::Sender<(TranscodingJob, oneshot::Sender<Result<String>>)>,
    command_tx: mpsc::Sender<(JobMessage, oneshot::Sender<JobResponse>)>,
}

impl JobQueueHandle {
    /// Submit a job to the queue
    pub async fn submit(&self, job: TranscodingJob) -> Result<String> {
        let (response_tx, response_rx) = oneshot::channel();
        self.submit_tx
            .send((job, response_tx))
            .await
            .map_err(|_| anyhow::anyhow!("Queue channel closed"))?;
        
        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("Failed to get response"))?
    }

    /// Send a command to the queue
    pub async fn send_command(&self, command: JobMessage) -> Result<JobResponse> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send((command, response_tx))
            .await
            .map_err(|_| anyhow::anyhow!("Command channel closed"))?;
        
        Ok(response_rx
            .await
            .map_err(|_| anyhow::anyhow!("Failed to get response"))?)
    }
}

/// Queue statistics
#[derive(Debug, Default, Clone)]
pub struct QueueStats {
    pub total: usize,
    pub pending: usize,
    pub processing: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
}
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Unique identifier for a supervised background or render job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct JobId(pub u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job-{}", self.0)
    }
}

/// Kind of asynchronous work tracked by `BackgroundJobSupervisor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    DraftPreviewRender,
    FinalExportRender,
    AgentDirectorTask,
}

/// Lifecycle state of a supervised job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running { progress_pct: u8 },
    Paused { progress_pct: u8 },
    Completed,
    Cancelled,
}

impl JobState {
    /// Returns `true` if the job is neither `Completed` nor `Cancelled`.
    #[inline]
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// Policy applied to active jobs when the desktop UI window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowClosePolicy {
    /// Keep queued/running jobs alive in headless background execution and mark them detached.
    DetachAndContinueBackground,
    /// Gracefully pause queued/running jobs at a resumable checkpoint.
    PauseGracefully,
    /// Deterministically cancel all queued, running, and paused jobs.
    CancelAll,
}

/// Tracked metadata and state for a single background job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: JobId,
    pub kind: JobKind,
    pub label: String,
    pub project_revision_id: u64,
    pub state: JobState,
    pub detached_to_background: bool,
}

/// Deterministic summary produced by `BackgroundJobSupervisor::on_window_close`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowCloseReport {
    pub policy: WindowClosePolicy,
    pub detached_jobs: Vec<JobId>,
    pub paused_jobs: Vec<JobId>,
    pub cancelled_jobs: Vec<JobId>,
    pub untouched_terminal_jobs: Vec<JobId>,
}

/// Manages active render and agent jobs across UI window open/close transitions so
/// closing the desktop window never silently corrupts or orphans work.
#[derive(Debug, Clone, Default)]
pub struct BackgroundJobSupervisor {
    next_job_id: u64,
    window_open: bool,
    jobs: BTreeMap<JobId, BackgroundJob>,
}

impl BackgroundJobSupervisor {
    /// Creates a new supervisor with the desktop window initially open.
    pub fn new() -> Self {
        Self {
            next_job_id: 1,
            window_open: true,
            jobs: BTreeMap::new(),
        }
    }

    /// Returns whether the desktop window is currently attached/open.
    #[inline]
    pub fn is_window_open(&self) -> bool {
        self.window_open
    }

    /// Reattaches the desktop window and clears detached markers on active jobs.
    pub fn reattach_window(&mut self) {
        self.window_open = true;
        for job in self.jobs.values_mut() {
            if job.state.is_active() {
                job.detached_to_background = false;
            }
        }
    }

    /// Enqueues a new job bound to `project_revision_id` and returns its `JobId`.
    pub fn enqueue(
        &mut self,
        kind: JobKind,
        label: impl Into<String>,
        project_revision_id: u64,
    ) -> JobId {
        let id = JobId(self.next_job_id);
        self.next_job_id += 1;
        let job = BackgroundJob {
            id,
            kind,
            label: label.into(),
            project_revision_id,
            state: JobState::Queued,
            detached_to_background: !self.window_open,
        };
        self.jobs.insert(id, job);
        id
    }

    /// Transitions a `Queued` (or `Paused`) job into `Running`.
    pub fn start(&mut self, id: JobId) -> Result<(), AppError> {
        let job = self.jobs.get_mut(&id).ok_or(AppError::JobNotFound(id))?;
        match job.state {
            JobState::Queued => {
                job.state = JobState::Running { progress_pct: 0 };
                Ok(())
            }
            JobState::Paused { progress_pct } => {
                job.state = JobState::Running { progress_pct };
                Ok(())
            }
            other => Err(AppError::InvalidJobState {
                job_id: id,
                message: format!("cannot start job from state {other:?}"),
            }),
        }
    }

    /// Updates progress (`0..=100`) on a `Running` job.
    pub fn update_progress(&mut self, id: JobId, progress_pct: u8) -> Result<(), AppError> {
        let job = self.jobs.get_mut(&id).ok_or(AppError::JobNotFound(id))?;
        match &mut job.state {
            JobState::Running { progress_pct: pct } => {
                *pct = progress_pct.min(100);
                Ok(())
            }
            other => Err(AppError::InvalidJobState {
                job_id: id,
                message: format!("cannot update progress in state {other:?}"),
            }),
        }
    }

    /// Marks a `Queued` or `Running` job as `Completed`.
    pub fn complete(&mut self, id: JobId) -> Result<(), AppError> {
        let job = self.jobs.get_mut(&id).ok_or(AppError::JobNotFound(id))?;
        match job.state {
            JobState::Queued | JobState::Running { .. } => {
                job.state = JobState::Completed;
                Ok(())
            }
            other => Err(AppError::InvalidJobState {
                job_id: id,
                message: format!("cannot complete job from state {other:?}"),
            }),
        }
    }

    /// Cancels an active (`Queued`, `Running`, or `Paused`) job.
    pub fn cancel(&mut self, id: JobId) -> Result<(), AppError> {
        let job = self.jobs.get_mut(&id).ok_or(AppError::JobNotFound(id))?;
        if !job.state.is_active() {
            return Err(AppError::InvalidJobState {
                job_id: id,
                message: format!("cannot cancel terminal job in state {:?}", job.state),
            });
        }
        job.state = JobState::Cancelled;
        Ok(())
    }

    /// Applies `policy` deterministically to all tracked jobs when the desktop UI window closes.
    pub fn on_window_close(&mut self, policy: WindowClosePolicy) -> WindowCloseReport {
        self.window_open = false;
        let mut report = WindowCloseReport {
            policy,
            detached_jobs: Vec::new(),
            paused_jobs: Vec::new(),
            cancelled_jobs: Vec::new(),
            untouched_terminal_jobs: Vec::new(),
        };

        for (id, job) in &mut self.jobs {
            if !job.state.is_active() {
                report.untouched_terminal_jobs.push(*id);
                continue;
            }

            match policy {
                WindowClosePolicy::DetachAndContinueBackground => {
                    job.detached_to_background = true;
                    report.detached_jobs.push(*id);
                }
                WindowClosePolicy::PauseGracefully => {
                    let pct = match job.state {
                        JobState::Queued => 0,
                        JobState::Running { progress_pct } | JobState::Paused { progress_pct } => {
                            progress_pct
                        }
                        JobState::Completed | JobState::Cancelled => 0,
                    };
                    job.state = JobState::Paused { progress_pct: pct };
                    report.paused_jobs.push(*id);
                }
                WindowClosePolicy::CancelAll => {
                    job.state = JobState::Cancelled;
                    job.detached_to_background = false;
                    report.cancelled_jobs.push(*id);
                }
            }
        }

        report
    }

    /// Looks up a job by `JobId`.
    pub fn job(&self, id: JobId) -> Option<&BackgroundJob> {
        self.jobs.get(&id)
    }

    /// Returns all tracked jobs in deterministic `JobId` order.
    pub fn jobs(&self) -> impl Iterator<Item = &BackgroundJob> {
        self.jobs.values()
    }

    /// Returns all currently active (`Queued`, `Running`, or `Paused`) jobs.
    pub fn active_jobs(&self) -> Vec<&BackgroundJob> {
        self.jobs.values().filter(|j| j.state.is_active()).collect()
    }

    /// Returns all jobs associated with `project_revision_id`.
    pub fn jobs_for_revision(&self, project_revision_id: u64) -> Vec<&BackgroundJob> {
        self.jobs
            .values()
            .filter(|j| j.project_revision_id == project_revision_id)
            .collect()
    }
}

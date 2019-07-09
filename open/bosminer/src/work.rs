pub mod engine;
pub mod hub;
pub mod solver;

pub use hub::{Hub, JobSender, JobSolutionReceiver};
pub use solver::{Generator, SolutionSender, Solver};

use crate::hal::BitcoinJob;

use crate::misc::LOGGER;
use slog::info;

use tokio::sync::watch;
use tokio_async_await::stream::StreamExt as StreamExtForWatchBroadcast;

use std::sync::Arc;

type WrappedJob = Option<Arc<dyn BitcoinJob>>;
type JobChannelReceiver = watch::Receiver<WrappedJob>;
type JobChannelSender = watch::Sender<WrappedJob>;

struct JobQueue {
    job_broadcast_rx: JobChannelReceiver,
    current_job: Option<Arc<dyn BitcoinJob>>,
    finished: bool,
}

impl JobQueue {
    pub fn new(job_broadcast_rx: JobChannelReceiver) -> Self {
        Self {
            job_broadcast_rx,
            current_job: None,
            finished: true,
        }
    }

    /// Returns current job from which the new work is generated
    /// When the current job has been replaced with a new one
    /// then it is indicated in the second return value
    pub async fn determine_current_job(&mut self) -> (Arc<dyn BitcoinJob>, bool) {
        // look at latest broadcasted job
        match self.job_broadcast_rx.get_ref().as_ref() {
            // no job has been broadcasted yet, wait
            None => (),
            // check if we are working on anything
            Some(latest_job) => match self.current_job {
                // we aren't, so work on the latest job
                None => return (latest_job.clone(), true),
                Some(ref current_job) => {
                    // is our current job different from latest?
                    if !Arc::ptr_eq(current_job, latest_job) {
                        // something new has been broadcasted, work on that
                        return (latest_job.clone(), true);
                    }
                    // if we haven't finished it, continue working on it
                    if !self.finished {
                        return (current_job.clone(), false);
                    }
                    // otherwise just wait for more work
                }
            },
        }
        // loop until we receive a job
        loop {
            let new_job = await!(self.job_broadcast_rx.next())
                .expect("job reception failed")
                .expect("job stream ended");
            if let Some(new_job) = new_job {
                return (new_job, true);
            }
        }
    }

    pub async fn get_job(&mut self) -> (Arc<dyn BitcoinJob>, bool) {
        let (job, is_new) = await!(self.determine_current_job());
        if is_new {
            self.current_job = Some(job.clone())
        }
        self.finished = false;
        (job, is_new)
    }

    /// Clears the current job when the whole address space is exhausted
    /// After this method has been called, the get_job starts blocking until
    /// the new job is delivered
    pub fn finish_current_job(&mut self) {
        info!(LOGGER, "--- finishing current job ---");
        self.finished = true;
    }
}

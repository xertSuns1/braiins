use crate::hal::{self, BitcoinJob};

use tokio::prelude::*;

use futures::channel::mpsc;
use futures::stream::StreamExt;

use std::sync::{Arc, Mutex as StdMutex};

use bitcoin_hashes::{sha256, Hash, HashEngine};
use byteorder::{ByteOrder, LittleEndian};

pub mod dummy;

/// A registry of solutions
#[allow(dead_code)]
struct SolutionRegistry {
    /// Unique solutions
    solutions: std::vec::Vec<hal::UniqueMiningWorkSolution>,
}

#[allow(dead_code)]
impl SolutionRegistry {
    fn new() -> Self {
        Self {
            solutions: std::vec::Vec::new(),
        }
    }
}

pub struct WorkHub(WorkGenerator, WorkSolutionSender);

/// This trait represents common API for work solvers to get work and
/// submit solutions
impl WorkHub {
    /// Hardware-facing API
    pub async fn generate_work(&mut self) -> Option<hal::MiningWork> {
        await!(self.0.generate())
    }

    /// Hardware-facing API
    pub fn send_solution(&self, solution: hal::UniqueMiningWorkSolution) {
        self.1.send(solution);
    }

    pub fn split(self) -> (WorkGenerator, WorkSolutionSender) {
        (self.0, self.1)
    }

    /// Construct new WorkHub and associated queue to send work through
    /// This is runner/orchestrator/pump-facing function
    pub fn new() -> (Self, JobSolver) {
        let job_channel = Arc::new(StdMutex::new(None));
        let (job_event_tx, job_event_rx) = mpsc::channel(1);
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
        (
            Self(
                WorkGenerator::new(job_event_rx, job_channel.clone()),
                WorkSolutionSender(solution_queue_tx),
            ),
            JobSolver(
                JobSender::new(job_event_tx, job_channel),
                JobSolutionReceiver(solution_queue_rx),
            ),
        )
    }
}

pub struct NewJobEvent;

type JobChannel = Arc<StdMutex<Option<Arc<dyn BitcoinJob>>>>;

struct JobQueue {
    event: mpsc::Receiver<NewJobEvent>,
    channel: JobChannel,
    current: Option<Arc<dyn BitcoinJob>>,
}

impl JobQueue {
    pub fn new(event: mpsc::Receiver<NewJobEvent>, channel: JobChannel) -> Self {
        Self {
            event,
            channel,
            current: None,
        }
    }

    /// Returns current job from which the new work is generated
    /// When the current job has been replaced with a new one
    /// then it is indicated in the second return value
    pub async fn get_job(&mut self) -> (Arc<dyn BitcoinJob>, bool) {
        let mut new_job = self.current.is_none();

        if new_job {
            // wait for event which signals delivery of a new job
            await!(self.event.next());
        }

        // the job queue have to now contain a new job
        let job_channel_top = self
            .channel
            .lock()
            .expect("cannot lock queue for receiving new job")
            .as_ref()
            .expect("job queue is empty")
            .clone();

        if !new_job {
            // check job queue top differs from current job
            new_job = !Arc::ptr_eq(self.current.as_ref().unwrap(), &job_channel_top);
        }
        if new_job {
            // update current job with the latest one
            self.current = Some(job_channel_top);
        }
        // return reference to the job and flag with a new job indication
        (self.current.as_ref().unwrap().clone(), new_job)
    }

    /// Clears the current job when the whole address space is exhausted
    /// After this method has been called, the get_job starts blocking until
    /// the new job is delivered
    pub fn finish_current_job(&mut self) {
        // atomically remove current job from job queue and local reference
        let mut job_channel_top = self
            .channel
            .lock()
            .expect("cannot lock queue for receiving new job");
        job_channel_top.take();
        self.current.take();
    }
}

pub struct WorkGenerator {
    job_queue: JobQueue,
    midstates: usize,
    next_version: u16,
    base_version: u32,
}

impl WorkGenerator {
    pub fn new(job_event: mpsc::Receiver<NewJobEvent>, job_channel: JobChannel) -> Self {
        Self {
            job_queue: JobQueue::new(job_event, job_channel),
            midstates: 1,
            next_version: 0,
            base_version: 0,
        }
    }

    /// Roll new versions for Bitcoin header for all midstates
    /// It finishes (clears) the current job if it determines then no new version
    /// cannot be generated
    fn next_versions(&mut self, job: &Arc<dyn BitcoinJob>, new_job: bool) -> Vec<u32> {
        const MASK: u32 = 0x1fffe000;
        const SHIFT: u32 = 13;

        let version_start;
        if new_job {
            version_start = 0;
            self.next_version = self.midstates as u16;
            self.base_version = job.version() & !MASK;
        } else {
            version_start = self.next_version;
            if let Some(next_version) = self.next_version.checked_add(self.midstates as u16) {
                self.next_version = next_version;
            } else {
                self.job_queue.finish_current_job();
                self.next_version = 0;
            }
        };

        let mut versions = Vec::with_capacity(self.midstates);
        for version in version_start..self.next_version {
            versions.push(self.base_version | ((version as u32) << SHIFT));
        }
        versions
    }

    /// Returns new work generated from the current job
    pub async fn generate(&mut self) -> Option<hal::MiningWork> {
        let (job, new_job) = await!(self.job_queue.get_job());

        let time = job.time();
        let versions = self.next_versions(&job, new_job);
        let mut midstates = Vec::with_capacity(versions.len());

        let mut engine = sha256::Hash::engine();
        let buffer = &mut [0u8; 64];

        buffer[4..36].copy_from_slice(&job.previous_hash().into_inner());
        buffer[36..64].copy_from_slice(&job.merkle_root().into_inner()[..32 - 4]);

        for version in versions {
            LittleEndian::write_u32(&mut buffer[0..4], version);
            engine.input(buffer);
            midstates.push(hal::Midstate {
                version,
                state: engine.midstate(),
            })
        }

        Some(hal::MiningWork {
            job,
            midstates,
            ntime: time,
        })
    }
}

#[derive(Clone)]
pub struct WorkSolutionSender(mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>);

impl WorkSolutionSender {
    pub fn send(&self, solution: hal::UniqueMiningWorkSolution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}

pub struct JobSolver(JobSender, JobSolutionReceiver);

impl JobSolver {
    pub fn send_job(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        self.0.send(job)
    }

    pub async fn receive_solution(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        await!(self.1.receive())
    }

    pub fn split(self) -> (JobSender, JobSolutionReceiver) {
        (self.0, self.1)
    }
}

#[derive(Clone)]
pub struct JobSender {
    job_event: mpsc::Sender<NewJobEvent>,
    job_channel: JobChannel,
}

impl JobSender {
    pub fn new(job_event: mpsc::Sender<NewJobEvent>, job_channel: JobChannel) -> Self {
        Self {
            job_event,
            job_channel,
        }
    }

    pub fn change_target(&mut self, target: uint::U256) {

    }

    pub fn send(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        let old_job = self
            .job_channel
            .lock()
            .expect("cannot lock queue for sending new job")
            .replace(job);
        if old_job.is_none() {
            self.job_event
                .try_send(NewJobEvent)
                .expect("cannot notify about new job");
        }
    }
}

pub struct JobSolutionReceiver(mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>);

impl JobSolutionReceiver {
    pub async fn receive(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        // TODO: compare with target difficulty
        await!(self.0.next())
    }
}

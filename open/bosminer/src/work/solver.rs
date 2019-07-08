use super::*;
use crate::hal;

use futures::channel::mpsc;

use std::sync::Arc;

use bitcoin_hashes::{sha256, Hash, HashEngine};
use byteorder::{ByteOrder, LittleEndian};

/// Workhub sources jobs from `job_queue` and uses `work_generator` to convert them to
/// actual `MiningWork` suitable for processing (solving) by actual mining backend
pub struct Solver {
    /// Work generator for converting jobs to MiningWork
    work_generator: Generator,
    solution_sender: SolutionSender,
}

impl Solver {
    /// Hardware-facing API
    pub async fn generate_work(&mut self) -> Option<hal::MiningWork> {
        await!(self.work_generator.generate())
    }

    /// Hardware-facing API
    pub fn send_solution(&self, solution: hal::UniqueMiningWorkSolution) {
        self.solution_sender.send(solution);
    }

    pub fn split(self) -> (Generator, SolutionSender) {
        (self.work_generator, self.solution_sender)
    }

    /// Construct new WorkHub and associated queue to send work through
    /// This is runner/orchestrator/pump-facing function
    pub fn new(
        work_generator: Generator,
        solution_queue_tx: mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>,
    ) -> Self {
        Self {
            work_generator,
            solution_sender: SolutionSender(solution_queue_tx),
        }
    }
}

/// Generates `MiningWork` by rolling the version field of the block header
pub struct Generator {
    job_queue: JobQueue,
    /// Number of midstates that each generated work covers
    midstates: usize,
    /// Starting value of the rolled part of the version (before BIP320 shift)
    next_version: u16,
    /// Base Bitcoin block header version with BIP320 bits cleared
    base_version: u32,
}

impl Generator {
    pub fn new(job_channel: JobChannelReceiver) -> Self {
        Self {
            job_queue: JobQueue::new(job_channel),
            midstates: 1,
            next_version: 0,
            base_version: 0,
        }
    }

    /// Roll new versions for the block header for all midstates
    /// Return None If the rolled version space is exhausted. The version range can be
    /// reset by specifying `new_job`
    fn next_versions(&mut self, job: &Arc<dyn BitcoinJob>, new_job: bool) -> Vec<u32> {
        const MASK: u32 = 0x1fffe000;
        const SHIFT: u32 = 13;

        // Allocate the range for all midstates as per the BIP320 rolled 16 bits
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

        // Convert the allocated range to a list of versions as per BIP320
        let mut versions = Vec::with_capacity(self.midstates);
        for version in version_start..self.next_version {
            versions.push(self.base_version | ((version as u32) << SHIFT));
        }
        versions
    }

    /// Produces `MiningWork` for the specified job. Each job contains a number of midstates as
    /// that matches the configuration of this `Generator`.
    /// `new_job` indicates that version rolling can be reset
    fn get_work(&mut self, job: Arc<dyn BitcoinJob>, versions: Vec<u32>) -> hal::MiningWork {
        let time = job.time();
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

        hal::MiningWork {
            job,
            midstates,
            ntime: time,
        }
    }

    /// Returns new work generated from the current job
    pub async fn generate(&mut self) -> Option<hal::MiningWork> {
        let (job, new_job) = await!(self.job_queue.get_job());

        let versions = self.next_versions(&job, new_job);
        Some(self.get_work(job, versions))
    }
}

/// This struct is to be passed to the underlying mining backend. It allows submission of
/// `UniqueMiningWorkSolution`
#[derive(Clone)]
pub struct SolutionSender(mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>);

impl SolutionSender {
    pub fn send(&self, solution: hal::UniqueMiningWorkSolution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils;

    #[test]
    fn test_block_midstate() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let version = block.version();
            let (_, job_broadcast_rx) = watch::channel(None);
            let job_queue = JobQueue {
                job_broadcast_rx,
                current_job: None,
                finished: false,
            };
            let mut generator = Generator {
                job_queue,
                midstates: 1,
                next_version: 0,
                base_version: version,
            };

            let work = generator.get_work(Arc::new(*block), vec![version]);

            assert_eq!(block.midstate, work.midstates[0].state);
        }
    }
}

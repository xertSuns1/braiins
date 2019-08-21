//! Basic components for building WorkEngine broadcasting infrastructure and to send WorkEngines
//! to the actual work solving (mining) backends

pub mod engine;
mod hub;
mod solver;

use crate::hal;

pub use hub::{Hub, JobSender, JobSolutionReceiver, JobSolver};
pub use solver::{Generator, SolutionSender, Solver};

use futures::channel::mpsc;
use tokio::prelude::*;
use tokio::sync::watch;

use std::sync::Arc;

/// Shared work engine type
pub type DynWorkEngine = Arc<dyn hal::WorkEngine>;

/// Builds a WorkEngine broadcasting channel. The broadcast channel requires an initial value. We
/// use the empty work engine that signals 'exhausted' state all the time.
/// You can optionally pass a channel `reschedule_sender` that will be used to return all exhausted
/// engines. This way you can track what engines are "done".
pub fn engine_channel(
    reschedule_sender: Option<mpsc::UnboundedSender<DynWorkEngine>>,
) -> (EngineSender, EngineReceiver) {
    let work_engine: DynWorkEngine = Arc::new(engine::ExhaustedWork);
    let (sender, receiver) = watch::channel(work_engine);
    (
        EngineSender::new(sender),
        EngineReceiver::new(receiver, reschedule_sender),
    )
}

/// Sender is responsible for broadcasting a new WorkEngine to all mining
/// backends
pub struct EngineSender {
    inner: watch::Sender<DynWorkEngine>,
}

impl EngineSender {
    fn new(watch_sender: watch::Sender<DynWorkEngine>) -> Self {
        Self {
            inner: watch_sender,
        }
    }

    pub fn broadcast(&mut self, engine: DynWorkEngine) {
        self.inner
            .broadcast(engine)
            .expect("cannot broadcast work engine")
    }
}

/// Manages incoming WorkEngines (see get_engine() for details)
#[derive(Clone)]
pub struct EngineReceiver {
    /// Broadcast channel that is used to distribute current `WorkEngine`
    watch_receiver: watch::Receiver<DynWorkEngine>,
    /// A channel that is (if present) used to send back exhausted engines
    /// to be "recycled" or just so that engine sender is notified that all work
    /// has been generated from them
    reschedule_sender: Option<mpsc::UnboundedSender<DynWorkEngine>>,
}

impl EngineReceiver {
    fn new(
        watch_receiver: watch::Receiver<DynWorkEngine>,
        reschedule_sender: Option<mpsc::UnboundedSender<DynWorkEngine>>,
    ) -> Self {
        Self {
            watch_receiver,
            reschedule_sender,
        }
    }

    /// Provides the most recent WorkEngine as long as the engine is able to provide any work.
    /// Otherwise, it sleeps and waits for a new
    pub async fn get_engine(&mut self) -> Option<DynWorkEngine> {
        let mut engine = self.watch_receiver.get_ref().clone();
        loop {
            if !engine.is_exhausted() {
                // return only work engine which can generate some work
                return Some(engine);
            }
            match await!(self.watch_receiver.next()) {
                // end of stream
                None => return None,
                // new work engine received
                Some(value) => engine = value.expect("cannot receive work engine"),
            }
        }
    }

    /// This function should be called just when last entry has been taken out of engine
    pub fn reschedule(&self) {
        let engine = self.watch_receiver.get_ref().clone();

        // If `reschedule_sender` is present, send the current engine back to it
        if let Some(reschedule_sender) = self.reschedule_sender.as_ref() {
            reschedule_sender
                .unbounded_send(engine)
                .expect("reschedule notify send failed");
        }
    }
}

//! Basic components for building WorkEngine broadcasting infrastructure and to send WorkEngines
//! to the actual work solving (mining) backends

pub mod engine;
mod hub;
mod solver;

use crate::hal;

pub use hub::{Hub, JobSender, JobSolutionReceiver, JobSolver};
pub use solver::{Generator, SolutionSender, Solver};

use crate::misc::LOGGER;
use slog::info;

use tokio::prelude::*;
use tokio::sync::watch;

use std::sync::Arc;

/// Shared work engine type
type DynWorkEngine = Arc<dyn hal::WorkEngine>;

/// Builds a WorkEngine broadcasting channel. The broadcast channel requires an initial value. We
/// use the empty work engine that signals 'exhausted' state all the time.
pub fn engine_channel() -> (EngineSender, EngineReceiver) {
    let (sender, receiver) = watch::channel(Arc::new(engine::ExhaustedWork) as DynWorkEngine);
    (EngineSender::new(sender), EngineReceiver::new(receiver))
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
    inner: watch::Receiver<DynWorkEngine>,
}

impl EngineReceiver {
    fn new(watch_receiver: watch::Receiver<DynWorkEngine>) -> Self {
        Self {
            inner: watch_receiver,
        }
    }

    /// Provides the most recent WorkEngine as long as the engine is able to provide any work.
    /// Otherwise, it sleeps and waits for a new
    pub async fn get_engine(&mut self) -> Option<DynWorkEngine> {
        let mut engine = self.inner.get_ref().clone();
        loop {
            if !engine.is_exhausted() {
                // return only work engine which can generate some work
                return Some(engine);
            }
            match await!(self.inner.next()) {
                // end of stream
                None => return None,
                // new work engine received
                Some(value) => engine = value.expect("cannot receive work engine"),
            }
        }
    }

    pub fn reschedule(&self) {
        // TODO: wakeup WorkHub to reschedule new work
        info!(LOGGER, "--- finishing current job ---");
    }
}

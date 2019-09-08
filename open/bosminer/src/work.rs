// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! Basic components for building WorkEngine broadcasting infrastructure and to send WorkEngines
//! to the actual work solving (mining) backends

pub mod engine;
mod hub;
mod solver;

use crate::job;

pub use hub::{Hub, JobSender, JobSolutionReceiver, JobSolver};
pub use solver::{Generator, SolutionSender, Solver};

use futures::channel::mpsc;
use tokio::prelude::*;
use tokio::sync::watch;

use std::fmt::Debug;
use std::sync::Arc;

pub enum LoopState<T> {
    /// Mining work is exhausted
    Exhausted,
    /// Returning latest work (subsequent call will return Exhausted)
    Break(T),
    /// Mining work generation will continue
    Continue(T),
}

impl<T> LoopState<T> {
    pub fn unwrap(self) -> T {
        match self {
            LoopState::Break(val) => val,
            LoopState::Continue(val) => val,
            _ => panic!("called `LoopState::unwrap()` on a `None` value"),
        }
    }

    #[inline]
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> LoopState<U> {
        use LoopState::{Break, Continue, Exhausted};

        match self {
            Exhausted => Exhausted,
            Break(x) => Break(f(x)),
            Continue(x) => Continue(f(x)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Midstate {
    /// Version field used for calculating the midstate
    pub version: u32,
    /// Internal state of SHA256 after processing the first chunk (32 bytes)
    pub state: ii_bitcoin::Midstate,
}

/// Describes actual mining work for assignment to a hashing hardware.
/// Starting with merkle_root_tail the data goes to chunk2 of SHA256.
/// TODO: add ntime limit for supporting hardware that can do nTime rolling on its own
#[derive(Clone, Debug)]
pub struct Assignment {
    /// Bitcoin job shared with initial network protocol and work solution
    // TODO: remove pub after moving `UniqueMiningWorkSolution` to this module
    pub job: Arc<dyn job::Bitcoin>,
    /// Multiple midstates can be generated for each work
    pub midstates: Vec<Midstate>,
    /// Start value for nTime, hardware may roll nTime further
    pub ntime: u32,
}

impl Assignment {
    pub fn new(job: Arc<dyn job::Bitcoin>, midstates: Vec<Midstate>, ntime: u32) -> Self {
        Self {
            job,
            midstates,
            ntime,
        }
    }

    /// Return merkle root tail
    pub fn merkle_root_tail(&self) -> u32 {
        self.job.merkle_root_tail()
    }

    /// Return current target (nBits)
    #[inline]
    pub fn bits(&self) -> u32 {
        self.job.bits()
    }
}

pub trait Engine: Debug + Send + Sync {
    fn is_exhausted(&self) -> bool;

    fn next_work(&self) -> LoopState<Assignment>;
}

/// Shared work engine type
pub type DynEngine = Arc<dyn Engine>;

/// Builds a WorkEngine broadcasting channel. The broadcast channel requires an initial value. We
/// use the empty work engine that signals 'exhausted' state all the time.
/// You can optionally pass a channel `reschedule_sender` that will be used to return all exhausted
/// engines. This way you can track what engines are "done".
pub fn engine_channel(
    reschedule_sender: Option<mpsc::UnboundedSender<DynEngine>>,
) -> (EngineSender, EngineReceiver) {
    let work_engine: DynEngine = Arc::new(engine::ExhaustedWork);
    let (sender, receiver) = watch::channel(work_engine);
    (
        EngineSender::new(sender),
        EngineReceiver::new(receiver, reschedule_sender),
    )
}

/// Sender is responsible for broadcasting a new WorkEngine to all mining
/// backends
pub struct EngineSender {
    inner: watch::Sender<DynEngine>,
}

impl EngineSender {
    fn new(watch_sender: watch::Sender<DynEngine>) -> Self {
        Self {
            inner: watch_sender,
        }
    }

    pub fn broadcast(&mut self, engine: DynEngine) {
        self.inner
            .broadcast(engine)
            .expect("cannot broadcast work engine")
    }
}

/// Manages incoming WorkEngines (see get_engine() for details)
#[derive(Clone)]
pub struct EngineReceiver {
    /// Broadcast channel that is used to distribute current `WorkEngine`
    watch_receiver: watch::Receiver<DynEngine>,
    /// A channel that is (if present) used to send back exhausted engines
    /// to be "recycled" or just so that engine sender is notified that all work
    /// has been generated from them
    reschedule_sender: Option<mpsc::UnboundedSender<DynEngine>>,
}

impl EngineReceiver {
    fn new(
        watch_receiver: watch::Receiver<DynEngine>,
        reschedule_sender: Option<mpsc::UnboundedSender<DynEngine>>,
    ) -> Self {
        Self {
            watch_receiver,
            reschedule_sender,
        }
    }

    /// Provides the most recent WorkEngine as long as the engine is able to provide any work.
    /// Otherwise, it sleeps and waits for a new
    pub async fn get_engine(&mut self) -> Option<DynEngine> {
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

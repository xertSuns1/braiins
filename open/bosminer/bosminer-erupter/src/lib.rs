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

pub mod config;
pub mod device;
pub mod error;
pub mod icarus;

use ii_logging::macros::*;

use bosminer::error::backend::ResultExt;
use bosminer::hal;
use bosminer::shutdown;
use bosminer::stats;
use bosminer::work;

use error::ErrorKind;

use ii_async_compat::{tokio, tokio_executor};
use tokio_executor::threadpool::blocking;

use futures::executor::block_on;
use futures::lock::Mutex;
use futures::task::Poll;
use ii_async_compat::futures;

use std::sync::Arc;
use std::time::Duration;

fn main_task(
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<stats::Mining>>,
    _shutdown: shutdown::Sender,
) -> bosminer::error::Result<()> {
    info!("Block Erupter: finding device in USB...");
    let usb_context =
        libusb::Context::new().context(ErrorKind::Usb("cannot create USB context"))?;
    let mut device = device::BlockErupter::find(&usb_context)
        .ok_or_else(|| ErrorKind::Usb("cannot find Block Erupter device"))?;

    info!("Block Erupter: initialization...");
    device.init()?;
    info!("Block Erupter: initialized and ready to solve the work!");

    let (generator, solution_sender) = work_solver.split();
    let mut solver = device.into_solver(generator);

    // iterate until there exists any work or the error occurs
    for solution in &mut solver {
        solution_sender.send(solution);

        block_on(mining_stats.lock()).unique_solutions += 1;
    }

    // check solver for errors
    solver.get_stop_reason()?;
    Ok(())
}

/// Represents raw solution from the Block Erupter
#[derive(Debug)]
pub struct Solution {
    /// Actual nonce
    nonce: u32,
    /// Index of a solution (if multiple were found)
    solution_idx: usize,
}

impl Solution {
    pub fn new(nonce: u32, solution_idx: usize) -> Self {
        Self {
            nonce,
            solution_idx,
        }
    }
}

impl hal::BackendSolution for Solution {
    #[inline]
    fn nonce(&self) -> u32 {
        self.nonce
    }

    #[inline]
    fn midstate_idx(&self) -> usize {
        // device supports only one midstate
        0
    }

    #[inline]
    fn solution_idx(&self) -> usize {
        self.solution_idx
    }
}

pub struct Backend;

impl hal::Backend for Backend {
    const DEFAULT_MIDSTATE_COUNT: usize = config::DEFAULT_MIDSTATE_COUNT;
    const JOB_TIMEOUT: Duration = config::JOB_TIMEOUT;

    /// Starts statistics tasks specific for block erupter
    fn start_mining_stats_task(_mining_stats: Arc<Mutex<stats::Mining>>) {
        tokio::spawn(stats::hashrate_meter_task());
    }

    fn run(
        &self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<stats::Mining>>,
        shutdown: shutdown::Sender,
    ) {
        // Spawn future in blocking context which guarantees that the task is run in a separate thread
        tokio::spawn(async move {
            let inner = move || {
                if let Err(e) = main_task(work_solver, mining_stats, shutdown) {
                    error!("{}", e);
                }
            };

            // The `blocking()` call is evaluated here with a simple match
            // rather than via `poll_fn()` because the inner closure
            // is `FnOnce` and can't be attempted multiple times anyway.
            match blocking(inner) {
                Poll::Ready(Err(_)) | Poll::Pending => panic!(
                    "Could not run main_task in a blocking context. Note: The Erupter backend requires a threadpool context."),
                _ => {},
            }
        });
    }
}

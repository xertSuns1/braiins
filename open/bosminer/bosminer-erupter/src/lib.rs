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
use bosminer::node;
use bosminer::shutdown;
use bosminer::stats;
use bosminer::work;
use bosminer_macros::MiningStats;

use error::ErrorKind;

use ii_async_compat::{tokio, tokio_executor};
use tokio_executor::blocking;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

fn main_task(
    work_solver: work::Solver,
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

    #[inline]
    fn target(&self) -> &ii_bitcoin::Target {
        &icarus::ASIC_TARGET
    }
}

#[derive(Debug, MiningStats)]
pub struct Backend {
    #[member_mining_stats]
    mining_stats: stats::Mining,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            mining_stats: Default::default(),
        }
    }
}

impl hal::Backend for Backend {
    const DEFAULT_MIDSTATE_COUNT: usize = config::DEFAULT_MIDSTATE_COUNT;
    const DEFAULT_HASHRATE_INTERVAL: Duration = config::DEFAULT_HASHRATE_INTERVAL;
    const JOB_TIMEOUT: Duration = config::JOB_TIMEOUT;

    fn run(self: Arc<Self>, work_solver: work::Solver, shutdown: shutdown::Sender) {
        // Spawn the future in a separate blocking pool (for blocking operations)
        // so that this doesn't block the regular threadpool.
        tokio::spawn(async move {
            blocking::run(move || {
                if let Err(e) = main_task(work_solver, shutdown) {
                    error!("{}", e);
                }
            })
            .await;
        });
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Block Erupter")
    }
}

impl node::Info for Backend {}

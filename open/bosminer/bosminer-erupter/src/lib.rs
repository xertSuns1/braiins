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

use ii_logging::macros::*;

pub mod config;
pub mod device;
pub mod error;
pub mod icarus;

use bosminer::async_trait;
use bosminer::error::backend::ResultExt;
use bosminer::hal;
use bosminer::node;
use bosminer::stats;
use bosminer::work;
use bosminer_macros::WorkSolverNode;

use error::ErrorKind;

use ii_async_compat::tokio;
use tokio::task;

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

#[derive(Debug, WorkSolverNode)]
pub struct Backend {
    #[member_work_solver_stats]
    work_solver_stats: stats::BasicWorkSolver,
    work_generator: Mutex<Option<work::Generator>>,
    solution_sender: work::SolutionSender,
}

impl Backend {
    pub fn new(work_generator: work::Generator, solution_sender: work::SolutionSender) -> Self {
        Self {
            work_solver_stats: Default::default(),
            work_generator: Mutex::new(Some(work_generator)),
            solution_sender,
        }
    }

    fn run(&self) -> bosminer::error::Result<()> {
        info!("Block Erupter: finding device in USB...");
        let usb_context =
            libusb::Context::new().context(ErrorKind::Usb("cannot create USB context"))?;
        let mut device = device::BlockErupter::find(&usb_context)
            .ok_or_else(|| ErrorKind::Usb("cannot find Block Erupter device"))?;

        info!("Block Erupter: initialization...");
        device.init()?;
        info!("Block Erupter: initialized and ready to solve the work!");

        let mut solver = device.into_solver(
            self.work_generator
                .lock()
                .expect("cannot lock work generator")
                .take()
                .expect("missing work generator"),
        );

        // iterate until there exists any work or the error occurs
        for solution in &mut solver {
            self.solution_sender.send(solution);
        }

        // check solver for errors
        solver.get_stop_reason()?;
        Ok(())
    }

    fn enable(self: Arc<Self>) {
        // Spawn the future in a separate blocking pool (for blocking operations)
        // so that this doesn't block the regular threadpool.
        task::spawn_blocking(move || {
            if let Err(e) = self.run() {
                error!("{}", e);
            }
        });
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Block Erupter")
    }
}

#[async_trait]
impl hal::Backend for Backend {
    type Type = Self;
    type Config = config::Backend;

    const DEFAULT_HASHRATE_INTERVAL: Duration = config::DEFAULT_HASHRATE_INTERVAL;
    const JOB_TIMEOUT: Duration = config::JOB_TIMEOUT;

    fn create(_backend_config: &mut config::Backend) -> hal::WorkNode<Self> {
        node::WorkSolverType::WorkSolver(Box::new(|work_generator, solution_sender| {
            Self::new(work_generator, solution_sender)
        }))
    }

    async fn init_work_hub(
        _backend_config: config::Backend,
        _work_hub: work::SolverBuilder<Self::Type>,
    ) -> bosminer::Result<hal::FrontendConfig> {
        panic!("BUG: called `init_work_hub`");
    }

    async fn init_work_solver(
        _config: config::Backend,
        work_solver: Arc<Self>,
    ) -> bosminer::Result<hal::FrontendConfig> {
        // TODO: remove it after `node::WorkSolver` trait will be extended with `enable` method
        work_solver.enable();

        Ok(hal::FrontendConfig {
            cgminer_custom_commands: None,
        })
    }
}

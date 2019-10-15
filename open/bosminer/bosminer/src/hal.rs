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

use crate::shutdown;
use crate::stats;
use crate::work;

use futures::lock::Mutex;

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

/// Represents raw solution from the mining hardware
pub trait BackendSolution: Debug + Send + Sync {
    /// Actual nonce
    fn nonce(&self) -> u32;
    /// Index of a midstate that corresponds to the found nonce
    fn midstate_idx(&self) -> usize;
    /// Index of a solution (if multiple were found)
    fn solution_idx(&self) -> usize;
}

/// Minimal interface for running compatible backend with bOSminer crate
pub trait Backend: Send + Sync + 'static {
    /// Number of midstates
    const DEFAULT_MIDSTATE_COUNT: usize;
    /// Maximum time it takes to compute one job under normal circumstances
    const JOB_TIMEOUT: Duration;

    /// TODO: This function is to be removed once we replace the stats module with a more robust
    /// solution
    fn start_mining_stats_task(mining_stats: Arc<Mutex<stats::Mining>>);

    fn add_args<'a, 'b>(&self, app: clap::App<'a, 'b>) -> clap::App<'a, 'b> {
        app
    }

    fn init(&mut self, _args: &clap::ArgMatches) {}

    fn run(
        &self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<stats::Mining>>,
        shutdown: shutdown::Sender,
    );
}

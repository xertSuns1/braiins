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

use std::sync::Arc;
use std::time::Duration;

/// Minimal interface for running compatible backend with bOSminer crate
pub trait Backend: Send + Sync + 'static {
    /// Number of midstates
    const DEFAULT_MIDSTATE_COUNT: usize;
    /// Maximum time it takes to compute one job under normal circumstances
    const JOB_TIMEOUT: Duration;

    fn run(
        &self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<stats::Mining>>,
        shutdown: shutdown::Sender,
    );
}

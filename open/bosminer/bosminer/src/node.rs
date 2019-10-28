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

use crate::stats;

use std::fmt::{Debug, Display};
use std::sync::Arc;

/// Generic trait for providing information about unique location of a "node" which is abstraction
/// for all elements that somehow transform or provide jobs/work.
/// Typical path of job/work is: client/pool -> backend -> chain -> chip -> core
/// The `node::Info` also provides interface for accounting various statistics related to shares.
/// All nodes implementing this trait and stored in `work::Solution` internal list will be
/// automatically updated whenever the solution is received in `job::SolutionReceiver`
pub trait Info: Debug + Display + Stats {}

pub trait Stats: Send + Sync {
    /// Return object with all statistics for current node.
    fn mining_stats(&self) -> &dyn stats::Mining;
}

/// Shared node info type
pub type DynInfo = Arc<dyn Info>;

/// Unique path describing hierarchy of components
pub type Path = Vec<DynInfo>;

/// Shared unique path describing hierarchy of components
pub type SharedPath = Arc<Path>;

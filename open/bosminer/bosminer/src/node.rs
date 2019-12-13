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

use crate::job;
use crate::stats;

use bosminer_config::client;

use std::any::Any;
use std::fmt::{Debug, Display};
use std::sync::Arc;

use async_trait::async_trait;

/// Generic trait for providing information about unique location of a "node" which is abstraction
/// for all elements that somehow transform or provide jobs/work.
/// Typical path of job/work is: client/pool -> backend -> chain -> chip -> core
/// The `node::Info` also provides interface for accounting various statistics related to shares.
/// All nodes implementing this trait and stored in `work::Solution` internal list will be
/// automatically updated whenever the solution is received in `job::SolutionReceiver`
pub trait Info: Any + Debug + Display + Stats {
    /// Support method for implementation of equality method
    fn get_unique_ptr(self: Arc<Self>) -> Arc<dyn Any>;
}

pub trait Stats: Send + Sync {
    /// Return object with general mining statistics
    fn mining_stats(&self) -> &dyn stats::Mining;
}

/// Common interface for client nodes with ability to generate new jobs (usually client connected
/// to remote pool)
#[async_trait]
pub trait Client: Info + ClientStats {
    /// Return basic information about client used for connection to remote server
    fn descriptor(&self) -> Option<&client::Descriptor> {
        None
    }

    /// Return latest received job
    async fn get_last_job(&self) -> Option<Arc<dyn job::Bitcoin>>;
    /// Try to enable client (default state of client node should be disabled)
    fn enable(self: Arc<Self>);
}

pub trait ClientStats: Stats {
    /// Return object with client specific statistics
    fn client_stats(&self) -> &dyn stats::Client;
}

pub enum WorkSolverType<H, S = H> {
    WorkHub(H),
    WorkSolver(S),
}

impl<T> WorkSolverType<T>
where
    T: WorkSolver,
{
    pub fn as_ref(&self) -> &T {
        match self {
            WorkSolverType::WorkHub(node) | WorkSolverType::WorkSolver(node) => node,
        }
    }

    pub fn into_inner(self) -> T {
        match self {
            WorkSolverType::WorkHub(node) | WorkSolverType::WorkSolver(node) => node,
        }
    }
}

/// Common interface for nodes with ability to solve generated work and providing common interface
/// for mining control
pub trait WorkSolver: Info + WorkSolverStats {}

pub trait WorkSolverStats: Stats {
    /// Return object with work solver specific statistics
    fn work_solver_stats(&self) -> &dyn stats::WorkSolver;
}

/// Shared node info type
pub type DynInfo = Arc<dyn Info>;

/// Unique path describing hierarchy of components
pub type Path = Vec<DynInfo>;

/// Shared unique path describing hierarchy of components
pub type SharedPath = Arc<Path>;

impl<T: ?Sized + Info> Info for Arc<T> {
    fn get_unique_ptr(self: Arc<Self>) -> Arc<dyn Any> {
        self.as_ref().clone().get_unique_ptr()
    }
}

impl<T: ?Sized + Stats> Stats for Arc<T> {
    fn mining_stats(&self) -> &dyn stats::Mining {
        self.as_ref().mining_stats()
    }
}

impl<T: ?Sized + WorkSolver> WorkSolver for Arc<T> {}

impl<T: ?Sized + WorkSolverStats> WorkSolverStats for Arc<T> {
    fn work_solver_stats(&self) -> &dyn stats::WorkSolver {
        self.as_ref().work_solver_stats()
    }
}

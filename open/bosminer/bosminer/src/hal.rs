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

use crate::client::stratum_v2;
use crate::error;
use crate::node;
use crate::work;

use ii_cgminer_api::command;
use ii_stratum::v2::types::DeviceInfo;

use std::convert::TryInto;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

/// Represents raw solution from the mining hardware
pub trait BackendSolution: Debug + Send + Sync {
    /// Actual nonce
    fn nonce(&self) -> u32;
    /// Index of a midstate that corresponds to the found nonce
    fn midstate_idx(&self) -> usize;
    /// Index of a solution (if multiple were found)
    fn solution_idx(&self) -> usize;
    /// Backend target used for finding this nonce
    /// This information is used mainly for detecting HW errors
    fn target(&self) -> &ii_bitcoin::Target;
}

/// Enum returned from `Backend::create` is intended for choosing type of backend root node (work
/// hub or work solver) and also for providing closure responsible for creating this node.
pub type WorkNode<T> = node::WorkSolverType<
    Box<dyn FnOnce() -> T + Send + Sync>,
    Box<dyn FnOnce(work::Generator, work::SolutionSender) -> T + Send + Sync>,
>;

#[derive(Debug)]
pub struct ClientConfig {
    pub descriptor: bosminer_config::ClientDescriptor,
    pub channel: Option<(
        stratum_v2::ExtensionChannelToStratumReceiver,
        stratum_v2::ExtensionChannelFromStratumSender,
    )>,
}

#[derive(Debug)]
pub struct GroupConfig {
    pub descriptor: bosminer_config::GroupDescriptor,
    pub clients: Vec<ClientConfig>,
}

#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub vendor: String,
    pub hw_rev: String,
    pub fw_ver: String,
    pub dev_id: String,
}

impl Default for BackendInfo {
    fn default() -> Self {
        Self {
            vendor: Default::default(),
            hw_rev: Default::default(),
            fw_ver: Default::default(),
            dev_id: Default::default(),
        }
    }
}

impl From<BackendInfo> for DeviceInfo {
    fn from(info: BackendInfo) -> DeviceInfo {
        DeviceInfo {
            vendor: info
                .vendor
                .try_into()
                .expect("BUG: cannot convert 'DeviceInfo::vendor'"),
            hw_rev: info
                .hw_rev
                .try_into()
                .expect("BUG: cannot convert 'DeviceInfo::hw_rev'"),
            fw_ver: info
                .fw_ver
                .try_into()
                .expect("BUG: cannot convert 'DeviceInfo::fw_ver'"),
            dev_id: info
                .dev_id
                .try_into()
                .expect("BUG: cannot convert 'DeviceInfo::dev_id'"),
        }
    }
}

pub trait BackendConfig: Debug + Send + Sync {
    /// Number of midstates that backend is able to solve at once.
    fn midstate_count(&self) -> usize;
    /// List of client groups which should be used after backend initialization.
    fn client_groups(&mut self) -> Vec<GroupConfig> {
        vec![]
    }
    /// Optional information about backend
    fn info(&self) -> Option<BackendInfo> {
        None
    }
}

pub struct FrontendConfig {
    pub cgminer_custom_commands: Option<command::Map>,
}

/// Minimal interface for running compatible backend with BOSminer crate
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Work solver type used for initialization of backend hierarchy
    type Type: node::WorkSolver;
    /// Backend specific configuration with frontend specific settings
    type Config: BackendConfig;

    /// Default hashrate interval used for statistics
    const DEFAULT_HASHRATE_INTERVAL: Duration;
    /// Maximum time it takes to compute one job under normal circumstances
    const JOB_TIMEOUT: Duration;

    /// Return `node::WorkSolverType` with closure for creating either work hub or work solver
    /// depending on backend preference/implementation. Returned node will be then registered in
    /// BOSminer frontend and passed to appropriate backend method for future initialization
    /// (`init_work_hub` or `init_work_solver`). The create method should be non-blocking and all
    /// blocking operation should be moved to init method which is asynchronous.
    fn create(backend_config: &mut Self::Config) -> WorkNode<Self::Type>;

    // TODO: Create empty default implementation for `init_*` functions after `async_trait` will
    // allow default implementation for methods with return value.

    /// Function is called when `create` function returns `node::WorkSolverType::WorkHub`
    /// Passed work hub should be used for creating backend hierarchy consisting of work hubs and
    /// work solvers. All nodes should be also initialized.
    async fn init_work_hub(
        backend_config: Self::Config,
        work_hub: work::SolverBuilder<Self::Type>,
    ) -> error::Result<FrontendConfig>;

    /// Function is called when `create` function returns `node::WorkSolverType::WorkSolver`
    /// Passed work solver is available for time consuming initialization which should not be done
    /// in create function.
    async fn init_work_solver(
        backend_config: Self::Config,
        work_solver: Arc<Self::Type>,
    ) -> error::Result<FrontendConfig>;
}

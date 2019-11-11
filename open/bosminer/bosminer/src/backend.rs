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

//! This module contains dynamically built backend hierarchy

use crate::node;

use async_trait::async_trait;
use futures::lock::Mutex;
use ii_async_compat::futures;

use std::sync::Arc;

use once_cell::sync::Lazy;

#[async_trait]
pub trait HierarchyBuilder: Send + Sync {
    async fn add_root(&self, work_solver: Arc<dyn node::WorkSolver>);

    async fn branch(
        &self,
        first_child: bool,
        work_hub: Arc<dyn node::WorkSolver>,
        work_solver: Arc<dyn node::WorkSolver>,
    );
}

/// This struct is intended mainly for tests to ignore backend hierarchy completely
pub struct IgnoreHierarchy;

#[async_trait]
impl HierarchyBuilder for IgnoreHierarchy {
    async fn add_root(&self, _work_solver: Arc<dyn node::WorkSolver>) {}

    async fn branch(
        &self,
        _first_child: bool,
        _work_hub: Arc<dyn node::WorkSolver>,
        _work_solver: Arc<dyn node::WorkSolver>,
    ) {
    }
}

/// This struct is the default hierarchy builder for bOSminer. It collects all work solvers and
/// work hubs (special case of solver which only routes work to its child nodes and is useful for
/// statistics aggregation and group control)
pub struct BuildHierarchy;

#[async_trait]
impl HierarchyBuilder for BuildHierarchy {
    async fn add_root(&self, work_solver: Arc<dyn node::WorkSolver>) {
        add_work_solver(work_solver).await;
    }

    async fn branch(
        &self,
        first_child: bool,
        work_hub: Arc<dyn node::WorkSolver>,
        work_solver: Arc<dyn node::WorkSolver>,
    ) {
        if first_child {
            branch_work_solver(work_hub, work_solver).await;
        } else {
            add_work_solver(work_solver).await;
        }
    }
}

/// Helper method that puts a `work_solver` node into a specified `container`
fn push_work_solver(
    container: &mut Vec<Arc<dyn node::WorkSolver>>,
    work_solver: Arc<dyn node::WorkSolver>,
) {
    assert!(
        container
            .iter()
            .find(|old| Arc::ptr_eq(old, &work_solver))
            .is_none(),
        "BUG: work solver already present in the registry"
    );
    container.push(work_solver);
}

async fn add_work_solver(work_solver: Arc<dyn node::WorkSolver>) {
    push_work_solver(&mut *WORK_SOLVERS.lock().await, work_solver)
}

async fn branch_work_solver(
    work_hub: Arc<dyn node::WorkSolver>,
    work_solver: Arc<dyn node::WorkSolver>,
) {
    let mut work_solvers = WORK_SOLVERS.lock().await;

    match work_solvers
        .iter_mut()
        .rev()
        .find(|old_work_solver| Arc::ptr_eq(old_work_solver, &work_hub))
    {
        None => work_solvers.push(work_solver),
        Some(old_work_solver) => {
            *old_work_solver = work_solver;
            push_work_solver(&mut *WORK_HUBS.lock().await, work_hub);
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn get_work_solvers() -> Vec<Arc<dyn node::WorkSolver>> {
    WORK_SOLVERS.lock().await.iter().cloned().collect()
}

/// Global lists for distinguishing between work solvers which do real work and work hubs which are
/// only for aggregation and group control. Also CGMiner API reports devices which corresponds to
/// work solver nodes.
static WORK_HUBS: Lazy<Mutex<Vec<Arc<dyn node::WorkSolver>>>> = Lazy::new(|| Mutex::new(vec![]));
static WORK_SOLVERS: Lazy<Mutex<Vec<Arc<dyn node::WorkSolver>>>> = Lazy::new(|| Mutex::new(vec![]));

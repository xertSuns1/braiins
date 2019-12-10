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

use crate::node::{self, WorkSolverType};

use async_trait::async_trait;
use futures::lock::{Mutex, MutexGuard};
use ii_async_compat::futures;

use std::sync::Arc;

#[async_trait]
pub trait HierarchyBuilder: Send + Sync {
    async fn add_work_hub(&self, _work_hub: Arc<dyn node::WorkSolver>) {}

    async fn add_work_solver(&self, _work_solver: Arc<dyn node::WorkSolver>) {}

    async fn add_node(&self, node: WorkSolverType<Arc<dyn node::WorkSolver>>) {
        match node {
            WorkSolverType::WorkHub(work_hub) => {
                self.add_work_hub(work_hub).await;
            }
            WorkSolverType::WorkSolver(work_solver) => {
                self.add_work_solver(work_solver).await;
            }
        }
    }

    async fn add_root(&self, node: WorkSolverType<Arc<dyn node::WorkSolver>>) {
        self.add_node(node).await;
    }

    async fn branch(
        &self,
        _parent_work_hub: Arc<dyn node::WorkSolver>,
        node: WorkSolverType<Arc<dyn node::WorkSolver>>,
    ) {
        self.add_node(node).await;
    }
}

/// This struct is intended mainly for tests to ignore backend hierarchy completely
pub struct IgnoreHierarchy;

#[async_trait]
impl HierarchyBuilder for IgnoreHierarchy {}

/// This structure contains list of backend nodes and is also the default hierarchy builder for the
/// bOSminer. It collects all work solvers and work hubs (special case of solver which only routes
/// work to its child nodes and is useful for statistics aggregation and group control)
pub struct Registry {
    /// Special work hub which represents the whole backend
    root_hub: Mutex<Option<Arc<dyn node::WorkSolver>>>,
    /// List of all work hubs which are useful for statistics aggregation and group control
    work_hubs: Mutex<Vec<Arc<dyn node::WorkSolver>>>,
    /// List of work solvers which do real work and usually represents physical HW
    work_solvers: Mutex<Vec<Arc<dyn node::WorkSolver>>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            root_hub: Mutex::new(None),
            work_hubs: Mutex::new(vec![]),
            work_solvers: Mutex::new(vec![]),
        }
    }

    /// Helper method that puts a `work_solver` node into a specified `container`
    fn push_work_solver(
        &self,
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

    async fn register_root_hub(&self, root_hub: Arc<dyn node::WorkSolver>) {
        if let Some(_) = self.root_hub.lock().await.replace(root_hub) {
            panic!("BUG: root hub already present in the registry");
        }
    }

    async fn register_work_hub(&self, work_hub: Arc<dyn node::WorkSolver>) {
        self.push_work_solver(&mut *self.work_hubs.lock().await, work_hub);
    }

    async fn register_work_solver(&self, work_solver: Arc<dyn node::WorkSolver>) {
        self.push_work_solver(&mut *self.work_solvers.lock().await, work_solver);
    }

    #[inline]
    pub async fn lock_root_hub<'a>(&'a self) -> MutexGuard<'a, Option<Arc<dyn node::WorkSolver>>> {
        self.root_hub.lock().await
    }

    #[inline]
    pub async fn lock_work_hubs<'a>(&'a self) -> MutexGuard<'a, Vec<Arc<dyn node::WorkSolver>>> {
        self.work_hubs.lock().await
    }

    #[inline]
    pub async fn lock_work_solvers<'a>(&'a self) -> MutexGuard<'a, Vec<Arc<dyn node::WorkSolver>>> {
        self.work_solvers.lock().await
    }
}

#[async_trait]
impl HierarchyBuilder for Registry {
    async fn add_work_hub(&self, work_hub: Arc<dyn node::WorkSolver>) {
        self.register_work_hub(work_hub).await;
    }

    async fn add_work_solver(&self, work_solver: Arc<dyn node::WorkSolver>) {
        self.register_work_solver(work_solver).await;
    }

    async fn add_root(&self, node: WorkSolverType<Arc<dyn node::WorkSolver>>) {
        // register node as a root hub
        self.register_root_hub(node.as_ref().clone()).await;
        // and add its actual type (work hub/solver)
        self.add_node(node).await;
    }
}

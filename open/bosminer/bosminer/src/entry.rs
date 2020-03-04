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

//! This module provides top level functionality to build the BOSminer core and use it to connect
//! the frontend and hardware specific backend.

use crate::api;
use crate::backend;
use crate::hal::{self, BackendConfig as _};
use crate::hub;
use crate::stats;

use ii_async_compat::tokio;

use std::sync::Arc;

pub async fn main<T: hal::Backend>(backend_config: T::Config, signature: &'static str) {
    let backend_registry = Arc::new(backend::Registry::new());
    // Get frontend specific settings from backend config
    let backend_info = backend_config.info();

    // Initialize hub core which manages all resources
    let core = Arc::new(hub::Core::new(
        backend_config.midstate_count(),
        &backend_registry,
        backend_info.clone(),
    ));

    // Create and initialize the backend
    let frontend_config = core
        .build_backend::<T>(backend_config)
        .await
        .expect("Backend initialization failed");

    tokio::spawn(core.clone().run());
    // start statistics processing
    tokio::spawn(stats::mining_task(
        core.frontend.clone(),
        T::DEFAULT_HASHRATE_INTERVAL,
    ));

    // the bosminer is controlled with API which also controls when the miner will end
    api::run(core, frontend_config, signature).await;
}

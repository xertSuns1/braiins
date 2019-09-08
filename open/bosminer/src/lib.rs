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

#![feature(await_macro, async_await, duration_float)]

pub mod client;
pub mod config;
pub mod error;
pub mod hal;
pub mod job;
pub mod runtime_config;
pub mod shutdown;
pub mod stats;
pub mod work;

pub mod test_utils;

#[cfg(not(feature = "backend_selected"))]
compile_error!(
    "Backend \"antminer_s9\" or \"erupter\" must be selected with parameter '--features'."
);

#[cfg(all(
    feature = "antminer_s9",
    not(all(
        target_arch = "arm",
        target_vendor = "unknown",
        target_os = "linux",
        target_env = "musl"
    ))
))]
compile_error!(
    "Target \"arm-unknown-linux-musleabi\" for \"antminer_s9\" must be selected with parameter '--target'."
);

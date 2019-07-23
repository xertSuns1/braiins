#![feature(await_macro, async_await)]

pub mod btc;
pub mod client;
pub mod error;
pub mod hal;
pub mod misc;
pub mod stats;
pub mod utils;
pub mod work;

#[cfg(test)]
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

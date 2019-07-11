#![feature(await_macro, async_await)]

#[cfg(feature = "erupter")]
mod erupter;
#[cfg(feature = "antminer_s9")]
mod s9;

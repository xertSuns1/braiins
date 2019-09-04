#![feature(await_macro, async_await)]

extern crate fs2;
extern crate futures;
extern crate libc;
extern crate nix;
extern crate timeout_readwrite;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

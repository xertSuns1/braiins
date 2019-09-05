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

use futures::compat::Future01CompatExt;
use futures::{FutureExt, TryFutureExt};
use futures_01::future::Either;
use futures_01::Future as OldFuture;

use std::future::Future as StdFuture;
use std::time::{Duration, Instant};

use tokio::timer::Delay;

/// Like `tokio::run`, but takes an `async` block
pub fn run<F>(future: F)
where
    F: StdFuture<Output = ()> + Send + 'static,
{
    tokio::run(future.unit_error().boxed().compat());
}

/// Like `tokio::spawn`, but takes an `async` block
pub fn spawn<F>(future: F)
where
    F: StdFuture<Output = ()> + Send + 'static,
{
    tokio::spawn(future.unit_error().boxed().compat());
}

/// Start Tokio runtime
///
/// It is much like tokio::run_async, but instead of waiting for all
/// tasks to finish, wait just for the main task.
///
/// This is a way to shutdown Tokio without diving too deep into
/// Tokio internals.
pub fn run_main_exits<F>(future: F)
where
    F: StdFuture<Output = ()> + Send + 'static,
{
    use tokio::runtime::Runtime;

    let mut runtime = Runtime::new().expect("failed to start new Runtime");
    runtime
        .block_on(future.unit_error().boxed().compat())
        .expect("main task can't return error");
}

/// Run a future to completion on the current thread.
///
/// This function will block the caller until the given future has completed.
/// This implementation is compatible with tokio.
pub fn block_on<F: StdFuture + Send>(future: F) -> F::Output {
    future
        .unit_error()
        .boxed()
        .compat()
        .wait()
        .expect("future in `block_on` returned error")
}

/// Enum representing waiting for timeout.
/// Unfortunately timeout Error is thrown away in the process.
#[derive(Debug, Clone, Copy)]
pub enum TimeoutResult<R> {
    TimedOut,
    Error,
    Returned(R),
}

/// Wait for future with timeout
pub async fn timeout_future<O>(
    future: impl StdFuture<Output = O> + Send,
    timeout: Duration,
) -> TimeoutResult<O> {
    match await!(Delay::new(Instant::now() + timeout)
        .select2(future.unit_error().boxed().compat())
        .compat())
    {
        Ok(Either::A((_, _))) => TimeoutResult::TimedOut,
        Ok(Either::B((r, _))) => TimeoutResult::Returned(r),
        Err(_) => TimeoutResult::Error,
    }
}

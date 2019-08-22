use futures::compat::Future01CompatExt;
use futures::{FutureExt, TryFutureExt};
use futures_01::future::Either;
use futures_01::Future as OldFuture;
use ii_wire::utils::CompatFix;
use std::future::Future as StdFuture;
use std::time::{Duration, Instant};
use tokio::timer::Delay;

/// Start Tokio runtime
///
/// It is much like tokio::run_async, but instead of waiting for all
/// tasks to finish, wait just for the main task.
///
/// This is a way to shutdown Tokio without diving too deep into
/// Tokio internals.
pub fn run_async_main_exits<F>(future: F)
where
    F: StdFuture<Output = ()> + Send + 'static,
{
    use tokio::runtime::Runtime;

    let mut runtime = Runtime::new().expect("failed to start new Runtime");
    runtime
        .block_on(future.compat_fix())
        .expect("main task can't return error");
}

/// Run a future to completion on the current thread.
///
/// This function will block the caller until the given future has completed.
/// This implementation is compatible with tokio.
pub fn compat_block_on<F: StdFuture + Send>(f: F) -> F::Output {
    f.unit_error()
        .boxed()
        .compat()
        .wait()
        .expect("future in `compat_block_on` returned error")
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

use tokio::prelude::*;

use futures::{FutureExt, TryFutureExt};

use std::future::Future as StdFuture;
use wire::utils::CompatFix;

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

use std::future::Future;
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
    F: Future<Output = ()> + Send + 'static,
{
    use tokio::runtime::Runtime;

    let mut runtime = Runtime::new().expect("failed to start new Runtime");
    runtime
        .block_on(future.compat_fix())
        .expect("main task can't return error");
}

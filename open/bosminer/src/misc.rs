use std::env;
use std::fs::OpenOptions;

use lazy_static::lazy_static;
use slog::{o, Drain, Duplicate, Level, LevelFilter, Logger};
use slog_async;
use slog_envlogger;
use slog_term;

lazy_static! {
    /// Build static reference to the logger that will be accessible from all crates
    pub static ref LOGGER: Logger = {
        let level = Level::Trace;

        // envlogger doesn't allow to set default log level, so this is a workaround
        if !env::var("RUST_LOG").is_ok() {
            env::set_var("RUST_LOG", "info");
        }

        // Setup drain for terminal output
        let terminal_decorator = slog_term::TermDecorator::new().build();
        let terminal_drain = slog_term::FullFormat::new(terminal_decorator).build().fuse();
        let terminal_drain = LevelFilter::new(terminal_drain, level).fuse();
        let terminal_drain = slog_envlogger::new(terminal_drain);
        let terminal_drain = slog_async::Async::new(terminal_drain).build().fuse();

        // Setup drain for file output
        let file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .truncate(false)
                .open("/tmp/bosminer.log")
                .unwrap();

        let file_decorator = slog_term::PlainDecorator::new(file);
        let file_drain = slog_term::FullFormat::new(file_decorator).build().fuse();
        let file_drain = LevelFilter::new(file_drain, level).fuse();
        let file_drain = slog_async::Async::new(file_drain).build().fuse();

        // Combine both drains
        let composite_drain = Duplicate::new(terminal_drain, file_drain).fuse();

        let log = Logger::root(composite_drain, o!());
        log
    };
}

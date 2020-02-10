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

// Re-export futures and tokio
pub use bytes;
pub use futures;
pub use tokio;
pub use tokio_util;

/// A general async prelude.
///
/// Re-exports `futures::prelude::*`, along with `tokio`, `tokio_util`
/// and `FutureExt` (custom extensions).
pub mod prelude {
    pub use super::{bytes, futures, tokio, tokio_util, FutureExt as _};

    pub use futures::prelude::*;

    pub use stream_cancel::{StreamExt as _, Tripwire};
}

pub use stream_cancel::{self, Tripwire};

use std::error::Error as StdError;
use std::fmt;
use std::panic::{self, PanicInfo};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::prelude::*;
use stream_cancel::Trigger;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinError, JoinHandle};
use tokio::{signal, time};

/// This registers a customized panic hook with the stdlib.
/// The customized panic hook does the same thing as the default
/// panic handling - ie. it prints out the panic information
/// and optionally a trace - but then it calls abort().
///
/// This means that a panic in Tokio threadpool worker thread
/// will bring down the whole program as if the panic
/// occured on the main thread.
///
/// This function can be called any number of times,
/// but the hook will be set only on the first call.
/// This is thread-safe.
pub fn setup_panic_handling() {
    static HOOK_SETTER: Once = Once::new();

    HOOK_SETTER.call_once(|| {
        let default_hook = panic::take_hook();

        let our_hook = move |pi: &PanicInfo| {
            default_hook(pi);
            process::abort();
        };

        panic::set_hook(Box::new(our_hook));
    });
}

/// An extension trait for `Future` goodies,
/// currently this only entails the `timeout()` function.
pub trait FutureExt: Future {
    /// Require a `Future` to complete before the specified duration has elapsed.
    ///
    /// This is a chainable alias for `tokio::time::timeout()`.
    fn timeout(self, timeout: Duration) -> time::Timeout<Self>
    where
        Self: Sized,
    {
        time::timeout(timeout, self)
    }
}

impl<F: Future> FutureExt for F {}

/// Wrapper for `select!` macro from `futures`.
/// The reason for this is that the macro needs to be told
/// to look for futures at `::ii_async_compat::futures` rather than `::furures`.
#[macro_export]
macro_rules! select {
    ($($tokens:tt)*) => {
        futures::inner_macro::select! {
            futures_crate_path(::ii_async_compat::futures)
            $( $tokens )*
        }
    }
}

/// Wrapper for `join!` macro from `futures`.
/// The reason for this is that the macro needs to be told
/// to look for futures at `::ii_async_compat::futures` rather than `::futures`.
#[macro_export]
macro_rules! join {
    ($($tokens:tt)*) => {
        futures::inner_macro::join! {
            futures_crate_path(::ii_async_compat::futures)
            $( $tokens )*
        }
    }
}

#[derive(Debug)]
struct Halt {
    trigger: Trigger,
    notify_tx: oneshot::Sender<()>,
}

#[derive(Debug)]
struct Joins {
    joins_rx: mpsc::UnboundedReceiver<JoinHandle<()>>,
    notify_rx: oneshot::Receiver<()>,
}

/// Error type returned by `HaltHandle::join()`.
#[derive(Debug)]
pub enum HaltError {
    /// Tasks didn't finish inside the timeout passed to `join()`.
    Timeout,
    /// Once of the tasks panicked.
    Join(JoinError),
}

impl HaltError {
    fn map<'a, T, F: FnOnce(&'a JoinError) -> Option<T>>(&'a self, f: F) -> Option<T> {
        match self {
            HaltError::Timeout => None,
            HaltError::Join(err) => f(err),
        }
    }
}

impl fmt::Display for HaltError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HaltError::Timeout => write!(fmt, "Timeout"),
            HaltError::Join(err) => write!(fmt, "Join error: {}", err),
        }
    }
}

impl StdError for HaltError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.map(JoinError::source)
    }

    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn StdError> {
        self.map(JoinError::cause)
    }
}

/// A handle with which tasks can be spawned
/// that can be then signaled to halt.
#[derive(Debug)]
pub struct HaltHandle {
    tripwire: Tripwire,
    halt: Mutex<Option<Halt>>,
    joins_tx: mpsc::UnboundedSender<JoinHandle<()>>,
    joins: Mutex<Option<Joins>>,
    ctrlc_task_spawned: AtomicBool,
}

impl HaltHandle {
    /// Constructor
    pub fn new() -> Self {
        let (trigger, tripwire) = Tripwire::new();
        let (notify_tx, notify_rx) = oneshot::channel();
        let (joins_tx, joins_rx) = mpsc::unbounded_channel();

        Self {
            tripwire,
            halt: Mutex::new(Some(Halt { trigger, notify_tx })),
            joins_tx,
            joins: Mutex::new(Some(Joins {
                joins_rx,
                notify_rx,
            })),
            ctrlc_task_spawned: AtomicBool::new(false),
        }
    }

    /// Create a handle and wrap in `Arc` for sharing between tasks.
    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Spawn a new task. `f` is a function that takes
    /// a `Tripwire` and returns a `Future` to be spawned.
    /// `Tripwire` can be passed to `StreamExt::take_until`
    /// to make the stream stop generating items when
    /// `halt()` is called on the `HaltHandle`.
    pub fn spawn<FT, FN>(&self, f: FN)
    where
        FT: Future<Output = ()> + Send + 'static,
        FN: FnOnce(Tripwire) -> FT,
    {
        let ft = f(self.tripwire.clone());
        let task = tokio::spawn(ft);
        self.add_task(task);
    }

    pub(crate) fn add_task(&self, task: JoinHandle<()>) {
        let _ = self.joins_tx.send(task);
    }

    /// Tell the handle to halt all the associated tasks.
    pub fn halt(&self) {
        if let Some(halt) = self.halt.lock().unwrap().take() {
            halt.trigger.cancel();
            halt.notify_tx.send(()).unwrap();
        }
    }

    // TODO: Convert these to take self: &Arc<Self> once this is stabilized
    // cf. https://github.com/rust-lang/rust/issues/44874
    /// Tell the handle to call `halt()` in `Ctrl + C` / `SIGINT`.
    pub fn halt_on_ctrlc(self: Arc<Self>) {
        Self::handle_ctrlc(self, |this| async move { this.halt() });
    }

    /// Tell the handle to catch `Ctrl + C` / `SIGINT` and run
    /// the future generated by `f` when the signal is received.
    pub fn handle_ctrlc<FT, FN>(self: Arc<Self>, f: FN)
    where
        FT: Future + Send + 'static,
        FN: FnOnce(Arc<Self>) -> FT,
    {
        if !self
            .ctrlc_task_spawned
            .compare_and_swap(false, true, Ordering::SeqCst)
        {
            let ft = f(self);
            tokio::spawn(async move {
                signal::ctrl_c().await.expect("Error listening for SIGINT");
                ft.await;
            });
        }
    }

    /// Wait for all associated tasks to finish once `halt()` is called.
    /// An optional `timeout` may be provided, this is the maximum time
    /// to wait after `halt()` has been called before.
    ///
    /// Returns `Ok(())` when tasks are collected succesfully, or a `HaltError::Timeout`
    /// when tasks didn't stop in time, or a `HaltError::Join` when a task panics.
    pub async fn join(&self, timeout: Option<Duration>) -> Result<(), HaltError> {
        let mut joins = self
            .joins
            .lock()
            .unwrap()
            .take()
            .expect("HaltHandle: join() called multiple times");
        let _ = joins.notify_rx.await;

        let mut handles = vec![];
        while let Ok(handle) = joins.joins_rx.try_recv() {
            handles.push(handle);
        }

        let ft = future::join_all(handles.drain(..));
        let mut res = if let Some(timeout) = timeout {
            match ft.timeout(timeout).await {
                Ok(res) => res,
                Err(_) => return Err(HaltError::Timeout),
            }
        } else {
            ft.await
        };

        res.drain(..)
            .fold(Ok(()), Result::and)
            .map_err(|e| HaltError::Join(e))
    }
}

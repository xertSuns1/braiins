// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU Common Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Common Public License for more details.
//
// You should have received a copy of the GNU Common Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! This module provides a way to
//!   * spawn tasks in "termination context"
//!   * terminate that context
//!   * wait for "termination" in normal context
//!
//! Termination context means that task is run `select`-ed on termiation condition, and when
//! that condition is signaled, select returns and the task is dropped.
//! In case you want to do some cleanup, you can wait on the termination condition and then
//! cancel/cleanup whatever you want.
//!
//! TODO: This module is still a bit of a hack. In order to do this termination properly, you need
//! to have feedback from tasks and cleanup handlers, that the termination has been done. Only then
//! can you return from `do_stop` task.

use ii_logging::macros::*;

use std::sync::Arc;
use std::time::Duration;
use tokio::timer::delay_for;

use core::future::Future;
use futures::future::select;
use futures::future::FutureExt;
use futures::lock::Mutex;
use futures::stream::StreamExt;
use ii_async_compat::futures;
use ii_async_compat::tokio;
use tokio::sync::watch;

/// Sender of `Halt` condition
#[derive(Clone)]
pub struct Sender {
    inner: Arc<Mutex<watch::Sender<bool>>>,
}

impl Sender {
    /// Broadcast `Halt` condition
    pub async fn do_stop(&self) {
        self.inner
            .lock()
            .await
            .broadcast(true)
            .expect("restart broadcasting failed");
        // TODO: this is a hack, we should collect "halt status" from all receivers and return
        // once we've collected them all.
        delay_for(Duration::from_secs(2)).await;
    }
}

/// Receiver of `Halt` condition
#[derive(Clone)]
pub struct Receiver {
    inner: watch::Receiver<bool>,
}

impl Receiver {
    /// Wait for `Halt` to be broadcasted
    pub async fn wait_for_halt(&mut self) {
        loop {
            match self.inner.next().await {
                None => {
                    error!("Owner dropped HaltSender, no one to stop us now! Shutting down task.");
                    break;
                }
                Some(halt) => {
                    if halt {
                        break;
                    }
                }
            }
        }
    }

    /// Spawn a new task that is dropped when `Halt` is received
    pub fn spawn<F>(&self, f: F)
    where
        F: Future<Output = ()> + 'static + Send,
    {
        let mut receiver = self.clone();
        tokio::spawn(async move {
            select(f.boxed(), receiver.wait_for_halt().boxed()).await;
        });
    }
}

pub fn make_pair() -> (Sender, Receiver) {
    let (tx, rx) = watch::channel(false);
    (
        Sender {
            inner: Arc::new(Mutex::new(tx)),
        },
        Receiver { inner: rx },
    )
}

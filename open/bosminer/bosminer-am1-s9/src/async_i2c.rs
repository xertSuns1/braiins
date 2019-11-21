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

//! Async wrapper for `I2cdev` - runs I2cDevice in a separate thread and forwards
//! requests from async tasks.

use ii_logging::macros::*;

use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::executor::block_on;
use futures::stream::StreamExt;
use ii_async_compat::{futures, tokio, tokio_executor};
use tokio_executor::blocking;

use embedded_hal::blocking::i2c::{Read, Write};
use linux_embedded_hal::I2cdev;

use crate::error::{self, ErrorKind};
use failure::ResultExt;

use std::convert::AsRef;
use std::path::Path;

enum Request {
    Read {
        address: u8,
        num_bytes: usize,
        /// Channel used to send back result
        reply: oneshot::Sender<error::Result<Vec<u8>>>,
    },
    Write {
        address: u8,
        bytes: Vec<u8>,
        /// Channel used to send back result
        reply: oneshot::Sender<error::Result<()>>,
    },
}

/// Server for I2C read/write requests
/// Runs in separate thread.
/// Terminates when all request sender sides are dropped.
fn serve_requests(
    mut i2c_device: I2cdev,
    mut request_rx: mpsc::UnboundedReceiver<Request>,
) -> error::Result<()> {
    while let Some(request) = block_on(request_rx.next()) {
        match request {
            Request::Read {
                address,
                num_bytes,
                reply,
            } => {
                let mut bytes = vec![0; num_bytes];
                let result = i2c_device
                    .read(address, &mut bytes)
                    .with_context(|e| ErrorKind::I2c(e.to_string()))
                    .map(|_| bytes)
                    .map_err(|e| e.into());
                if reply.send(result).is_err() {
                    warn!("AsyncI2c reply send failed - remote side may have ended");
                }
            }
            Request::Write {
                address,
                bytes,
                reply,
            } => {
                let result = i2c_device
                    .write(address, &bytes)
                    .with_context(|e| ErrorKind::I2c(e.to_string()))
                    .map_err(|e| e.into());
                if reply.send(result).is_err() {
                    warn!("AsyncI2c reply send failed - remote side may have ended");
                }
            }
        }
    }
    Ok(())
}

/// Clonable async I2C device. I2cDevice is closed when last sender channel is dropped.
pub struct AsyncI2cDev {
    request_tx: mpsc::UnboundedSender<Request>,
}

/// TODO: Make this into a trait, then implement different backends.
/// TODO: Write tests for this and for power controller (fake async I2C with power controller,
/// check power initialization goes as expected etc., maybe reuse I2C bus from sensors?).
/// TODO: Reuse traits from `i2c/i2c.rs`
impl AsyncI2cDev {
    /// Open I2C device
    /// Although this function is not async, it has to be called from within Tokio context
    /// because it spawns task in a separate thread that serves the (blocking) I2C requests.
    pub fn open<P: AsRef<Path>>(path: P) -> error::Result<Self> {
        let i2c_device = I2cdev::new(path).with_context(|e| ErrorKind::I2c(e.to_string()))?;
        let (request_tx, request_rx) = mpsc::unbounded();

        // Spawn the future in a separate blocking pool (for blocking operations)
        // so that this doesn't block the regular threadpool.
        tokio::spawn(async move {
            blocking::run(move || {
                if let Err(e) = serve_requests(i2c_device, request_rx) {
                    error!("{}", e);
                }
            })
            .await;
        });

        Ok(Self { request_tx })
    }

    pub async fn read(&self, address: u8, num_bytes: usize) -> error::Result<Vec<u8>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let request = Request::Read {
            address,
            num_bytes,
            reply: reply_tx,
        };
        self.request_tx
            .unbounded_send(request)
            .expect("I2C request failed");
        reply_rx.await.expect("failed to receive I2C reply")
    }

    pub async fn write(&self, address: u8, bytes: Vec<u8>) -> error::Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let request = Request::Write {
            address,
            bytes,
            reply: reply_tx,
        };
        self.request_tx
            .unbounded_send(request)
            .expect("I2C request failed");
        reply_rx.await.expect("failed to receive I2C reply")
    }
}

// Please somebody write tests here

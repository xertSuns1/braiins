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

use futures::channel::mpsc;
use futures::stream::StreamExt;

/// Message used for shutdown synchronization
pub type ShutdownMsg = &'static str;

/// Sender side of shutdown messenger
#[derive(Clone)]
pub struct Sender(mpsc::UnboundedSender<ShutdownMsg>);

impl Sender {
    pub fn send(&self, msg: ShutdownMsg) {
        self.0.unbounded_send(msg).expect("shutdown send failed");
    }
}

/// Receiver side of shutdown messenger
pub struct Receiver(mpsc::UnboundedReceiver<ShutdownMsg>);

impl Receiver {
    pub async fn receive(&mut self) -> ShutdownMsg {
        let reply = await!(self.0.next());

        // TODO: do we have to handle all these cases?
        let msg = match reply {
            None => "all hchains died",
            Some(m) => m,
        };
        msg
    }
}

/// Shutdown messenger channel
pub fn channel() -> (Sender, Receiver) {
    let (shutdown_tx, shutdown_rx) = mpsc::unbounded();
    (Sender(shutdown_tx), Receiver(shutdown_rx))
}

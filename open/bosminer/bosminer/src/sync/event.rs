// Copyright (C) 2020  Braiins Systems s.r.o.
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

use ii_async_compat::tokio;
use tokio::sync::broadcast;

type BroadcastSender = broadcast::Sender<()>;
type BroadcastReceiver = broadcast::Receiver<()>;

#[derive(Debug, Clone)]
pub struct Monitor {
    broadcast_sender: BroadcastSender,
}

impl Monitor {
    pub fn new() -> Self {
        // Throwaway the receiver as we provide the `subscribe()` method to create as many
        // broadcast receivers as needed for a particular application
        let (broadcast_sender, _) = broadcast::channel(1);

        Self { broadcast_sender }
    }

    #[inline]
    pub fn publish(&self) -> Sender {
        Sender {
            broadcast_sender: self.broadcast_sender.clone(),
        }
    }

    #[inline]
    pub fn subscribe(&self) -> Receiver {
        Receiver {
            broadcast_receiver: self.broadcast_sender.subscribe(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sender {
    broadcast_sender: BroadcastSender,
}

impl Sender {
    #[inline]
    pub fn notify(&self) {
        // Ignore number of subscribers and errors because there are recoverable
        let _ = self.broadcast_sender.send(());
    }
}

#[derive(Debug)]
pub struct Receiver {
    broadcast_receiver: BroadcastReceiver,
}

impl Receiver {
    pub async fn wait_for_event(&mut self) -> Result<(), ()> {
        loop {
            match self.broadcast_receiver.recv().await {
                Ok(_) => return Ok(()),
                Err(broadcast::RecvError::Lagged(_)) => {
                    // Receiver handle falls behind - try it again
                }
                Err(broadcast::RecvError::Closed) => return Err(()),
            }
        }
    }
}

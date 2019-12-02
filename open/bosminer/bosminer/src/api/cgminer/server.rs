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

//! Contains all server-related (networking) code

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use ii_async_compat::{bytes, futures, tokio, tokio_util};

use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use tokio_util::codec::{Decoder, Encoder, LinesCodec, LinesCodecError};

use serde_json as json;

use super::{Command, Handler, ResponseSet};

/// Codec for the CGMiner API.
/// The `Codec` decodes `Command`s and encodes `ResponseSet`s.
#[derive(Default, Debug)]
pub struct Codec(LinesCodec);

fn no_max_line_length(err: LinesCodecError) -> io::Error {
    match err {
        LinesCodecError::Io(io) => io,
        LinesCodecError::MaxLineLengthExceeded => unreachable!(),
    }
}

impl Decoder for Codec {
    type Item = Command;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let line = self.0.decode(src).map_err(no_max_line_length)?;

        if let Some(line) = line {
            json::from_str(line.as_str())
                .map(Command::new)
                .map(Option::Some)
                .map_err(Into::into)
        } else {
            Ok(None)
        }
    }
}

impl Encoder for Codec {
    type Item = ResponseSet;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let line = json::to_string(&item)?;
        self.0.encode(line, dst).map_err(no_max_line_length)
    }
}

/// Network framing for the API server, uses `Codec`
#[derive(Debug)]
struct Framing;

impl ii_wire::Framing for Framing {
    type Tx = ResponseSet;
    type Rx = Command;
    type Error = io::Error;
    type Codec = Codec;
}

/// wire-based server type
type Server = ii_wire::Server<Framing>;

/// wire-based connection type
type Connection = ii_wire::Connection<Framing>;

async fn handle_connection(mut conn: Connection, handler: Arc<dyn Handler>) {
    while let Some(Ok(command)) = conn.next().await {
        if let Some(resp) = command.handle(&*handler).await {
            match conn.tx.send(resp).await {
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
}

/// Start up an API server with a `handler` object, listening on `listen_addr`
pub async fn run(handler: Arc<dyn Handler>, listen_addr: SocketAddr) -> io::Result<()> {
    let mut server = Server::bind(&listen_addr)?;

    while let Some(conn) = server.next().await {
        if let Ok(conn) = conn {
            tokio::spawn(handle_connection(conn, handler.clone()));
        }
    }

    Ok(())
}

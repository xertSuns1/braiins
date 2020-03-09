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

//! A generic CGMiner API server

pub mod command;
pub mod response;
pub mod support;

#[cfg(test)]
mod test;

use ii_logging::macros::*;

use ii_async_compat::{bytes, futures, tokio, tokio_util};

use bytes::{Buf, BufMut, BytesMut};
use futures::{SinkExt, StreamExt};
use serde_json::Deserializer;
use tokio_util::codec::{Decoder, Encoder};

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

/// Re-export json because it is required in command handlers
pub use serde_json as json;

/// Version of CGMiner compatible API
pub const API_VERSION: &str = "3.7";

/// Default signature of CGMiner API
pub const SIGNATURE: &str = "CGMiner";
/// Format tag for response messages replaced in dispatcher with real signature
pub const SIGNATURE_TAG: &str = "{SIGNATURE}";

/// Default signature of CGMiner API
pub const PARAMETER_DELIMITER: char = ',';

/// Codec for the CGMiner API.
/// The `Codec` decodes `Command`s and encodes `ResponseSet`s.
#[derive(Default, Debug)]
pub struct Codec {
    encode_buf: Vec<u8>,
}

impl Decoder for Codec {
    type Item = command::Request;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let (res, offset) = {
            let mut stream = Deserializer::from_slice(&*src).into_iter();
            (stream.next(), stream.byte_offset())
        };

        match res {
            Some(Ok(json)) => {
                src.advance(offset);

                if src.as_ref().iter().any(|byte| !byte.is_ascii_whitespace()) {
                    // There was a non-whitespace byte following the JSON
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Stray data following JSON",
                    ))
                } else {
                    Ok(Some(command::Request::new(json)))
                }
            }
            Some(Err(err)) if err.is_eof() => Ok(None),
            Some(Err(err)) => Err(err.into()),
            None => Ok(None),
        }
    }
}

impl Encoder for Codec {
    type Item = support::ResponseType;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.encode_buf.clear();
        json::to_writer(&mut self.encode_buf, &item)?;
        dst.reserve(self.encode_buf.len() + 1);
        dst.put_slice(&self.encode_buf);
        // original CGMiner API returns null terminated string as a JSON response
        dst.put_u8(0);
        Ok(())
    }
}

/// Network framing for the API server, uses `Codec`
#[derive(Debug)]
struct Framing;

impl ii_wire::Framing for Framing {
    type Tx = support::ResponseType;
    type Rx = command::Request;
    type Error = io::Error;
    type Codec = Codec;
}

/// wire-based server type
type Server = ii_wire::Server<Framing>;

/// wire-based connection type
type Connection = ii_wire::Connection<Framing>;

async fn handle_connection_task(mut conn: Connection, command_receiver: Arc<command::Receiver>) {
    let response = match conn.next().await {
        Some(Ok(command)) => command_receiver.handle(command).await,
        Some(Err(err)) if err.kind() == io::ErrorKind::InvalidData => {
            command_receiver.error_response(response::ErrorCode::InvalidJSON)
        }
        _ => return, // We pretty much ignore I/O errors here
    };

    conn.send(response)
        .await
        .unwrap_or_else(|e| warn!("CGMiner API: cannot send response ({})", e));
}

/// Start up an API server with a `command_receiver` object, listening on `listen_addr`
pub async fn run(command_receiver: command::Receiver, listen_addr: SocketAddr) -> io::Result<()> {
    let mut server = Server::bind(&listen_addr)?;
    let command_receiver = Arc::new(command_receiver);

    while let Some(conn) = server.next().await {
        if let Ok(conn) = conn {
            tokio::spawn(handle_connection_task(conn, command_receiver.clone()));
        }
    }

    Ok(())
}

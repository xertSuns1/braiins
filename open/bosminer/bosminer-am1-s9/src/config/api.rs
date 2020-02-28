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

//! This module handles configuration commands needed for configuration backend API

use super::*;

use serde::{Deserialize, Serialize};
use serde_repr::*;

use std::fs;
use std::io::{self, Write};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::time::SystemTime;

// TODO: move it to shared crate
pub struct UnixTime;

impl UnixTime {
    fn now() -> u32 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as u32)
            .unwrap_or(0)
    }
}

fn generator_string() -> String {
    format!("bosminer {}", bosminer::version::STRING.clone())
}

#[derive(Serialize_repr, Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u32)]
pub enum StatusCode {
    Success = 0,

    // error codes
    SystemError = 1,
    MissingFile = 2,
    InvalidFormat = 3,
    IncompatibleFormatVersion = 4,
}

#[derive(Serialize, Clone, Debug)]
struct Status {
    code: StatusCode,
    message: Option<String>,
    generator: String,
    timestamp: u32,
}

impl Status {
    fn new<T: Into<Option<String>>>(code: StatusCode, message: T) -> Self {
        Self {
            code,
            message: message.into(),
            generator: generator_string(),
            timestamp: UnixTime::now(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
struct MetadataResponse {
    pub status: Status,
    pub data: serde_json::Value,
}

#[derive(Serialize, Debug)]
struct DataResponse<B> {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<B>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    pub data: serde_json::Value,
}

#[derive(Serialize, Clone, Debug)]
struct SaveSuccess {
    pub path: String,
    pub format: Format,
}

#[derive(Serialize, Clone, Debug)]
struct SaveResponse {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<SaveSuccess>,
}

struct FileGuard<'a> {
    path: Option<&'a Path>,
    file: Option<fs::File>,
}

impl<'a> FileGuard<'a> {
    fn create(path: &'a Path) -> io::Result<Self> {
        Ok(Self {
            path: Some(path),
            file: Some(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?,
            ),
        })
    }

    fn persist<P: AsRef<Path>>(mut self, path: P) -> io::Result<()> {
        // Close the file before moving
        let _ = self
            .file
            .take()
            .expect("BUG: missing file in file guard for 'persist'");
        fs::rename(self.path.expect("BUG: missing path in file guard"), path)?;
        // Drop the original path because the file has been deleted
        self.path.take();

        Ok(())
    }
}

impl<'a> Deref for FileGuard<'a> {
    type Target = fs::File;

    fn deref(&self) -> &Self::Target {
        self.file
            .as_ref()
            .expect("BUG: missing file in file guard for 'Deref'")
    }
}

impl<'a> DerefMut for FileGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.file
            .as_mut()
            .expect("BUG: missing file in file guard for 'DerefMut'")
    }
}

impl<'a> Drop for FileGuard<'a> {
    fn drop(&mut self) {
        self.path.take().map(|path| {
            fs::remove_file(path).expect(
                format!(
                    "TODO: cannot remove file '{}'",
                    path.to_str().unwrap_or_default()
                )
                .as_str(),
            )
        });
    }
}

pub struct Handler<'a> {
    config_path: &'a str,
    // TODO: consider phantomdata to include `ConfigBody` type in this type
}

impl<'a> Handler<'a> {
    pub const CONFIG_TMP_EXTENSION: &'static str = "toml.part";

    pub fn new(config_path: &'a str) -> Self {
        Self { config_path }
    }

    fn send_response<T>(self, response: T)
    where
        T: Serialize,
    {
        serde_json::to_writer(io::stdout(), &response).expect("BUG: cannot serialize response");
    }

    pub fn handle_metadata<B: ConfigBody>(self) {
        let metadata = FormatWrapper::<B>::metadata();

        let response = MetadataResponse {
            status: Status::new(StatusCode::Success, None),
            data: metadata,
        };

        self.send_response(response);
    }

    pub fn handle_data<B: ConfigBody>(self) {
        let response = match FormatWrapper::<B>::parse(self.config_path) {
            // TODO: Improve error handling
            Ok(config)
            | Err(crate::config::FormatWrapperError::IncompatibleVersion(_, Some(config))) => {
                DataResponse {
                    status: Status::new(StatusCode::Success, None),
                    data: Some(config),
                }
            }
            Err(e) => DataResponse {
                status: Status::new(StatusCode::InvalidFormat, format!("{}", e)),
                data: None,
            },
        };

        self.send_response(response);
    }

    pub fn handle_save<B: ConfigBody>(self) {
        let mut request: SaveRequest =
            serde_json::from_reader(io::stdin()).expect("TODO: deserialize SaveRequest");

        let config_format = Format {
            generator: generator_string().into(),
            timestamp: UnixTime::now().into(),
            version: B::version(),
            model: B::model(),
        };

        let json_format =
            serde_json::to_value(config_format).expect("BUG: cannot serialize Format");
        request
            .data
            .as_object_mut()
            .expect("TODO: invalid data type")
            .insert("format".to_string(), json_format);

        let mut config: FormatWrapper<B> =
            serde_json::from_value(request.data).expect("TODO: deserialize Backend");
        config.sanity_check().expect("TODO: invalid configuration");

        let config_path = Path::new(self.config_path);
        let config_tmp_path = config_path.with_extension(Self::CONFIG_TMP_EXTENSION);

        let mut file = FileGuard::create(&config_tmp_path).expect("TODO: File::create");

        file.write_all(
            toml::to_string_pretty(&config)
                .expect("TODO: toml::to_string_pretty")
                .as_bytes(),
        )
        .expect("TODO: file.write_all");

        file.persist(config_path).expect("TODO: file.persist");

        let response = SaveResponse {
            status: Status::new(StatusCode::Success, None),
            data: Some(SaveSuccess {
                path: config_path
                    .canonicalize()
                    .expect("TODO: path.canonicalize")
                    .into_os_string()
                    .into_string()
                    .expect("TODO: into_os_string"),
                format: config.format,
            }),
        };

        self.send_response(response);
    }
}

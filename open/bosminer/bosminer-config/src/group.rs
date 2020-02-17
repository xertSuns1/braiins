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

/// Contains basic information about group
#[derive(Clone, Debug)]
pub struct Descriptor {
    pub name: String,
    /// Optionally set fixed share ratio which is value between 0.0 to 1.0 where 1.0 represents
    /// that all work is generated from this group
    pub fixed_share_ratio: Option<f64>,
}

impl Descriptor {
    pub const DEFAULT_NAME: &'static str = "Default";
    pub const DEFAULT_INDEX: usize = 0;
}

impl Default for Descriptor {
    fn default() -> Self {
        Self {
            name: Self::DEFAULT_NAME.to_string(),
            fixed_share_ratio: None,
        }
    }
}

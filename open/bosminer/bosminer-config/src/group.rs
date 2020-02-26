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

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub enum LoadBalanceStrategy {
    #[serde(rename = "quota")]
    Quota(usize),
    /// Fixed share ratio is value between 0.0 to 1.0 where 1.0 represents that all work is
    /// generated from the group
    #[serde(rename = "fixed_share_ratio")]
    FixedShareRatio(f64),
}

impl LoadBalanceStrategy {
    pub fn get_quota(&self) -> Option<usize> {
        match self {
            Self::Quota(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_fixed_share_ratio(&self) -> Option<f64> {
        match self {
            Self::FixedShareRatio(value) => Some(*value),
            _ => None,
        }
    }
}

/// Contains basic information about group
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub name: String,
    #[serde(flatten)]
    #[serde(skip_serializing_if = "Option::is_none")]
    strategy: Option<LoadBalanceStrategy>,
}

impl Descriptor {
    pub const DEFAULT_NAME: &'static str = "Default";
    pub const DEFAULT_INDEX: usize = 0;
    pub const DEFAULT_QUOTA: usize = 1;

    pub fn new<T>(name: String, strategy: T) -> Self
    where
        T: Into<Option<LoadBalanceStrategy>>,
    {
        Self {
            name,
            strategy: strategy.into(),
        }
    }

    pub fn strategy(&self) -> LoadBalanceStrategy {
        self.strategy
            .clone()
            .unwrap_or(LoadBalanceStrategy::Quota(Self::DEFAULT_QUOTA))
    }

    pub fn get_quota(&self) -> Option<usize> {
        match self.strategy() {
            LoadBalanceStrategy::Quota(value) => Some(value),
            _ => None,
        }
    }

    pub fn get_fixed_share_ratio(&self) -> Option<f64> {
        self.strategy
            .as_ref()
            .and_then(|strategy| strategy.get_fixed_share_ratio())
    }
}

impl Default for Descriptor {
    fn default() -> Self {
        Self {
            name: Self::DEFAULT_NAME.to_string(),
            strategy: None,
        }
    }
}

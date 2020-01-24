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

use std::ops::Deref;

pub enum OptionDefault<T> {
    Some(T),
    Default(T),
}

impl<T> OptionDefault<T> {
    pub fn new(value: Option<T>, default: T) -> Self {
        match value {
            Some(val) => OptionDefault::Some(val),
            None => OptionDefault::Default(default),
        }
    }

    pub fn is_some(&self) -> bool {
        match *self {
            OptionDefault::Some(_) => true,
            OptionDefault::Default(_) => false,
        }
    }

    pub fn eq_some(&self, b: &T) -> bool
    where
        T: std::cmp::PartialEq,
    {
        match self {
            OptionDefault::Some(a) => a == b,
            OptionDefault::Default(_) => false,
        }
    }
}

impl<T> Deref for OptionDefault<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            OptionDefault::Some(val) => val,
            OptionDefault::Default(val) => val,
        }
    }
}

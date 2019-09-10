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

//! Simple wrapper around UIO device

use crate::error::{self, ErrorKind};
use failure::ResultExt;
use uio_async;

pub struct Device {
    pub uio: uio_async::UioDevice,
    uio_name: String,
}

impl Device {
    pub fn open(hashboard_idx: usize, uio_type: &'static str) -> error::Result<Self> {
        let uio_name = format!("chain{}-{}", hashboard_idx - 1, uio_type);
        let uio = uio_async::UioDevice::open_by_name(&uio_name).with_context(|_| {
            ErrorKind::UioDevice(uio_name.clone(), "cannot find uio device".to_string())
        })?;
        Ok(Self { uio, uio_name })
    }
    pub fn map<T>(&self) -> error::Result<uio_async::UioTypedMapping<T>> {
        let map = self.uio.map_mapping(0).with_context(|_| {
            ErrorKind::UioDevice(self.uio_name.clone(), "cannot map uio device".to_string())
        })?;
        Ok(map.into_typed())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{fifo, MidstateCount};

    /// Index of chain for testing (must exist and be defined in DTS)
    const TEST_CHAIN_INDEX: usize = 8;

    /// Try opening UIO device.
    /// This test needs properly configured UIO devices for hash-chain 8 in
    /// device-tree so that we have something to open.
    #[test]
    fn test_lookup_uio() {
        Device::open(TEST_CHAIN_INDEX, "mem").expect("uio open failed");
    }

    /// Try opening non-existent UIO device.
    #[test]
    #[should_panic]
    fn test_lookup_uio_notfound() {
        Device::open(TEST_CHAIN_INDEX, "nonsense").unwrap();
    }

    /// Try mapping memory from UIO device.
    #[test]
    fn test_map_uio() {
        let _mem: uio_async::UioTypedMapping<u8> = Device::open(TEST_CHAIN_INDEX, "mem")
            .expect("uio open failed")
            .map()
            .expect("mapping failed");
    }

    /// Try to map memory twice.
    /// This is to check that the UioMapping Drop trait is working: Drop
    /// does perform unmap which drops the Uio fd lock.
    #[test]
    fn test_map_uio_twice_checklock() {
        // haha! this should fail
        let _: uio_async::UioTypedMapping<u8> = Device::open(TEST_CHAIN_INDEX, "mem")
            .expect("uio open failed")
            .map()
            .expect("mapping failed");
        let _: uio_async::UioTypedMapping<u8> = Device::open(TEST_CHAIN_INDEX, "mem")
            .expect("uio open failed")
            .map()
            .expect("mapping failed");
    }

    /// Try to map IRQ.
    #[test]
    fn test_map_irq() {
        Device::open(TEST_CHAIN_INDEX, "cmd-rx").expect("uio open failed");
    }

    /// Test that we get IRQ.
    /// Test it on empty tx queue (IRQ always asserted).
    #[test]
    fn test_get_irq() {
        let uio = Device::open(TEST_CHAIN_INDEX, "work-tx")
            .expect("uio open failed")
            .uio;
        uio.irq_enable().expect("irq enable failed");
        let res = uio
            .irq_wait_timeout(fifo::FIFO_READ_TIMEOUT)
            .expect("waiting for timeout failed");
        assert!(res.is_some(), "expected interrupt");
    }

    /// Test that we get timeout when waiting for IRQ.
    /// Test it on empty rx queue (IRQ always deasserted).
    #[test]
    fn test_get_irq_timeout() {
        // TODO: replace this with call to flush or something more meaningful
        // create fifo to flush interrupts
        let mut fifo = fifo::HChainFifo::new(TEST_CHAIN_INDEX, MidstateCount::new(1))
            .expect("fifo construction failed");
        // fifo initialization flushes all received responses
        fifo.init().expect("fifo initialization failed");
        drop(fifo);
        // work rx fifo now shouldn't get any interrupts (it's empty)
        let uio = Device::open(TEST_CHAIN_INDEX, "work-rx")
            .expect("uio open failed")
            .uio;
        uio.irq_enable().expect("irq enable failed");
        let res = uio
            .irq_wait_timeout(fifo::FIFO_READ_TIMEOUT)
            .expect("waiting for timeout failed");
        assert!(res.is_none(), "expected timeout");
    }
}

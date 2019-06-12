use crate::hal;

//pub struct WorkDef<T>(Arc<Mutex<WorkDefInt>>);

pub struct WorkDef {
    i: u64,
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare_test_work(i: u64) -> hal::MiningWork {
    hal::MiningWork {
        version: 0,
        extranonce_2: 0,
        midstates: vec![uint::U256([i, 0, 0, 0])],
        merkel_root_lsw: 0xffff_ffff,
        nbits: 0xffff_ffff,
        ntime: 0xffff_ffff,
        //            version: 0,
        //            extranonce_2: 0,
        //            midstates: vec![uint::U256([v, 2, 3, 4])],
        //            merkel_root_lsw: 0xdeadbeef,
        //            nbits: 0x1a44b9f2,
        //            ntime: 0x4dd7f5c7,
    }
}

impl WorkDef {
    fn get_work(&mut self) -> hal::MiningWork {
        let work = prepare_test_work(self.i);
        self.i = self.i.wrapping_add(1);
        work
    }
}

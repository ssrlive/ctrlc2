// Copyright (c) 2023 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

#[macro_use]
mod harness;
use harness::{platform, run_harness};

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn test_set_handler() {
    let signal_count = Arc::new(AtomicUsize::new(0));
    let signal_count_handler = Arc::clone(&signal_count);
    ctrlc2::set_handler(move || signal_count_handler.fetch_add(1, Ordering::SeqCst) == 1).unwrap();

    for expected in 1..=2 {
        unsafe { platform::raise_ctrl_c() };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while signal_count.load(Ordering::SeqCst) < expected && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(signal_count.load(Ordering::SeqCst), expected);
    }

    match ctrlc2::set_handler(|| true) {
        Err(ctrlc2::Error::MultipleHandlers) => {}
        ret => panic!("{ret:?}"),
    }
}

fn tests() {
    run_tests!(test_set_handler);
}

fn main() {
    run_harness(tests);
}

// Copyright (c) 2015 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

fn main() {
    let handle = ctrlc2::set_handler(move || {
        println!(" ");
        println!("Ctrl-C received, ready to exiting...");
        true
    })
    .expect("Error setting Ctrl-C handler");
    println!("Waiting for Ctrl-C...");
    handle.join().expect("Error joining Ctrl-C handler thread");
    println!("Got it! Exiting...");
}

// Copyright (c) 2015 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

#[cfg(feature = "tokio")]
#[cfg_attr(feature = "tokio", tokio::main(flavor = "current_thread"))]
async fn main() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

    #[allow(deprecated)]
    ctrlc2::set_async_handler(async move {
        println!(" ");
        tx.send(()).await.expect("ctrlc2 send signal failed");
    })
    .await;

    println!("Waiting for Ctrl-C...");
    rx.recv().await.expect("Could not receive from channel.");
    println!("Got it! Exiting...");
}

#[cfg(not(feature = "tokio"))]
fn main() {
    println!("This example requires the 'tokio' feature.");
}

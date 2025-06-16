use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() {
    let running = Arc::new(AtomicUsize::new(0));
    let handle = ctrlc2::set_handler(move || {
        let prev = running.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            println!(" ");
            println!("Ctrl-C received, press Ctrl-C again to exit.");
            false
        } else {
            println!(" ");
            println!("Exiting...");
            true
        }
    })
    .expect("Error setting Ctrl-C handler");
    println!("Running...");
    handle.join().unwrap();
    println!("Exit complete.");
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
    let running = Arc::new(AtomicUsize::new(0));
    let r = running.clone();
    let handle = ctrlc2::set_handler(move || {
        let prev = r.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            println!("Exiting...");
            false
        } else {
            true
        }
    })
    .expect("Error setting Ctrl-C handler");
    println!("Running...");
    for _ in 1..6 {
        thread::sleep(Duration::from_secs(5));
        if running.load(Ordering::SeqCst) > 0 {
            break;
        }
    }
    handle.join().unwrap();
}

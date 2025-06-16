#[cfg(all(feature = "tokio", feature = "async"))]
#[cfg_attr(feature = "tokio", tokio::main(flavor = "current_thread"))]
async fn main() {
    async_main().await;
}

#[cfg(all(not(feature = "tokio"), feature = "async"))]
fn main() {
    futures::executor::block_on(async_main());
}

#[cfg(not(feature = "async"))]
fn main() {
    let _args = <Args as clap::Parser>::parse();
    println!("This example requires the 'async' feature.");
}

#[cfg(feature = "async")]
async fn async_main() {
    let args = <Args as clap::Parser>::parse();

    println!("Please press Ctrl-C {} time(s) to exit", args.count_of_ctrl_c);

    let running = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let ctrlc = ctrlc2::AsyncCtrlC::new(move || {
        let count = running.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        println!("Ctrl-C pressed {} time(s)", count);
        if count < args.count_of_ctrl_c {
            println!(" ");
            println!("Press Ctrl-C {} more time(s) to exit", args.count_of_ctrl_c - count);
            false // Continue handling Ctrl-C
        } else {
            println!(" ");
            println!("Exiting...");
            true // Stop handling Ctrl-C
        }
    })
    .expect("cannot create Ctrl+C handler");

    println!("Waiting for Ctrl-C...");
    ctrlc.await.unwrap();
    println!("Got it! Exiting...");
}

/// async_test2 program parameters
#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Number of times to press Ctrl+C before exiting
    #[arg(short, long, default_value_t = 3)]
    count_of_ctrl_c: usize,
}

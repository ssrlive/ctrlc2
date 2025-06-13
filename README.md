# CtrlC2

[![Version](https://img.shields.io/crates/v/ctrlc2.svg?style=flat)](https://crates.io/crates/ctrlc2)
[![Documentation](https://img.shields.io/badge/docs-release-brightgreen.svg?style=flat)](https://docs.rs/ctrlc2)
[![Download](https://img.shields.io/crates/d/ctrlc2.svg)](https://crates.io/crates/ctrlc2)
[![License](https://img.shields.io/crates/l/ctrlc2.svg?style=flat)](https://github.com/ssrlive/ctrlc2/blob/master/LICENSE-MIT)

> For [this reason](https://github.com/Detegr/rust-ctrlc/pull/110), I have decided to create a fork of [ctrlc](https://github.com/Detegr/rust-ctrlc) and maintain it.
> I will try to keep it up to date with the original repo. If you have any suggestions or want to contribute, please open an issue or a PR. Thanks!
> I will respond to issues and PRs as soon as possible.

A simple easy to use wrapper around Ctrl-C signal.

[Documentation](https://docs.rs/ctrlc2/)

## Example usage

In `cargo.toml`:

```toml
[dependencies]
ctrlc2 = "3.5"
```

then, in `main.rs`

```rust
use std::sync::mpsc::channel;
use ctrlc2;

fn main() {
    let (tx, rx) = channel();
    
    let handle = ctrlc2::set_handler(move || {tx.send(()).expect("Could not send signal on channel."); true})
        .expect("Error setting Ctrl-C handler");
    
    println!("Waiting for Ctrl-C...");
    rx.recv().expect("Could not receive from channel.");
    println!("Got it! Exiting..."); 
    handle.join().unwrap();
}
```

### Asynchronous support

This library now supports asynchronous operation using either the tokio runtimes.

Selecting the tokio is done using feature flags (e.g. --no-default-features --features tokio)

```rust
#[cfg(feature = "tokio")]
#[cfg_attr(feature = "tokio", tokio::main(flavor = "current_thread"))]
async fn main() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

    ctrlc2::set_async_handler(async move {
        tx.send(())
            .await
            .expect("Could not send signal on channel.");
    })
    .await;

    println!("Waiting for Ctrl-C...");
    rx.recv().await.expect("Could not receive from channel.");
    println!("Got it! Exiting...");
}
```

or using the `async` feature, it even more simple:

```rust
#[cfg(all(feature = "async", feature = "tokio"))]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let ctrlc = ctrlc2::AsyncCtrlC::new(move || {
        println!("Ctrl-C received!");
        true
    })
    .expect("cannot create Ctrl+C handler");

    println!("Waiting for Ctrl-C...");
    ctrlc.await;
    println!("Got it! Exiting...");
}
```

#### Try the example yourself
`cargo build --examples && target/debug/examples/readme_example`

## Handling SIGTERM and SIGHUP
Add CtrlC to Cargo.toml using `termination` feature and CtrlC will handle SIGINT, SIGTERM and SIGHUP.

## License

Licensed under either of
 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be dual licensed as above, without any
additional terms or conditions.

## Similar crates

There are alternatives that give you more control over the different signals and/or add async support.

- [signal-hook](https://github.com/vorner/signal-hook)
- [tokio::signal](https://docs.rs/tokio/latest/tokio/signal/index.html)

// Copyright (c) 2017 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

#![warn(missing_docs)]

//! Cross platform handling of Ctrl-C signals.
//!
//! [HandlerRoutine]:https://msdn.microsoft.com/en-us/library/windows/desktop/ms683242.aspx
//!
//! [set_handler()](fn.set_handler.html) allows setting a handler closure which is executed on
//! `Ctrl+C`. On Unix, this corresponds to a `SIGINT` signal. On windows, `Ctrl+C` corresponds to
//! [`CTRL_C_EVENT`][HandlerRoutine] or [`CTRL_BREAK_EVENT`][HandlerRoutine].
//!
//! Setting a handler will start a new dedicated signal handling thread where we
//! execute the handler each time we receive a `Ctrl+C` signal. There can only be
//! one handler, you would typically set one at the start of your program.
//!
//! # Example
//! ```no_run
//! # #[allow(clippy::needless_doctest_main)]
//! use std::sync::atomic::{AtomicBool, Ordering};
//! use std::sync::Arc;
//!
//! fn main() {
//!     let running = Arc::new(AtomicBool::new(true));
//!     let r = running.clone();
//!
//!     let handle = ctrlc2::set_handler(move || {
//!         r.store(false, Ordering::SeqCst);
//!         true
//!     }).expect("Error setting Ctrl-C handler");
//!
//!     println!("Waiting for Ctrl-C...");
//!     while running.load(Ordering::SeqCst) {}
//!     println!("Got it! Exiting...");
//!     handle.join().unwrap();
//! }
//! ```
//!
//! # Handling SIGTERM and SIGHUP
//! Handling of `SIGTERM and SIGHUP` can be enabled with `termination` feature. If this is enabled,
//! the handler specified by `set_handler()` will be executed for `SIGINT`, `SIGTERM` and `SIGHUP`.
//!

#![macro_use]

mod error;
mod platform;
pub use platform::Signal;
mod signal;
pub use signal::*;
#[cfg(feature = "async")]
mod r#async;
#[cfg(feature = "async")]
pub use r#async::AsyncCtrlC;

pub use error::Error;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

static INIT: AtomicBool = AtomicBool::new(false);
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Register signal handler for Ctrl-C.
///
/// Starts a new dedicated signal handling thread. Should only be called once,
/// typically at the start of your program.
///
/// # Example
/// ```no_run
/// ctrlc2::set_handler(|| {println!("Hello world!"); true}).expect("Error setting Ctrl-C handler");
/// ```
///
/// # Warning
/// On Unix, the handler registration for `SIGINT`, (`SIGTERM` and `SIGHUP` if termination feature
/// is enabled) or `SA_SIGINFO` posix signal handlers will be overwritten. On Windows, multiple
/// handler routines are allowed, but they are called on a last-registered, first-called basis
/// until the signal is handled.
///
/// ctrlc2::try_set_handler will error (on Unix) if another signal handler exists for the same
/// signal(s) that ctrlc2 is trying to attach the handler to.
///
/// On Unix, signal dispositions and signal handlers are inherited by child processes created via
/// `fork(2)` on, but not by child processes created via `execve(2)`.
/// Signal handlers are not inherited on Windows.
///
/// # Errors
/// Will return an error if a system error occurred while setting the handler.
///
/// # Panics
/// Any panic in the handler will not be caught and will cause the signal handler thread to stop.
pub fn set_handler<F>(user_handler: F) -> Result<JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    init_and_set_handler(user_handler, true)
}

/// The same as ctrlc2::set_handler but errors if a handler already exists for the signal(s).
///
/// # Errors
/// Will return an error if another handler exists or if a system error occurred while setting the
/// handler.
pub fn try_set_handler<F>(user_handler: F) -> Result<JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    init_and_set_handler(user_handler, false)
}

fn init_and_set_handler<F>(user_handler: F, overwrite: bool) -> Result<JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    if !INIT.load(Ordering::Acquire) {
        let _guard = INIT_LOCK.lock().unwrap();

        if !INIT.load(Ordering::Relaxed) {
            let handle = set_handler_inner(user_handler, overwrite)?;
            INIT.store(true, Ordering::Release);
            return Ok(handle);
        }
    }

    Err(Error::MultipleHandlers)
}

fn set_handler_inner<F>(mut user_handler: F, overwrite: bool) -> Result<JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    unsafe { platform::init_os_handler(overwrite)? };

    let builder = thread::Builder::new()
        .name("ctrl-c".into())
        .spawn(move || {
            loop {
                unsafe { platform::block_ctrl_c() }.expect("Critical system error while waiting for Ctrl-C");

                if user_handler() {
                    break;
                }
            }
        })
        .map_err(Error::System)?;

    Ok(builder)
}

/// Register signal handler in tokio runtime for Ctrl-C.
#[cfg(feature = "tokio")]
pub async fn set_async_handler<F>(user_handler: F) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + 'static + Send,
{
    tokio::spawn(async move {
        let block = async move {
            #[cfg(unix)]
            {
                #[cfg(not(feature = "termination"))]
                tokio::signal::ctrl_c().await?;

                #[cfg(feature = "termination")]
                {
                    use tokio::signal::unix::{SignalKind, signal};
                    let mut kill_signal = signal(SignalKind::terminate())?;
                    let mut int_signal = signal(SignalKind::interrupt())?;
                    let mut hup_signal = signal(SignalKind::hangup())?;
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {},
                        _ = kill_signal.recv() => {},
                        _ = int_signal.recv() => {},
                        _ = hup_signal.recv() => {}
                    }
                }
            }

            #[cfg(windows)]
            tokio::signal::ctrl_c().await?;

            user_handler.await;

            Ok::<(), std::io::Error>(())
        };
        if let Err(err) = block.await {
            eprintln!("Critical system error while waiting for Ctrl-C: {}", err);
        }
    })
}

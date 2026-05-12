// Copyright (c) 2017 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

#![doc = include_str!("../README.md")]

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

static INIT: AtomicBool = AtomicBool::new(false);
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Register signal handler for Ctrl-C.
///
/// Starts a new dedicated signal handling thread. Should only be called once,
/// typically at the start of your program.
///
/// The `user_handler` function is customizable and the return boolean value
/// is indicating whether the user agreed terminate the program or not.
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
pub fn set_handler<F>(user_handler: F) -> Result<std::thread::JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    init_and_set_handler(user_handler, true)
}

/// The same as ctrlc2::set_handler but errors if a handler already exists for the signal(s).
///
/// The `user_handler` function is customizable and the return boolean value
/// is indicating whether the user agreed terminate the program or not.
///
/// # Errors
/// Will return an error if another handler exists or if a system error occurred while setting the
/// handler.
pub fn try_set_handler<F>(user_handler: F) -> Result<std::thread::JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    init_and_set_handler(user_handler, false)
}

fn init_and_set_handler<F>(user_handler: F, overwrite: bool) -> Result<std::thread::JoinHandle<()>, Error>
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

fn set_handler_inner<F>(mut user_handler: F, overwrite: bool) -> Result<std::thread::JoinHandle<()>, Error>
where
    F: FnMut() -> bool + 'static + Send,
{
    unsafe { platform::init_os_handler(overwrite)? };

    let builder = std::thread::Builder::new()
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

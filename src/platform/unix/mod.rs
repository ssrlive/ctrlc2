// Copyright (c) 2017 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

#[cfg(target_vendor = "apple")]
use crate::error::Error as CtrlcError;

#[cfg(not(target_vendor = "apple"))]
mod implementation {
    use nix::errno::Errno;
    use std::cell::UnsafeCell;
    use std::mem::MaybeUninit;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};

    const UNINITIALIZED: u8 = 0;
    const INITIALIZED: u8 = 1;
    const INITIALIZATION_FAILED: u8 = 2;

    struct Semaphore {
        value: UnsafeCell<MaybeUninit<nix::libc::sem_t>>,
        state: AtomicU8,
        initialization_error: AtomicI32,
        post_error: AtomicI32,
        initialization_lock: Mutex<()>,
    }

    // SAFETY: Initialization is serialized and published through `state`. POSIX
    // permits concurrent sem_post/sem_wait operations on an initialized semaphore.
    unsafe impl Sync for Semaphore {}

    impl Semaphore {
        const fn new() -> Semaphore {
            Semaphore {
                value: UnsafeCell::new(MaybeUninit::uninit()),
                state: AtomicU8::new(UNINITIALIZED),
                initialization_error: AtomicI32::new(0),
                post_error: AtomicI32::new(0),
                initialization_lock: Mutex::new(()),
            }
        }

        fn initialization_result(&self) -> Option<Result<(), Errno>> {
            match self.state.load(Ordering::Acquire) {
                INITIALIZED => Some(Ok(())),
                INITIALIZATION_FAILED => Some(Err(Errno::from_raw(self.initialization_error.load(Ordering::Relaxed)))),
                _ => None,
            }
        }

        fn initialize(&self) -> Result<(), Errno> {
            self.initialize_with(|value| {
                // SAFETY: `value` points to stable, uninitialized storage owned by this semaphore.
                let result = unsafe { nix::libc::sem_init(value, 0, 0) };
                if result == 0 { Ok(()) } else { Err(Errno::last()) }
            })
        }

        fn initialize_with(&self, initialize: impl FnOnce(*mut nix::libc::sem_t) -> Result<(), Errno>) -> Result<(), Errno> {
            if let Some(result) = self.initialization_result() {
                return result;
            }

            let _guard = self.initialization_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

            if let Some(result) = self.initialization_result() {
                return result;
            }

            let result = initialize(self.value.get().cast::<nix::libc::sem_t>());
            match result {
                Ok(()) => {
                    self.state.store(INITIALIZED, Ordering::Release);
                    Ok(())
                }
                Err(error) => {
                    self.initialization_error.store(error as i32, Ordering::Relaxed);
                    self.state.store(INITIALIZATION_FAILED, Ordering::Release);
                    Err(error)
                }
            }
        }

        fn post(&self) -> Result<(), Errno> {
            self.ensure_initialized()?;
            // SAFETY: The semaphore was initialized before the signal handler was installed.
            let result = unsafe { nix::libc::sem_post(self.value.get().cast::<nix::libc::sem_t>()) };
            normalize_post_result(if result == 0 { Ok(()) } else { Err(Errno::last()) })
        }

        fn notify(&self) {
            if let Err(error) = self.post() {
                let _ = self
                    .post_error
                    .compare_exchange(0, error as i32, Ordering::Relaxed, Ordering::Relaxed);
            }
        }

        fn wait(&self) -> Result<(), Errno> {
            self.ensure_initialized()?;
            wait_until_signaled(|| {
                let post_error = self.post_error.swap(0, Ordering::Relaxed);
                if post_error != 0 {
                    return Err(Errno::from_raw(post_error));
                }

                // SAFETY: Initialization was published before this semaphore became observable.
                let result = unsafe { nix::libc::sem_wait(self.value.get().cast::<nix::libc::sem_t>()) };
                if result == 0 { Ok(()) } else { Err(Errno::last()) }
            })?;

            let post_error = self.post_error.swap(0, Ordering::Relaxed);
            if post_error == 0 {
                Ok(())
            } else {
                Err(Errno::from_raw(post_error))
            }
        }

        fn ensure_initialized(&self) -> Result<(), Errno> {
            self.initialization_result().unwrap_or(Err(Errno::EINVAL))
        }
    }

    impl Drop for Semaphore {
        fn drop(&mut self) {
            if self.state.load(Ordering::Relaxed) == INITIALIZED {
                // SAFETY: An initialized semaphore is only destroyed with exclusive access.
                unsafe { nix::libc::sem_destroy(self.value.get().cast::<nix::libc::sem_t>()) };
            }
        }
    }

    static SEMAPHORE: Semaphore = Semaphore::new();

    fn normalize_post_result(result: Result<(), Errno>) -> Result<(), Errno> {
        match result {
            // A saturated semaphore already has a pending wakeup.
            Err(Errno::EOVERFLOW) => Ok(()),
            result => result,
        }
    }

    fn wait_until_signaled(mut wait_once: impl FnMut() -> Result<(), Errno>) -> Result<(), Errno> {
        loop {
            match wait_once() {
                Err(Errno::EINTR) => continue,
                result => return result,
            }
        }
    }

    pub fn sem_init() -> Result<(), nix::Error> {
        SEMAPHORE.initialize()
    }

    pub fn sem_post() {
        SEMAPHORE.notify();
    }

    pub fn sem_wait() -> Result<(), nix::Error> {
        SEMAPHORE.wait()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        #[test]
        fn initialization_is_thread_safe_and_only_runs_once() {
            let semaphore = Arc::new(Semaphore::new());
            let initialization_count = Arc::new(AtomicUsize::new(0));
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    let semaphore = Arc::clone(&semaphore);
                    let initialization_count = Arc::clone(&initialization_count);
                    thread::spawn(move || {
                        semaphore
                            .initialize_with(|value| {
                                initialization_count.fetch_add(1, Ordering::Relaxed);
                                // SAFETY: The initializer holds the semaphore's initialization lock.
                                let result = unsafe { nix::libc::sem_init(value, 0, 0) };
                                if result == 0 { Ok(()) } else { Err(Errno::last()) }
                            })
                            .unwrap();
                    })
                })
                .collect();

            for thread in threads {
                thread.join().unwrap();
            }

            assert_eq!(initialization_count.load(Ordering::Relaxed), 1);
        }

        #[test]
        fn initialization_failure_is_reported_and_not_retried() {
            let semaphore = Semaphore::new();
            let initialization_count = AtomicUsize::new(0);
            let initialize = || {
                initialization_count.fetch_add(1, Ordering::Relaxed);
                Err(Errno::EACCES)
            };

            assert_eq!(semaphore.initialize_with(|_| initialize()), Err(Errno::EACCES));
            assert_eq!(
                semaphore.initialize_with(|_| panic!("failed initialization was retried")),
                Err(Errno::EACCES)
            );
            assert_eq!(initialization_count.load(Ordering::Relaxed), 1);
        }

        #[test]
        fn posts_are_counted_even_before_waiting() {
            let semaphore = Semaphore::new();
            semaphore.initialize().unwrap();
            semaphore.post().unwrap();
            semaphore.post().unwrap();

            semaphore.wait().unwrap();
            semaphore.wait().unwrap();
        }

        #[test]
        fn wait_retries_interruption_but_returns_other_errors() {
            let mut attempts = 0;
            assert_eq!(
                wait_until_signaled(|| {
                    attempts += 1;
                    match attempts {
                        1 | 2 => Err(Errno::EINTR),
                        _ => Ok(()),
                    }
                }),
                Ok(())
            );
            assert_eq!(attempts, 3);

            let mut attempts = 0;
            assert_eq!(
                wait_until_signaled(|| {
                    attempts += 1;
                    Err(Errno::EIO)
                }),
                Err(Errno::EIO)
            );
            assert_eq!(attempts, 1);
        }

        #[test]
        fn overflow_is_success_but_other_post_errors_are_preserved() {
            assert_eq!(normalize_post_result(Err(Errno::EOVERFLOW)), Ok(()));
            assert_eq!(normalize_post_result(Err(Errno::EINVAL)), Err(Errno::EINVAL));
        }

        #[test]
        fn unexpected_post_error_is_returned_by_waiter() {
            let semaphore = Semaphore::new();
            semaphore.initialize().unwrap();
            semaphore.post().unwrap();
            semaphore.post_error.store(Errno::EIO as i32, Ordering::Relaxed);

            assert_eq!(semaphore.wait(), Err(Errno::EIO));
        }
    }
}

#[cfg(target_vendor = "apple")]
mod implementation {
    use super::CtrlcError;
    use dispatch2::{DispatchRetained, DispatchSemaphore, DispatchTime};
    use nix::errno::Errno;
    use std::ptr::NonNull;
    use std::sync::OnceLock;

    static SEMAPHORE: OnceLock<Result<DispatchRetained<DispatchSemaphore>, Errno>> = OnceLock::new();

    unsafe extern "C" {
        fn dispatch_semaphore_create(value: isize) -> Option<NonNull<DispatchSemaphore>>;
    }

    fn semaphore() -> Result<&'static DispatchRetained<DispatchSemaphore>, Errno> {
        SEMAPHORE
            .get_or_init(|| {
                // SAFETY: The zero initial value is valid, and the returned object follows
                // dispatch's create rule.
                let semaphore = unsafe { dispatch_semaphore_create(0) }.ok_or(Errno::ENOMEM)?;
                // SAFETY: `dispatch_semaphore_create` returns a retained dispatch object.
                Ok(unsafe { DispatchRetained::from_raw(semaphore) })
            })
            .as_ref()
            .map_err(|error| *error)
    }

    pub fn sem_init() -> Result<(), nix::Error> {
        semaphore()?;
        Ok(())
    }

    pub fn sem_post() {
        if let Some(Ok(semaphore)) = SEMAPHORE.get() {
            semaphore.signal();
        }
    }

    pub fn sem_wait() -> Result<(), CtrlcError> {
        let semaphore = semaphore()?;
        if semaphore.wait(DispatchTime::FOREVER) == 0 {
            Ok(())
        } else {
            Err(CtrlcError::System(std::io::Error::other(
                "dispatch semaphore wait unexpectedly timed out",
            )))
        }
    }
}

/// Platform specific error type
pub type Error = nix::Error;

/// Platform specific signal type
pub type Signal = nix::sys::signal::Signal;

extern "C" fn os_handler(_: nix::libc::c_int) {
    implementation::sem_post();
}

/// Register os signal handler.
///
/// Must be called before calling [`block_ctrl_c()`](fn.block_ctrl_c.html)
/// and should only be called once.
///
/// # Errors
/// Will return an error if semaphore or signal-handler initialization failed.
///
#[inline]
pub unsafe fn init_os_handler(overwrite: bool) -> Result<(), nix::Error> {
    use nix::sys::signal;

    implementation::sem_init()?;

    let handler = signal::SigHandler::Handler(os_handler);
    #[cfg(not(target_os = "nto"))]
    let new_action = signal::SigAction::new(handler, signal::SaFlags::SA_RESTART, signal::SigSet::empty());
    // SA_RESTART is not supported on QNX Neutrino 7.1 and before
    #[cfg(target_os = "nto")]
    let new_action = signal::SigAction::new(handler, signal::SaFlags::empty(), signal::SigSet::empty());

    let sigint_old = unsafe { signal::sigaction(signal::Signal::SIGINT, &new_action) }?;
    if !overwrite && !matches!(sigint_old.handler(), signal::SigHandler::SigDfl) {
        log::warn!("SIGINT handler already set, not overwriting");
        if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old) } {
            log::error!("Failed to restore SIGINT handler: {err}");
        }
        return Err(nix::Error::EEXIST);
    }

    #[cfg(feature = "termination")]
    {
        let sigterm_old = match unsafe { signal::sigaction(signal::Signal::SIGTERM, &new_action) } {
            Ok(old) => old,
            Err(e) => {
                log::warn!("Failed to set SIGTERM handler: {e}");
                if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old) } {
                    log::error!("Failed to restore SIGINT handler: {err}");
                }
                return Err(e);
            }
        };
        if !overwrite && !matches!(sigterm_old.handler(), signal::SigHandler::SigDfl) {
            log::warn!("SIGTERM handler already set, not overwriting");
            if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old) } {
                log::error!("Failed to restore SIGINT handler: {err}");
            }
            if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGTERM, &sigterm_old) } {
                log::error!("Failed to restore SIGTERM handler: {err}");
            }
            return Err(nix::Error::EEXIST);
        }
        let sighup_old = match unsafe { signal::sigaction(signal::Signal::SIGHUP, &new_action) } {
            Ok(old) => old,
            Err(e) => {
                log::warn!("Failed to set SIGHUP handler: {e}");
                if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old) } {
                    log::error!("Failed to restore SIGINT handler: {err}");
                }
                if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGTERM, &sigterm_old) } {
                    log::error!("Failed to restore SIGTERM handler: {err}");
                }
                return Err(e);
            }
        };
        if !overwrite && !matches!(sighup_old.handler(), signal::SigHandler::SigDfl) {
            log::warn!("SIGHUP handler is already set, not overwriting it");
            if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old) } {
                log::error!("Failed to restore SIGINT handler: {err}");
            }
            if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGTERM, &sigterm_old) } {
                log::error!("Failed to restore SIGTERM handler: {err}");
            }
            if let Err(err) = unsafe { signal::sigaction(signal::Signal::SIGHUP, &sighup_old) } {
                log::error!("Failed to restore SIGHUP handler: {err}");
            }
            return Err(nix::Error::EEXIST);
        }
    }

    Ok(())
}

/// Blocks until a Ctrl-C signal is received.
///
/// Must be called after calling [`init_os_handler()`](fn.init_os_handler.html).
///
/// # Errors
/// Will return an error if waiting for the semaphore failed.
///
#[inline]
pub unsafe fn block_ctrl_c() -> Result<(), crate::Error> {
    implementation::sem_wait()?;
    Ok(())
}

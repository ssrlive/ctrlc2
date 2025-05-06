// Copyright (c) 2017 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use nix::unistd;
use std::os::fd::OwnedFd;

static PIPE: std::sync::OnceLock<(OwnedFd, OwnedFd)> = std::sync::OnceLock::new();

/// Platform specific error type
pub type Error = nix::Error;

/// Platform specific signal type
pub type Signal = nix::sys::signal::Signal;

extern "C" fn os_handler(_: nix::libc::c_int) {
    // Assuming this always succeeds. Can't really handle errors in any meaningful way.

    if let Some((_, fd1)) = PIPE.get() {
        let _ = unistd::write(fd1, &[0u8]);
    }
}

// pipe2(2) is not available on macOS, iOS, AIX, Haiku, etc., so we need to use pipe(2) and fcntl(2)
#[inline]
#[cfg(any(target_vendor = "apple", target_os = "haiku", target_os = "aix", target_os = "nto",))]
fn pipe2(flags: nix::fcntl::OFlag) -> nix::Result<(OwnedFd, OwnedFd)> {
    use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};

    let pipe = unistd::pipe()?;

    let mut res = Ok(0);

    if flags.contains(OFlag::O_CLOEXEC) {
        res = res
            .and_then(|_| fcntl(&pipe.0, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)))
            .and_then(|_| fcntl(&pipe.1, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)));
    }

    if flags.contains(OFlag::O_NONBLOCK) {
        res = res
            .and_then(|_| fcntl(&pipe.0, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)))
            .and_then(|_| fcntl(&pipe.1, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)));
    }

    match res {
        Ok(_) => Ok(pipe),
        Err(e) => {
            let _ = unistd::close(pipe.0);
            let _ = unistd::close(pipe.1);
            Err(e)
        }
    }
}

#[inline]
#[cfg(not(any(target_vendor = "apple", target_os = "haiku", target_os = "aix", target_os = "nto",)))]
fn pipe2(flags: nix::fcntl::OFlag) -> nix::Result<(OwnedFd, OwnedFd)> {
    unistd::pipe2(flags)
}

/// Register os signal handler.
///
/// Must be called before calling [`block_ctrl_c()`](fn.block_ctrl_c.html)
/// and should only be called once.
///
/// # Errors
/// Will return an error if a system error occurred.
///
#[inline]
pub unsafe fn init_os_handler(overwrite: bool) -> Result<(), nix::Error> {
    use nix::fcntl;
    use nix::sys::signal;

    let pipe = PIPE.get_or_init(|| pipe2(fcntl::OFlag::O_CLOEXEC).unwrap());

    // Make sure we never block on write in the os handler.
    fcntl::fcntl(&pipe.1, fcntl::FcntlArg::F_SETFL(fcntl::OFlag::O_NONBLOCK))?;

    let handler = signal::SigHandler::Handler(os_handler);
    #[cfg(not(target_os = "nto"))]
    let new_action = signal::SigAction::new(handler, signal::SaFlags::SA_RESTART, signal::SigSet::empty());
    // SA_RESTART is not supported on QNX Neutrino 7.1 and before
    #[cfg(target_os = "nto")]
    let new_action = signal::SigAction::new(handler, signal::SaFlags::empty(), signal::SigSet::empty());

    let sigint_old = unsafe { signal::sigaction(signal::Signal::SIGINT, &new_action) }?;
    if !overwrite && sigint_old.handler() != signal::SigHandler::SigDfl {
        unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old) }.unwrap();
        return Err(nix::Error::EEXIST);
    }

    #[cfg(feature = "termination")]
    {
        let sigterm_old = match unsafe { signal::sigaction(signal::Signal::SIGTERM, &new_action) } {
            Ok(old) => old,
            Err(e) => {
                unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old).unwrap() };
                return Err(e);
            }
        };
        if !overwrite && sigterm_old.handler() != signal::SigHandler::SigDfl {
            unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old).unwrap() };
            unsafe { signal::sigaction(signal::Signal::SIGTERM, &sigterm_old).unwrap() };
            return Err(nix::Error::EEXIST);
        }
        let sighup_old = match unsafe { signal::sigaction(signal::Signal::SIGHUP, &new_action) } {
            Ok(old) => old,
            Err(e) => {
                unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old).unwrap() };
                unsafe { signal::sigaction(signal::Signal::SIGTERM, &sigterm_old).unwrap() };
                return Err(e);
            }
        };
        if !overwrite && sighup_old.handler() != signal::SigHandler::SigDfl {
            unsafe { signal::sigaction(signal::Signal::SIGINT, &sigint_old).unwrap() };
            unsafe { signal::sigaction(signal::Signal::SIGTERM, &sigterm_old).unwrap() };
            unsafe { signal::sigaction(signal::Signal::SIGHUP, &sighup_old).unwrap() };
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
/// Will return an error if a system error occurred.
///
#[inline]
pub unsafe fn block_ctrl_c() -> Result<(), crate::Error> {
    use std::io;
    let mut buf = [0u8];

    let (fd0, _) = PIPE.get().expect("PIPE not initialized");

    // TODO: Can we safely convert the pipe fd into a std::io::Read
    // with std::os::unix::io::FromRawFd, this would handle EINTR
    // and everything for us.
    loop {
        match unistd::read(fd0, &mut buf[..]) {
            Ok(1) => break,
            Ok(_) => return Err(crate::Error::System(io::ErrorKind::UnexpectedEof.into())),
            Err(nix::errno::Errno::EINTR) => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

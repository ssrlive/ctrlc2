// Copyright (c) 2017 CtrlC developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, TRUE, WAIT_FAILED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
use windows_sys::Win32::System::Threading::{CreateSemaphoreA, INFINITE, ReleaseSemaphore, WaitForSingleObject};
use windows_sys::core::BOOL;

/// Platform specific error type
pub type Error = std::io::Error;

/// Platform specific signal type
pub type Signal = u32;

const MAX_SEM_COUNT: i32 = 255;
static mut SEMAPHORE: HANDLE = 0 as HANDLE;

unsafe extern "system" fn os_handler(ctrl_type: u32) -> BOOL {
    match ctrl_type {
        CTRL_C_EVENT => {
            log::debug!("Ctrl-C event received");
        }
        CTRL_BREAK_EVENT => {
            log::debug!("Ctrl-Break event received");
        }
        _ => {
            log::debug!("Unknown control event received: {ctrl_type}");
        }
    }
    // Assuming this always succeeds. Can't really handle errors in any meaningful way.
    unsafe { ReleaseSemaphore(SEMAPHORE, 1, std::ptr::null_mut()) };
    TRUE
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
pub unsafe fn init_os_handler(_overwrite: bool) -> Result<(), Error> {
    unsafe { SEMAPHORE = CreateSemaphoreA(std::ptr::null_mut(), 0, MAX_SEM_COUNT, std::ptr::null()) };
    if unsafe { SEMAPHORE.is_null() } {
        let err = std::io::Error::last_os_error();
        log::error!("Failed to create semaphore: {err}");
        return Err(err);
    }

    if unsafe { SetConsoleCtrlHandler(Some(os_handler), TRUE) } == FALSE {
        let e = std::io::Error::last_os_error();
        unsafe { CloseHandle(SEMAPHORE) };
        unsafe { SEMAPHORE = 0 as HANDLE };
        log::error!("Failed to set console control handler: {e}");
        return Err(e);
    }

    log::debug!("OS signal handler initialized successfully");

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
pub unsafe fn block_ctrl_c() -> Result<(), Error> {
    match unsafe { WaitForSingleObject(SEMAPHORE, INFINITE) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_FAILED => Err(std::io::Error::last_os_error()),
        r => Err(std::io::Error::other(format!(
            "WaitForSingleObject(), unexpected return value \"{r:x}\""
        ))),
    }
}

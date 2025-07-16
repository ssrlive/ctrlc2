use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use crate::{Error, set_handler};

/// A future which is fulfilled when the program receives the Ctrl+C signal.
#[derive(Debug)]
pub struct AsyncCtrlC {
    waker: Arc<Mutex<Option<Waker>>>,
    active: Arc<AtomicBool>,
}

impl Future for AsyncCtrlC {
    type Output = std::io::Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // check if the signal has already been activated
        if self.active.load(Ordering::SeqCst) {
            Poll::Ready(Ok(()))
        } else {
            // set the waker so that it can be woken up when the signal is triggered
            {
                let mut waker_guard = self.waker.lock().map_err(|e| std::io::Error::other(format!("acquire lock: {e}")))?;
                *waker_guard = Some(cx.waker().clone());
            }

            // check the status again to avoid race conditions if it was activated while setting the waker
            if self.active.load(Ordering::SeqCst) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
    }
}

static INSTANCE_CREATED: AtomicBool = AtomicBool::new(false);

impl AsyncCtrlC {
    /// Creates a new `AsyncCtrlC` future.
    ///
    /// There should be at most one `AsyncCtrlC` instance in the whole program. The
    /// second call to `AsyncCtrlC::new()` would return an error.
    ///
    /// The `user_handler` function is customizable and the return boolean value
    /// is indicating whether the user agreed terminate the program or not.
    pub fn new<F>(mut user_handler: F) -> std::io::Result<Self>
    where
        F: FnMut() -> bool + 'static + Send,
    {
        if INSTANCE_CREATED.load(Ordering::SeqCst) {
            return Err(Error::MultipleHandlers.into());
        }
        INSTANCE_CREATED.store(true, Ordering::SeqCst);

        let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let active = Arc::new(AtomicBool::new(false));

        let waker_clone = waker.clone();
        let active_clone = active.clone();

        set_handler(move || {
            let handled = user_handler();
            if handled {
                active_clone.store(true, Ordering::SeqCst);
                if let Ok(mut waker_guard) = waker_clone.lock() {
                    if let Some(waker) = waker_guard.take() {
                        waker.wake();
                    }
                }
            }
            handled
        })?;
        Ok(AsyncCtrlC { waker, active })
    }
}

impl Drop for AsyncCtrlC {
    fn drop(&mut self) {
        // When AsyncCtrlC is dropped, reset the instance created flag
        // This allows new instances to be created
        INSTANCE_CREATED.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_async_ctrlc() {
        if cfg!(windows) && std::env::var("CI").is_ok() {
            println!("Skipping test_async_ctrlc in CI environment on Windows");
            return;
        }

        let cancel_token = tokio_util::sync::CancellationToken::new();
        let cancel_token_clone = cancel_token.clone();

        let ctrlc_future = crate::AsyncCtrlC::new(move || {
            println!("Ctrl+C received, cancelling...");
            cancel_token_clone.cancel();
            true
        })
        .unwrap();

        // Simulate a Ctrl+C signal after a short delay
        let fire_signal = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Send Ctrl+C signal
            #[cfg(unix)]
            nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT).unwrap();

            #[cfg(windows)]
            {
                // Since Windows API `GenerateConsoleCtrlEvent` will cause all parallel test processes exit,
                // so we can't run this code on CI environment.
                // If you want to test it, run it on your local Windows machine.
                use windows_sys::Win32::System::Console::{CTRL_C_EVENT, GenerateConsoleCtrlEvent};
                unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) };
            }

            println!("Ctrl+C signal sent");
        });

        let main_worker = tokio::spawn(async move {
            // Here goes the main logic of your application and wait for cancellation token
            println!("[Main worker] started, till the cancellation token is received...");
            cancel_token.cancelled().await;
            println!("[Main worker] cancelled, exiting...");
        });

        // Wait for the future to complete
        // This will block until the Ctrl+C signal is received
        ctrlc_future.await.unwrap();

        fire_signal.await.unwrap();
        main_worker.await.unwrap();

        println!("Test completed successfully.");
    }
}

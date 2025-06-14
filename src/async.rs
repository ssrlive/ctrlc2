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
        if self.active.swap(false, Ordering::SeqCst) {
            Poll::Ready(Ok(()))
        } else {
            let mut waker_guard = self.waker.lock().map_err(|e| std::io::Error::other(format!("{e}")))?;
            *waker_guard = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

static INSTANCE_CREATED: AtomicBool = AtomicBool::new(false);

impl AsyncCtrlC {
    /// Creates a new `AsyncCtrlC` future.
    ///
    /// There should be at most one `AsyncCtrlC` instance in the whole program. The
    /// second call to `AsyncCtrlC::new()` would return an error.
    pub fn new<F>(mut user_handler: F) -> std::io::Result<Self>
    where
        F: FnMut() -> bool + 'static + Send,
    {
        if INSTANCE_CREATED.swap(true, Ordering::SeqCst) {
            return Err(Error::MultipleHandlers.into());
        }

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

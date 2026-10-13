//! Cooperative cancellation scoped to one synchronous analysis call.

use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cancelled;

thread_local! {
    static CURRENT: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

pub(crate) fn current() -> Option<Arc<AtomicBool>> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Families retain their pure public APIs. A controlled caller owns this scope,
/// checks its result, and cannot return the unfinished collection after a stop.
pub(crate) fn run<T>(
    flag: Option<Arc<AtomicBool>>,
    work: impl FnOnce() -> T,
) -> Result<T, Cancelled> {
    struct Restore(Option<Arc<AtomicBool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|current| *current.borrow_mut() = self.0.take());
        }
    }
    let flag = flag.or_else(current);
    let _restore = Restore(CURRENT.with(|current| current.replace(flag)));
    if cancelled() {
        return Err(Cancelled);
    }
    let value = work();
    if cancelled() {
        Err(Cancelled)
    } else {
        Ok(value)
    }
}

pub(crate) fn flag_cancelled(flag: Option<&AtomicBool>) -> bool {
    #[cfg(test)]
    test_checkpoint();
    flag.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

pub(crate) fn cancelled() -> bool {
    CURRENT.with(|current| flag_cancelled(current.borrow().as_deref()))
}

#[cfg(test)]
thread_local! {
    static STOP_AFTER: RefCell<Option<(usize, Arc<AtomicBool>)>> = const { RefCell::new(None) };
}

#[cfg(test)]
fn test_checkpoint() {
    STOP_AFTER.with(|stop| {
        let mut stop = stop.borrow_mut();
        if let Some((remaining, flag)) = stop.as_mut() {
            if *remaining == 0 {
                flag.store(true, Ordering::Relaxed);
                *stop = None;
            } else {
                *remaining -= 1;
            }
        }
    });
}

/// Deterministic test stop after real worker checkpoints, independent of clocks.
#[cfg(test)]
pub(crate) fn cancel_after(flag: &Arc<AtomicBool>, checks: usize) {
    STOP_AFTER.with(|stop| *stop.borrow_mut() = Some((checks, Arc::clone(flag))));
}

//! Allocation accounting for the disabled MCP retention experiment.
//! Owners count live requested allocation bytes, including tracking layout.
//! This is neither an allocator-fragmentation estimate nor a process RSS cap.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::ptr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

static INSTALLED: AtomicBool = AtomicBool::new(false);
static ACCOUNTING_FAILED: AtomicBool = AtomicBool::new(false);
// Ordinary LSP work has no scoped allocation owner. Keep its headers for
// correct later free/realloc, but avoid initializing/reading owner TLS.
static ACTIVE_SCOPES: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static CURRENT: RefCell<Option<Arc<Owner>>> = const { RefCell::new(None) };
}

#[derive(Default)]
pub(crate) struct Owner {
    live: AtomicUsize,
    peak: AtomicUsize,
    overflowed: AtomicBool,
}

impl Owner {
    pub(crate) fn new() -> Arc<Self> {
        // An owner must not charge its own Arc to itself or another owner.
        let previous = CURRENT.with(|slot| slot.replace(None));
        let owner = Arc::new(Self::default());
        CURRENT.with(|slot| slot.replace(previous));
        owner
    }

    pub(crate) fn bytes(&self) -> usize {
        // Pessimistic allowance for this ledger's own Arc/control allocation.
        if self.overflowed.load(Ordering::Relaxed) {
            usize::MAX
        } else {
            self.live.load(Ordering::Relaxed).saturating_add(256)
        }
    }

    pub(crate) fn peak_bytes(&self) -> usize {
        self.peak.load(Ordering::Relaxed).saturating_add(256)
    }

    fn add(&self, bytes: usize) {
        let before = self
            .live
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
                Some(live.saturating_add(bytes))
            })
            .unwrap_or(usize::MAX);
        let Some(live) = before.checked_add(bytes) else {
            self.overflowed.store(true, Ordering::Relaxed);
            return;
        };
        self.peak.fetch_max(live, Ordering::Relaxed);
    }
}

/// Thread-bound scope. TLS owns the current Arc and this guard owns its predecessor.
pub(crate) struct Scope {
    previous: Option<Arc<Owner>>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Scope {
    pub(crate) fn enter(owner: Arc<Owner>) -> Self {
        ACTIVE_SCOPES.fetch_add(1, Ordering::Relaxed);
        let previous = CURRENT.with(|slot| slot.replace(Some(owner)));
        Self {
            previous,
            _thread_bound: std::marker::PhantomData,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|slot| slot.replace(self.previous.take()));
        ACTIVE_SCOPES.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) fn select(owner: Arc<Owner>) {
    CURRENT.with(|slot| slot.replace(Some(owner)));
}

pub(crate) fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed) && !ACCOUNTING_FAILED.load(Ordering::Relaxed)
}

#[repr(C)]
struct Header {
    owner: *const Owner,
}

fn tracked_layout(layout: Layout) -> Option<(Layout, usize)> {
    Layout::new::<Header>()
        .extend(layout)
        .ok()
        .map(|(layout, offset)| (layout.pad_to_align(), offset))
}

/// Installed by the experimental binary, never by ordinary library consumers.
pub struct TrackingAllocator;

// SAFETY: each returned allocation carries one fixed header at its System base.
// The extended Layout preserves the caller's alignment. The same original Layout
// recovers that base on free/realloc. Each tracked header owns one Arc strong
// reference; its counter is atomic so frees can occur on another thread.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: alloc_with_header receives the valid caller Layout unchanged.
        unsafe { allocate(layout, false) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the entire extended allocation is zeroed before its header is set.
        unsafe { allocate(layout, true) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let Some((combined, offset)) = tracked_layout(layout) else {
            return;
        };
        // SAFETY: this pointer came from allocate with the same original Layout.
        let base = unsafe { pointer.sub(offset) };
        let owner = unsafe { (*base.cast::<Header>()).owner };
        if !owner.is_null() {
            // SAFETY: the header owns a strong reference until after System free.
            unsafe {
                (*owner).live.fetch_sub(combined.size(), Ordering::Relaxed);
            }
        }
        unsafe {
            System.dealloc(base, combined);
        }
        if !owner.is_null() {
            // SAFETY: release exactly the strong reference acquired at allocation.
            unsafe {
                Arc::decrement_strong_count(owner);
            }
        }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        let Some((old, offset)) = tracked_layout(layout) else {
            return ptr::null_mut();
        };
        let Some((new, new_offset)) = tracked_layout(new_layout) else {
            return ptr::null_mut();
        };
        // SAFETY: the old Layout recovers the allocation base; realloc preserves
        // its header and alignment. On failure the old allocation stays valid.
        let base = unsafe { pointer.sub(offset) };
        let owner = unsafe { (*base.cast::<Header>()).owner };
        let resized = unsafe { System.realloc(base, old, new.size()) };
        if resized.is_null() {
            return resized;
        }
        if !owner.is_null() {
            if new.size() >= old.size() {
                unsafe {
                    (*owner).add(new.size() - old.size());
                }
            } else {
                unsafe {
                    (*owner)
                        .live
                        .fetch_sub(old.size() - new.size(), Ordering::Relaxed);
                }
            }
        }
        unsafe { resized.add(new_offset) }
    }
}

unsafe fn allocate(layout: Layout, zeroed: bool) -> *mut u8 {
    if !INSTALLED.load(Ordering::Relaxed) {
        INSTALLED.store(true, Ordering::Relaxed);
    }
    let Some((combined, offset)) = tracked_layout(layout) else {
        return ptr::null_mut();
    };
    // SAFETY: combined is a nonzero valid Layout, including its header.
    let base = unsafe {
        if zeroed {
            System.alloc_zeroed(combined)
        } else {
            System.alloc(combined)
        }
    };
    if base.is_null() {
        return base;
    }
    // TLS access has a constant, allocation-free initializer. During thread
    // teardown, unavailable TLS means an untracked allocation, not a panic.
    // This thread's live Scope contributes to ACTIVE_SCOPES before installing
    // its owner and until after restoring TLS. A zero count therefore proves
    // this allocation has no owner; other threads can only make us take the
    // conservative TLS path. Headers and their original ownership stay intact.
    let owner = if ACTIVE_SCOPES.load(Ordering::Relaxed) == 0 {
        ptr::null()
    } else {
        CURRENT
            .try_with(|slot| match slot.try_borrow() {
                Ok(current) => current.as_ref().map_or(ptr::null(), Arc::as_ptr),
                Err(_) => {
                    ACCOUNTING_FAILED.store(true, Ordering::Relaxed);
                    ptr::null()
                }
            })
            .unwrap_or(ptr::null())
    };
    if !owner.is_null() {
        // SAFETY: an active Scope keeps this owner alive; the header acquires
        // its own strong reference for use after that scope/thread finishes.
        unsafe {
            Arc::increment_strong_count(owner);
            (*owner).add(combined.size());
        }
    }
    unsafe {
        base.cast::<Header>().write(Header { owner });
        base.add(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_realloc_and_cross_thread_free_keep_the_original_owner() {
        // Guards charging capacity rather than length, lost realloc ownership,
        // and frees incorrectly charged to their current thread/scope.
        let owner = Owner::new();
        let base = owner.bytes();
        let mut data = {
            let _scope = Scope::enter(Arc::clone(&owner));
            Vec::<u8>::with_capacity(4096)
        };
        assert!(owner.bytes() >= base + 4096);
        data.reserve_exact(16384);
        assert!(owner.bytes() >= base + data.capacity());
        std::thread::spawn(move || drop(data)).join().unwrap();
        assert_eq!(owner.bytes(), base);
        assert!(owner.peak_bytes() >= base + 16384);
    }

    #[test]
    fn alignment_zeroing_and_shared_ownership_are_accounted() {
        // Guards header padding corrupting highly aligned allocations, or a
        // shared allocation disappearing from accounting before its last owner.
        let owner = Owner::new();
        let base = owner.bytes();
        let layout = Layout::from_size_align(32768, 4096).unwrap();
        let pointer = {
            let _scope = Scope::enter(Arc::clone(&owner));
            unsafe { std::alloc::alloc_zeroed(layout) }
        };
        assert!(!pointer.is_null());
        assert_eq!(pointer as usize % 4096, 0);
        assert!(
            unsafe { std::slice::from_raw_parts(pointer, layout.size()) }
                .iter()
                .all(|byte| *byte == 0)
        );
        let pointer = unsafe { std::alloc::realloc(pointer, layout, 65536) };
        assert!(!pointer.is_null());
        assert_eq!(pointer as usize % 4096, 0);
        assert!(
            unsafe { std::slice::from_raw_parts(pointer, layout.size()) }
                .iter()
                .all(|byte| *byte == 0)
        );
        unsafe {
            std::alloc::dealloc(pointer, Layout::from_size_align(65536, 4096).unwrap());
        }
        assert_eq!(owner.bytes(), base);
        let shared = {
            let _scope = Scope::enter(Arc::clone(&owner));
            Arc::new(vec![1u8; 65536])
        };
        let copy = Arc::clone(&shared);
        let charged = owner.bytes();
        drop(shared);
        assert_eq!(owner.bytes(), charged);
        drop(copy);
        assert_eq!(owner.bytes(), base);
    }

    #[test]
    fn owner_creation_and_nested_scopes_restore_allocation_ownership() {
        // Guards ledger/control construction or a nested worker stealing the
        // outer worker's subsequent allocations. Unowned realloc stays unowned.
        let unowned = Vec::<u8>::with_capacity(100);
        let outer = Owner::new();
        let inner = {
            let _scope = Scope::enter(Arc::clone(&outer));
            let inner = Owner::new();
            assert_eq!(outer.bytes(), 256);
            {
                let _nested = Scope::enter(Arc::clone(&inner));
                drop(vec![0u8; 4096]);
            }
            let mut unowned = unowned;
            unowned.reserve_exact(10000);
            assert_eq!(outer.bytes(), 256);
            let owned = vec![0u8; 1024];
            assert!(outer.bytes() > 256);
            drop(owned);
            inner
        };
        assert_eq!(outer.bytes(), 256);
        assert_eq!(inner.bytes(), 256);
        let owner = Owner::default();
        owner.add(usize::MAX);
        owner.add(1);
        assert_eq!(owner.bytes(), usize::MAX);
    }
}

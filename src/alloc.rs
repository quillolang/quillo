//! The program's memory allocator: mimalloc on the event loops and the
//! threads that wait for the server (database runtime, waiting pool), the
//! system's elsewhere (the main thread, before the server starts).
//!
//! What the waiting threads make, such as rows read from the database, is
//! mostly freed on an event loop; freeing on another thread is cheap in
//! mimalloc and costly in the system allocator. On the event loops,
//! measured on macOS (bench/server), mimalloc answers 5% more small
//! requests a second than the system allocator, and 7% more pages that
//! build lists and HTML. Each block is freed by the allocator that made it;
//! mimalloc knows its own (`mi_is_in_heap_region`).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;

thread_local! {
    static WAITER: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread (an event loop, or one that waits for the
/// server): its allocations come from mimalloc from now on.
pub fn waiter_thread() {
    WAITER.with(|w| w.set(true));
}

pub struct Split;

fn mimalloc_owns(p: *mut u8) -> bool {
    // SAFETY: only reads mimalloc's map of the memory it reserved.
    unsafe { libmimalloc_sys::mi_is_in_heap_region(p as *const c_void) }
}

// SAFETY: every block goes back to the allocator it came from, with the
// layout it was made with, as `GlobalAlloc` requires of each.
unsafe impl GlobalAlloc for Split {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if WAITER.with(|w| w.get()) {
            libmimalloc_sys::mi_malloc_aligned(l.size(), l.align()) as *mut u8
        } else {
            System.alloc(l)
        }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if WAITER.with(|w| w.get()) {
            libmimalloc_sys::mi_zalloc_aligned(l.size(), l.align()) as *mut u8
        } else {
            System.alloc_zeroed(l)
        }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if mimalloc_owns(p) {
            libmimalloc_sys::mi_free(p as *mut c_void)
        } else {
            System.dealloc(p, l)
        }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        if mimalloc_owns(p) {
            libmimalloc_sys::mi_realloc_aligned(p as *mut c_void, size, l.align()) as *mut u8
        } else {
            System.realloc(p, l, size)
        }
    }
}

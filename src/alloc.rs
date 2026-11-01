//! The program's memory allocator: mimalloc, on every thread.
//!
//! What the waiting threads make, such as rows read from the database, is
//! mostly freed on an event loop; freeing on another thread is cheap in
//! mimalloc and costly in the system allocator. On the event loops,
//! measured on macOS (quillolang/benchmarks, server/), mimalloc answers 5% more small
//! requests a second than the system allocator, and 7% more pages that
//! build lists and HTML. A script makes and frees everything on one thread, and on
//! macOS the system's allocator cost it a tenth of its time. Having both, with each
//! block freed by the one that made it, cost an eighth of the time of reading JSON
//! in asking which that was (FEEDBACK29.md; branch feedback29-fixes).

use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;

pub struct Mi;

// SAFETY: every block comes from mimalloc and goes back to it, with the
// alignment it was made with.
unsafe impl GlobalAlloc for Mi {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        libmimalloc_sys::mi_malloc_aligned(l.size(), l.align()) as *mut u8
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        libmimalloc_sys::mi_zalloc_aligned(l.size(), l.align()) as *mut u8
    }

    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, _: Layout) {
        libmimalloc_sys::mi_free(p as *mut c_void)
    }

    #[inline]
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        libmimalloc_sys::mi_realloc_aligned(p as *mut c_void, size, l.align()) as *mut u8
    }
}

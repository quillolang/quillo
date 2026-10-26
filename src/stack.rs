//! Room on the stack for deep recursion, on ordinary threads and in the
//! server's coroutines.
//!
//! A Quillo call checks that enough stack is left before it goes deeper; if
//! not, it continues on a new piece of stack (`maybe_grow`). How much is
//! left depends on where the current stack ends, which this module tracks
//! per thread: an ordinary thread's own stack, or the stack of the coroutine
//! it is running, which the server sets when it switches (`switch`).

use corosensei::stack::{DefaultStack, Stack};
use std::cell::Cell;

thread_local! {
    /// The lowest address the current stack may use; 0 until first needed.
    static LIMIT: Cell<usize> = const { Cell::new(0) };
}

/// Stack one level of nesting in a function's body may take while it is
/// evaluated (see `parser::MAX_NESTING`): far more in a debug build.
pub const PER_LEVEL: usize = if cfg!(debug_assertions) { 24 << 10 } else { 8 << 10 };

/// Runs `f` with room for code nested `levels` deep: on a new piece of
/// stack if this one has too little left.
pub fn for_nesting<R>(levels: u32, f: impl FnOnce() -> R) -> R {
    let need = (128 << 10).max(levels as usize * PER_LEVEL);
    maybe_grow(need, (4 << 20).max(need + (256 << 10)), f)
}

/// Kept free above a stack's end: its guard page, and room for code that
/// does not check (builtins, serialization).
pub const MARGIN: usize = 64 << 10;

fn limit() -> usize {
    LIMIT.with(|l| {
        if l.get() == 0 {
            let sp = psm::stack_pointer() as usize;
            if let Some(left) = stacker::remaining_stack() {
                l.set(sp.saturating_sub(left));
            }
        }
        l.get()
    })
}

/// Bytes left on the current stack, before its end (and `MARGIN`), when
/// known.
pub fn remaining() -> Option<usize> {
    let end = limit();
    (end != 0).then(|| (psm::stack_pointer() as usize).saturating_sub(end))
}

/// Makes `limit` the current stack's end (when switching to or from a
/// coroutine); returns the previous one.
pub fn switch(limit: usize) -> usize {
    LIMIT.with(|l| l.replace(limit))
}

/// The end a coroutine's stack starts with.
pub fn limit_of(stack: &DefaultStack) -> usize {
    stack.limit().get() + MARGIN
}

/// Runs `f`, on a new piece of stack of `size` bytes if fewer than
/// `red_zone` are left on this one.
pub fn maybe_grow<R>(red_zone: usize, size: usize, f: impl FnOnce() -> R) -> R {
    let sp = psm::stack_pointer() as usize;
    let end = limit();
    if end != 0 && sp >= end + red_zone {
        return f();
    }
    grow(size, f)
}

#[cfg(not(windows))]
fn grow<R>(size: usize, f: impl FnOnce() -> R) -> R {
    let stack = DefaultStack::new(size + MARGIN).expect("cannot allocate a stack");
    let low = limit_of(&stack);
    let high = stack.base().get();
    let outer = switch(low);
    let mut f = Some(f);
    let mut out = None;
    // SAFETY: `low..high` is memory of `stack`, which outlives the call.
    unsafe {
        psm::on_stack(low as *mut u8, high - low, || {
            let f = f.take().expect("called once");
            out = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
        });
    }
    switch(outer);
    drop(stack);
    match out.expect("ran") {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// On Windows, where stacks are switched with fibers, `stacker` does the
/// growing; meanwhile the end of the stack is whatever it says.
#[cfg(windows)]
fn grow<R>(size: usize, f: impl FnOnce() -> R) -> R {
    let outer = switch(0);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stacker::grow(size, || {
            switch(0);
            f()
        })
    }));
    switch(outer);
    match r {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
}

#[cfg(test)]
mod tests {
    fn depth(n: u64) -> u64 {
        super::maybe_grow(64 << 10, 1 << 20, || if n == 0 { 0 } else { 1 + depth(n - 1) })
    }

    #[test]
    fn deep_recursion_grows_the_stack() {
        // Far deeper than a thread's own stack would allow.
        assert_eq!(depth(200_000), 200_000);
    }
}

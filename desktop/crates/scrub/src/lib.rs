//! Memory hygiene for a process that must not outlive its secrets (REC-15, ADR-0017).
//!
//! The only crate in the desktop workspace that contains `unsafe`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::ptr;

use zeroize::Zeroize;

/// How much dead stack `scrub_stack` overwrites. It must stay well below the smallest thread
/// stack the process runs on (2 MiB for spawned threads).
pub const STACK_SCRUB_BYTES: usize = 512 * 1024;

/// The system allocator, except that every block is wiped before it is returned to it.
///
/// Rust and its dependencies free secret-bearing buffers (certificates, QR payloads, framing
/// copies) without wiping them; the freed bytes would otherwise sit in the heap until reused.
pub struct ZeroizingAllocator;

// SAFETY: every method delegates to `System` with the caller's layout. The wipe writes only
// inside `[ptr, ptr + layout.size())`, which the caller owns until it is freed here.
unsafe impl GlobalAlloc for ZeroizingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract for `layout`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: `block` was allocated here with `layout`, so it is valid for `layout.size()`.
        unsafe {
            wipe(block, layout.size());
            System.dealloc(block, layout);
        }
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A moving `realloc` would leave the old block unwiped, so always move explicitly.
        // SAFETY: `new_size` is validated by the caller per `GlobalAlloc::realloc`; the copy
        // stays inside both blocks; `block` is freed once, with its original layout.
        unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            let new_block = System.alloc(new_layout);
            if new_block.is_null() {
                return new_block;
            }
            ptr::copy_nonoverlapping(block, new_block, layout.size().min(new_size));
            self.dealloc(block, layout);
            new_block
        }
    }
}

/// # Safety
/// `block` must be valid for writes of `len` bytes.
unsafe fn wipe(block: *mut u8, len: usize) {
    if len == 0 {
        return;
    }
    // SAFETY: the caller guarantees `block` is valid for `len` writes.
    unsafe { std::slice::from_raw_parts_mut(block, len) }.zeroize();
}

/// Overwrites the stack below the caller, where finished frames left copies of their locals.
///
/// Call it from the thread that ran the session, after the session value has been dropped.
#[inline(never)]
pub fn scrub_stack() {
    let mut dead = [0_u8; STACK_SCRUB_BYTES];
    dead.zeroize();
    black_box(&dead);
}

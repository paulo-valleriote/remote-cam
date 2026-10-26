use std::alloc::{GlobalAlloc, Layout};

use remotecam_scrub::{STACK_SCRUB_BYTES, ZeroizingAllocator, scrub_stack};

const ALLOCATOR: ZeroizingAllocator = ZeroizingAllocator;

fn layout(size: usize) -> Layout {
    Layout::from_size_align(size, 8).expect("layout")
}

#[test]
fn allocates_and_frees_usable_memory() {
    let layout = layout(128);
    // SAFETY: the block is written within its size and freed once with its own layout.
    unsafe {
        let block = ALLOCATOR.alloc(layout);
        assert!(!block.is_null());
        block.write_bytes(0xAB, 128);
        assert_eq!(block.add(127).read(), 0xAB);
        ALLOCATOR.dealloc(block, layout);
    }
}

#[test]
fn allocates_zeroed_memory() {
    let layout = layout(64);
    // SAFETY: the block is read within its size and freed once with its own layout.
    unsafe {
        let block = ALLOCATOR.alloc_zeroed(layout);
        assert!((0..64).all(|offset| block.add(offset).read() == 0));
        ALLOCATOR.dealloc(block, layout);
    }
}

#[test]
fn growing_a_block_keeps_its_contents() {
    let old = layout(16);
    // SAFETY: each pointer is used within its size and freed once with the layout it holds.
    unsafe {
        let block = ALLOCATOR.alloc(old);
        for offset in 0..16 {
            block.add(offset).write(offset as u8);
        }
        let grown = ALLOCATOR.realloc(block, old, 4096);
        assert!(!grown.is_null());
        assert!((0..16).all(|offset| grown.add(offset).read() == offset as u8));
        ALLOCATOR.dealloc(grown, layout(4096));
    }
}

#[test]
fn shrinking_a_block_keeps_the_leading_contents() {
    let old = layout(4096);
    // SAFETY: each pointer is used within its size and freed once with the layout it holds.
    unsafe {
        let block = ALLOCATOR.alloc(old);
        block.write_bytes(0x5A, 4096);
        let shrunk = ALLOCATOR.realloc(block, old, 32);
        assert!((0..32).all(|offset| shrunk.add(offset).read() == 0x5A));
        ALLOCATOR.dealloc(shrunk, layout(32));
    }
}

#[test]
fn scrubbing_the_stack_fits_a_default_thread_stack() {
    std::thread::Builder::new()
        .stack_size(2 * STACK_SCRUB_BYTES)
        .spawn(scrub_stack)
        .expect("thread")
        .join()
        .expect("the scrub stays within the stack");
}

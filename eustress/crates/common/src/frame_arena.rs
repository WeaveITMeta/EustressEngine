//! A bump allocator for scratch collections a system rebuilds every frame.
//!
//! A system keeps one in a `Local<FrameArena>`, calls [`FrameArena::reset`] at
//! the top of each run, and builds its temporary collections with
//! `Vec::new_in(&*arena)`. Allocation is a pointer bump; freeing is a no-op;
//! `reset` reclaims everything at once. After the first few frames the arena
//! holds one block as large as the busiest frame needed, so a steady frame
//! makes no calls to the global allocator at all.
//!
//! `reset` takes `&mut self`, so the borrow checker proves no collection
//! built in the arena outlives the frame that built it.
//!
//! Only for collections that stay inside the system. Anything stored in a
//! component, a resource or an event must use the global allocator.

use std::alloc::{AllocError, Allocator, Layout};
use std::cell::{Cell, UnsafeCell};
use std::ptr::NonNull;

/// Alignment of every block the arena takes from the global allocator.
const BLOCK_ALIGN: usize = 16;
/// The first block's size, before the arena learns a frame's real need.
const FIRST_BLOCK: usize = 4 * 1024;

struct Block {
    ptr: NonNull<u8>,
    size: usize,
}

impl Block {
    fn new(size: usize) -> Block {
        let layout = Layout::from_size_align(size, BLOCK_ALIGN).expect("frame arena block layout");
        // SAFETY: `size` is never zero (see `FrameArena::grow_into`).
        let ptr = unsafe { std::alloc::alloc(layout) };
        let Some(ptr) = NonNull::new(ptr) else { std::alloc::handle_alloc_error(layout) };
        Block { ptr, size }
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        // SAFETY: allocated in `Block::new` with this exact layout.
        unsafe {
            std::alloc::dealloc(self.ptr.as_ptr(), Layout::from_size_align_unchecked(self.size, BLOCK_ALIGN))
        };
    }
}

/// A per-system scratch arena. See the module docs.
pub struct FrameArena {
    /// Every block taken this frame; allocation bumps through the last one.
    /// Only `alloc_in`/`grow_into` push here, and never while a reference
    /// into the Vec is alive; earlier blocks' memory never moves.
    blocks: UnsafeCell<Vec<Block>>,
    /// Bytes used in the last block.
    used: Cell<usize>,
    /// Start offset of the most recent allocation in the last block, so the
    /// newest allocation can grow or be freed in place.
    last: Cell<usize>,
}

// SAFETY: the arena owns its blocks outright; moving it to another thread
// moves that ownership. It is not `Sync` (the `Cell`s), so `&FrameArena`,
// and with it every collection built in it, stays on one thread.
unsafe impl Send for FrameArena {}

impl Default for FrameArena {
    fn default() -> Self {
        FrameArena { blocks: UnsafeCell::new(Vec::new()), used: Cell::new(0), last: Cell::new(usize::MAX) }
    }
}

impl FrameArena {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reclaim every allocation. When last frame spilled into more than one
    /// block, they become one block big enough for all of it, so next frame
    /// fits without spilling.
    pub fn reset(&mut self) {
        let blocks = self.blocks.get_mut();
        if blocks.len() > 1 {
            let total: usize = blocks.iter().map(|b| b.size).sum();
            blocks.clear();
            blocks.push(Block::new(total));
        }
        self.used.set(0);
        self.last.set(usize::MAX);
    }

    /// Bytes the arena holds from the global allocator.
    pub fn capacity(&self) -> usize {
        // SAFETY: shared read; no `&mut` to the Vec exists outside the
        // allocation methods, which do not run concurrently (`!Sync`).
        unsafe { &*self.blocks.get() }.iter().map(|b| b.size).sum()
    }

    /// Blocks held, for tests.
    pub fn block_count(&self) -> usize {
        // SAFETY: as in `capacity`.
        unsafe { &*self.blocks.get() }.len()
    }

    /// Bump-allocate `layout` in the last block, or in a new one.
    fn alloc_in(&self, layout: Layout) -> NonNull<[u8]> {
        if layout.size() == 0 {
            // A dangling pointer at the requested alignment is a valid
            // zero-sized allocation.
            let ptr = NonNull::<u8>::without_provenance(std::num::NonZero::new(layout.align()).expect("align"));
            return NonNull::slice_from_raw_parts(ptr, 0);
        }
        if let Some(p) = self.try_bump(layout) {
            return p;
        }
        self.grow_into(layout);
        self.try_bump(layout).expect("a fresh block fits the allocation it was sized for")
    }

    fn try_bump(&self, layout: Layout) -> Option<NonNull<[u8]>> {
        // SAFETY: shared read, as in `capacity`.
        let block = unsafe { &*self.blocks.get() }.last()?;
        let base = block.ptr.as_ptr() as usize;
        let start = (base + self.used.get()).checked_next_multiple_of(layout.align())? - base;
        let end = start.checked_add(layout.size())?;
        if end > block.size {
            return None;
        }
        self.used.set(end);
        self.last.set(start);
        // SAFETY: `start..end` lies inside the block.
        let ptr = unsafe { block.ptr.add(start) };
        Some(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }

    /// Start a new block big enough for `layout`, at least double the last.
    fn grow_into(&self, layout: Layout) {
        // SAFETY: no reference into the Vec is alive here; earlier blocks'
        // memory is separate heap memory and does not move when the Vec does.
        let blocks = unsafe { &mut *self.blocks.get() };
        let prev = blocks.last().map_or(FIRST_BLOCK / 2, |b| b.size);
        let need = layout.size() + layout.align().saturating_sub(BLOCK_ALIGN);
        let size = (prev * 2).max(need).max(FIRST_BLOCK);
        blocks.push(Block::new(size));
        self.used.set(0);
        self.last.set(usize::MAX);
    }

    /// Whether `ptr` is the newest allocation in the last block.
    fn is_last(&self, ptr: NonNull<u8>) -> bool {
        // SAFETY: shared read, as in `capacity`.
        let Some(block) = (unsafe { &*self.blocks.get() }).last() else { return false };
        let last = self.last.get();
        last != usize::MAX && ptr.as_ptr() as usize == block.ptr.as_ptr() as usize + last
    }
}

// SAFETY: blocks handed out stay valid until `reset` (which needs `&mut`, so
// no `&FrameArena` and no collection built in it can be alive) or drop.
// Allocations never overlap: each bump starts at or after the previous end.
unsafe impl Allocator for &FrameArena {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        Ok(self.alloc_in(layout))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        // The newest allocation gives its bytes back; everything else waits
        // for `reset`.
        if layout.size() != 0 && self.is_last(ptr) {
            self.used.set(self.last.get());
            self.last.set(usize::MAX);
        }
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        // A Vec pushing past its capacity is almost always the newest
        // allocation: extend it in place when the block has room.
        if old.size() != 0 && new.align() <= old.align() && self.is_last(ptr) {
            // SAFETY: shared read, as in `capacity`.
            let block = unsafe { &*self.blocks.get() }.last().expect("is_last implies a block");
            let start = self.last.get();
            if let Some(end) = start.checked_add(new.size()).filter(|&e| e <= block.size) {
                self.used.set(end);
                return Ok(NonNull::slice_from_raw_parts(ptr, new.size()));
            }
        }
        let fresh = self.alloc_in(new);
        // SAFETY: the caller guarantees `ptr` holds `old.size()` initialised
        // bytes; `fresh` is a distinct allocation of at least that many.
        unsafe { std::ptr::copy_nonoverlapping(ptr.as_ptr(), fresh.cast::<u8>().as_ptr(), old.size()) };
        Ok(fresh)
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        if new.align() <= old.align() {
            if self.is_last(ptr) && new.size() != 0 {
                self.used.set(self.last.get() + new.size());
            }
            return Ok(NonNull::slice_from_raw_parts(ptr, new.size()));
        }
        let fresh = self.alloc_in(new);
        // SAFETY: as in `grow`, copying the smaller size.
        unsafe { std::ptr::copy_nonoverlapping(ptr.as_ptr(), fresh.cast::<u8>().as_ptr(), new.size()) };
        Ok(fresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_in_the_arena_holds_its_values_across_growth() {
        let arena = FrameArena::new();
        let mut v: Vec<u64, &FrameArena> = Vec::new_in(&arena);
        for i in 0..10_000u64 {
            v.push(i * 3);
        }
        assert!(v.iter().enumerate().all(|(i, &x)| x == i as u64 * 3));
    }

    #[test]
    fn allocations_are_aligned_and_do_not_overlap() {
        let arena = FrameArena::new();
        let a = &arena;
        let mut spans = Vec::new();
        for (size, align) in [(1, 1), (3, 2), (8, 8), (5, 1), (64, 64), (24, 8), (1, 4096), (7, 16)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let p = a.allocate(layout).unwrap().cast::<u8>().as_ptr() as usize;
            assert_eq!(p % align, 0, "size {size} align {align}");
            spans.push((p, p + size));
        }
        spans.sort();
        assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "{spans:?}");
    }

    #[test]
    fn two_live_vecs_keep_separate_contents() {
        let arena = FrameArena::new();
        let mut a: Vec<u32, &FrameArena> = Vec::new_in(&arena);
        let mut b: Vec<u32, &FrameArena> = Vec::new_in(&arena);
        for i in 0..5_000 {
            a.push(i);
            b.push(u32::MAX - i);
        }
        assert!(a.iter().enumerate().all(|(i, &x)| x == i as u32));
        assert!(b.iter().enumerate().all(|(i, &x)| x == u32::MAX - i as u32));
    }

    #[test]
    fn the_newest_vec_grows_in_place() {
        let arena = FrameArena::new();
        let mut v: Vec<u8, &FrameArena> = Vec::with_capacity_in(16, &arena);
        let before = v.as_ptr();
        v.extend(0..200u8);
        assert_eq!(v.as_ptr(), before, "grew by copying instead of in place");
        assert_eq!(v[199], 199);
    }

    #[test]
    fn reset_folds_a_spilled_frame_into_one_block() {
        let mut arena = FrameArena::new();
        {
            let mut keep = Vec::new();
            for _ in 0..8 {
                let mut v: Vec<u8, &FrameArena> = Vec::with_capacity_in(3_000, &arena);
                v.push(1);
                keep.push(v);
            }
            assert!(arena.block_count() > 1);
        }
        let held = arena.capacity();
        arena.reset();
        assert_eq!(arena.block_count(), 1);
        assert_eq!(arena.capacity(), held);
        // The same frame again fits in the one block.
        let mut keep = Vec::new();
        for _ in 0..8 {
            keep.push(Vec::<u8, &FrameArena>::with_capacity_in(3_000, &arena));
        }
        assert_eq!(arena.block_count(), 1);
    }

    #[test]
    fn zero_sized_requests_succeed() {
        let arena = FrameArena::new();
        let v: Vec<(), &FrameArena> = (0..10).map(|_| ()).collect::<Vec<_>>().into_iter().fold(Vec::new_in(&arena), |mut v, x| {
            v.push(x);
            v
        });
        assert_eq!(v.len(), 10);
        let e: Vec<u64, &FrameArena> = Vec::new_in(&arena);
        assert!(e.is_empty());
        assert_eq!(arena.block_count(), 0);
    }

    #[test]
    fn freeing_the_newest_allocation_returns_its_bytes() {
        let arena = FrameArena::new();
        let a = {
            let v: Vec<u8, &FrameArena> = Vec::with_capacity_in(100, &arena);
            v.as_ptr()
        };
        let b: Vec<u8, &FrameArena> = Vec::with_capacity_in(100, &arena);
        assert_eq!(a, b.as_ptr());
    }
}

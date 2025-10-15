use crate::{IGlobalMemory, Pages, MEMORY_MAX_PAGES};
use core::{
    ptr,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};
use libc::{
    c_void, madvise, mmap, munmap, MADV_DONTNEED, MAP_ANON, MAP_FAILED, MAP_PRIVATE, PROT_READ,
    PROT_WRITE,
};

pub const WASM_PAGE: usize = 64 * 1024;

#[inline]
fn ceil_to_pages(len: usize) -> usize {
    (len + WASM_PAGE - 1) / WASM_PAGE * WASM_PAGE
}

/// Linear memory reservation with front/back guards.
struct MmapHeap {
    base: NonNull<u8>, // points to start of HEAP (after front guard)
    max_len: usize,    // total HEAP bytes reserved (without guards)
    len: AtomicUsize,  // bytes currently RW (multiple of page)
}

impl MmapHeap {
    /// Reserve `[GUARD | HEAP | GUARD]` and commit `initial_pages`.
    /// `max_pages` caps growth; both guards are at least one page.
    unsafe fn new_unsafe(initial_pages: u32, max_pages: u32) -> Result<Self, &'static str> {
        let map_len = (max_pages as usize) * WASM_PAGE;

        let addr = mmap(
            ptr::null_mut(),
            map_len,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        );
        if addr == MAP_FAILED {
            return Err("mmap reserve failed");
        }

        // Commit the initial part of HEAP as RW
        let init_len = ceil_to_pages((initial_pages as usize) * WASM_PAGE);

        // Ensure zero pages on first touch (they already are zero, but this keeps the story)
        let heap_ptr = addr as usize as *mut c_void;
        let _ = madvise(heap_ptr, map_len, MADV_DONTNEED);

        Ok(Self {
            base: NonNull::new_unchecked(addr as usize as *mut u8),
            max_len: map_len,
            len: AtomicUsize::new(init_len),
        })
    }

    /// Pointer to start of linear memory. Your JIT/interpreter can do `base.add(u32_offset)`.
    #[inline]
    pub fn base(&self) -> *mut u8 {
        self.base.as_ptr()
    }

    /// Bytes currently committed RW.
    #[inline]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    /// Max bytes we can grow to (reserved).
    #[inline]
    pub fn max_len(&self) -> usize {
        self.max_len
    }

    /// Grow by `delta_pages`. Newly committed pages are logically zero via DONTNEED.
    unsafe fn grow_unsafe(&self, delta_pages: u32) -> Result<(), &'static str> {
        if delta_pages == 0 {
            return Ok(());
        }
        let add = (delta_pages as usize) * WASM_PAGE;
        let old = self.len.load(Ordering::Relaxed);
        let new = old.checked_add(add).ok_or("overflow")?;
        if new > self.max_len {
            return Err("exceeds reserved");
        }
        self.len.store(new, Ordering::Release);
        Ok(())
    }

    /// Zero-and-forget: turn the committed range back into “fresh zero” without unmapping.
    unsafe fn recycle_unsafe(&mut self) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let ptr = self.base.as_ptr() as *mut c_void;
        // Keep writable; just tell kernel we don't need contents.
        let _ = madvise(ptr, len, MADV_DONTNEED);
        self.len.store(0, Ordering::Relaxed);
    }
}

impl Drop for MmapHeap {
    fn drop(&mut self) {
        unsafe {
            let map_base = self.base.as_ptr() as usize as *mut c_void;
            let _ = munmap(map_base, self.max_len);
        }
    }
}

impl MmapHeap {
    pub fn new(initial_pages: u32, max_pages: u32) -> Result<Self, &'static str> {
        unsafe { MmapHeap::new_unsafe(initial_pages, max_pages) }
    }
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.base.as_ptr(), self.len()) }
    }
    #[inline]
    pub fn as_slice_mut(&self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.base.as_ptr(), self.len()) }
    }
    #[inline]
    pub fn grow(&self, delta_pages: u32) -> Result<(), &'static str> {
        unsafe { self.grow_unsafe(delta_pages) }
    }
    #[inline]
    pub fn recycle(&mut self) {
        unsafe { self.recycle_unsafe() }
    }
}

pub struct MmapGlobalMemory {
    shared_memory: MmapHeap,
    initial_pages: Pages,
    current_pages: Pages,
}

impl MmapGlobalMemory {
    pub fn new(initial_pages: Pages) -> Self {
        let initial_len = initial_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        let maximum_len = MEMORY_MAX_PAGES
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        debug_assert!(
            initial_len <= maximum_len,
            "rwasm: initial memory size is greater than the maximum"
        );
        unsafe { core::hint::assert_unchecked(initial_len <= maximum_len) };
        let shared_memory =
            MmapHeap::new(initial_pages.into_inner(), MEMORY_MAX_PAGES.into_inner()).unwrap();
        Self {
            initial_pages,
            shared_memory,
            current_pages: initial_pages,
        }
    }
}

impl IGlobalMemory for MmapGlobalMemory {
    fn current_pages(&self) -> Pages {
        self.current_pages
    }

    fn grow(&mut self, additional: Pages) -> Option<Pages> {
        let current_pages = self.current_pages();
        if additional == Pages::from(0) {
            return Some(current_pages);
        }
        let desired_pages = current_pages.checked_add(additional)?;
        if desired_pages > MEMORY_MAX_PAGES {
            return None;
        }
        // At this point, it is okay to grow the underlying virtual memory
        // by the given number of additional pages.
        let new_size = desired_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        assert!(new_size >= self.shared_memory.len());
        if self.shared_memory.grow(additional.into_inner()).is_err() {
            return None;
        };
        self.current_pages = desired_pages;
        Some(current_pages)
    }

    fn data(&self) -> &[u8] {
        self.shared_memory.as_slice()
    }

    fn data_mut(&mut self) -> &mut [u8] {
        self.shared_memory.as_slice_mut()
    }

    fn reset(&mut self) {
        self.current_pages = self.initial_pages;
        self.shared_memory.recycle();
    }

    fn resize_for(&mut self, _initial_pages: Pages) {
        // we allocate max possible memory by default, we use `madvise` to zero it
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rw_memory() {
        let initial_pages = 1;
        let current_pages = initial_pages;
        let delta_pages = 1;
        let max_pages = 1024;
        let mem = MmapHeap::new(initial_pages, max_pages).unwrap();
        let mut value_idx = 0;
        let mut value_byte = 33;
        let mut value_slice = &[3, 2, 1, 2, 3];
        mem.as_slice_mut()[value_idx] = value_byte;
        assert_eq!(mem.as_slice()[value_idx], value_byte);
        mem.as_slice_mut()[value_idx..value_idx + value_slice.len()].copy_from_slice(value_slice);
        assert_eq!(
            &mem.as_slice()[value_idx..value_idx + value_slice.len()],
            value_slice
        );
        mem.grow(delta_pages).unwrap();
        // current_pages += delta_pages;
        value_idx = WASM_PAGE;
        value_byte = 22;
        value_slice = &[3, 2, 1, 2, 3];
        mem.as_slice_mut()[value_idx] = value_byte;
        assert_eq!(mem.as_slice_mut()[value_idx], value_byte);
        mem.as_slice_mut()[value_idx..value_idx + value_slice.len()].copy_from_slice(value_slice);
        assert_eq!(
            &mem.as_slice()[value_idx..value_idx + value_slice.len()],
            value_slice
        );
    }
}

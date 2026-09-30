use crate::types::{Pages, TrapCode};
use alloc::{vec, vec::Vec};
use core::ptr::NonNull;

/// Shared linear memory backing store for a running module.
/// Tracks current size in Wasm pages and provides bounds-checked read/write helpers.
///
/// The bytes live either in a `Vec` owned by the memory, grown in page-sized steps, or, with the
/// `memory-pool` feature, in a slot leased from a [`crate::MemoryPool`] (see
/// [`GlobalMemory::pooled`]). Both are reached through the same base pointer and length, so the
/// VM's access path does not branch on the backing.
///
/// There is no way to obtain the whole memory mutably. Every write goes through a window the
/// memory hands out for an exact range ([`GlobalMemory::tracked_mut`],
/// [`GlobalMemory::store_window`], [`GlobalMemory::copy_within`], [`GlobalMemory::write`]), and
/// handing it out is what marks the range dirty for a pooled slot. A write path that forgot to
/// mark its pages would leave one instance's bytes in the slot the next instance leases, so the
/// marking is not left to the callers.
pub struct GlobalMemory {
    /// Owner of the bytes of a `Vec` memory; empty when the memory is pooled. Only `new` and
    /// `grow` touch it, and both refresh `base` and `len` afterwards.
    buffer: Vec<u8>,
    /// First byte of the memory: the heap block of `buffer`, or the mapping of the leased slot.
    base: NonNull<u8>,
    /// Accessible bytes: `current_pages` in bytes.
    len: usize,
    /// Current logical size of the linear memory in pages.
    pub current_pages: Pages,
    /// The maximum allowed size of the linear memory in pages.
    pub max_allowed_memory_pages: Pages,
    /// The pooled slot backing the memory instead of `buffer`, when leased.
    #[cfg(all(feature = "memory-pool", unix))]
    lease: Option<crate::MemoryLease>,
}

// SAFETY: `base` points into `buffer` or into the leased slot, both owned by the memory and
// themselves `Send`; nothing else holds the pointer.
unsafe impl Send for GlobalMemory {}
// SAFETY: a shared reference only reads through `base`; every write needs `&mut self`.
unsafe impl Sync for GlobalMemory {}

impl GlobalMemory {
    /// Creates a memory of `initial_pages` that may grow up to `max_allowed_memory_pages`.
    ///
    /// # Panics
    ///
    /// Panics if `initial_pages` exceeds `max_allowed_memory_pages` or the page count does not
    /// fit the target's address space. Both arguments are chosen by the host (the store always
    /// starts at zero pages), never by a module, so this is a configuration error caught at
    /// startup rather than a reachable runtime failure.
    pub fn new(initial_pages: Pages, max_allowed_memory_pages: Pages) -> Self {
        let initial_len = initial_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        if initial_len > max_allowed_memory_pages.to_bytes().unwrap() {
            unreachable!("rwasm: initial memory size is greater than the maximum");
        }
        let mut buffer = vec![0; initial_len];
        Self {
            base: Self::base_of(&mut buffer),
            len: initial_len,
            buffer,
            current_pages: initial_pages,
            max_allowed_memory_pages,
            #[cfg(all(feature = "memory-pool", unix))]
            lease: None,
        }
    }

    /// The address of a `Vec`'s heap block (dangling, but non-null and aligned, when empty).
    fn base_of(buffer: &mut Vec<u8>) -> NonNull<u8> {
        NonNull::new(buffer.as_mut_ptr()).expect("rwasm: a Vec pointer is never null")
    }

    /// Creates a zero-page memory backed by a pooled slot.
    ///
    /// The memory grows inside the slot without zeroing anything: a slot is all zeros when it is
    /// leased. A `memory.grow` past the slot's capacity fails like an allocation failure, so the
    /// pool's `slot_pages` should not be smaller than `max_allowed_memory_pages`.
    #[cfg(all(feature = "memory-pool", unix))]
    pub fn pooled(lease: crate::MemoryLease, max_allowed_memory_pages: Pages) -> Self {
        Self {
            buffer: Vec::new(),
            base: lease.slot().base_ptr(),
            len: 0,
            current_pages: Pages::new_unchecked(0),
            max_allowed_memory_pages,
            lease: Some(lease),
        }
    }

    /// Whether the memory lives in a pooled slot.
    pub fn is_pooled(&self) -> bool {
        #[cfg(all(feature = "memory-pool", unix))]
        {
            return self.lease.is_some();
        }
        #[allow(unreachable_code)]
        false
    }

    /// Host pages written since the slot was leased, when pooled with dirty tracking.
    pub fn dirty_host_pages(&self) -> Option<usize> {
        #[cfg(all(feature = "memory-pool", unix))]
        {
            return self
                .lease
                .as_ref()
                .filter(|lease| lease.slot().tracks_dirty())
                .map(|lease| lease.slot().dirty_pages());
        }
        #[allow(unreachable_code)]
        None
    }

    /// Marks the host pages of a write to `[offset, offset + len)` dirty; a no-op for a `Vec`
    /// memory and in builds without the `memory-pool` feature.
    #[inline(always)]
    fn mark_dirty(&mut self, offset: usize, len: usize) {
        #[cfg(all(feature = "memory-pool", unix))]
        if let Some(lease) = &mut self.lease {
            lease.slot_mut().mark_dirty(offset, len);
        }
        #[cfg(not(all(feature = "memory-pool", unix)))]
        {
            let _ = (offset, len);
        }
    }

    /// Returns the number of pages in use by the linear memory.
    pub fn current_pages(&self) -> Pages {
        self.current_pages
    }

    /// Grows the linear memory by the given number of new pages.
    ///
    /// Returns the number of pages before the operation upon success.
    ///
    /// # Errors
    ///
    /// If the linear memory grows beyond its maximum limit after
    /// the growth operation.
    pub fn grow(&mut self, additional: Pages) -> Option<Pages> {
        let current_pages = self.current_pages();
        if additional == Pages::from(0) {
            return Some(current_pages);
        }
        let desired_pages = current_pages.checked_add(additional)?;
        if desired_pages > self.max_allowed_memory_pages {
            return None;
        }
        // At this point, it is okay to grow the underlying virtual memory
        // by the given number of additional pages.
        let new_size = desired_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        #[cfg(all(feature = "memory-pool", unix))]
        if let Some(lease) = &mut self.lease {
            // the slot is zero beyond `len` already, growing only widens the accessible prefix
            if new_size > lease.slot().capacity() {
                return None;
            }
            self.len = new_size;
            lease.slot_mut().note_accessible(new_size);
            self.current_pages = desired_pages;
            return Some(current_pages);
        }
        let additional_bytes = new_size.checked_sub(self.buffer.len())?;
        if self.buffer.try_reserve_exact(additional_bytes).is_err() {
            return None;
        }
        self.buffer.resize(new_size, 0);
        // the block may have moved
        self.base = Self::base_of(&mut self.buffer);
        self.len = new_size;
        self.current_pages = desired_pages;
        Some(current_pages)
    }

    /// Returns a shared slice to the bytes underlying to the byte buffer.
    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        // SAFETY: `base` points at `len` initialized bytes owned by `buffer` or by the leased
        // slot. Both live as long as `self`; `buffer` is reallocated only in `grow`, which
        // refreshes `base` and `len`, and a slot never moves.
        unsafe { core::slice::from_raw_parts(self.base.as_ptr(), self.len) }
    }

    /// Returns `[offset, offset + len)` for writing and marks it dirty.
    ///
    /// This is the only way to write to the memory: the caller cannot reach a byte outside the
    /// window, and the window's pages are recorded before it is handed out. A window that ends
    /// up unwritten (the caller fails afterwards) is marked all the same, which only costs a
    /// reset of pages that were clean.
    ///
    /// # Errors
    ///
    /// [`TrapCode::MemoryOutOfBounds`] if the range does not lie inside the memory.
    #[inline(always)]
    pub fn tracked_mut(&mut self, offset: usize, len: usize) -> Result<&mut [u8], TrapCode> {
        let end = offset.checked_add(len).ok_or(TrapCode::MemoryOutOfBounds)?;
        if end > self.len {
            return Err(TrapCode::MemoryOutOfBounds);
        }
        self.mark_dirty(offset, len);
        // SAFETY: the range was just checked against `len` (see `data` for `base`), and
        // `&mut self` makes the access exclusive.
        Ok(unsafe { core::slice::from_raw_parts_mut(self.base.as_ptr().add(offset), len) })
    }

    /// The write window of a Wasm store of `len` bytes at `address + offset`.
    ///
    /// # Errors
    ///
    /// [`TrapCode::MemoryOutOfBounds`] if `address + offset` overflows or the `len` bytes there
    /// do not lie inside the memory.
    #[inline(always)]
    pub fn store_window(
        &mut self,
        address: u32,
        offset: u32,
        len: usize,
    ) -> Result<&mut [u8], TrapCode> {
        let base = offset
            .checked_add(address)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        self.tracked_mut(base as usize, len)
    }

    /// Copies `len` bytes from `src` to `dst` inside the memory; the ranges may overlap.
    ///
    /// # Errors
    ///
    /// [`TrapCode::MemoryOutOfBounds`] if either range does not lie inside the memory; nothing
    /// is copied then.
    #[inline(always)]
    pub fn copy_within(&mut self, src: usize, dst: usize, len: usize) -> Result<(), TrapCode> {
        let src_end = src.checked_add(len).ok_or(TrapCode::MemoryOutOfBounds)?;
        let dst_end = dst.checked_add(len).ok_or(TrapCode::MemoryOutOfBounds)?;
        if src_end > self.len || dst_end > self.len {
            return Err(TrapCode::MemoryOutOfBounds);
        }
        self.mark_dirty(dst, len);
        // SAFETY: both ranges were just checked against `len` (see `data` for `base`);
        // `ptr::copy` allows them to overlap.
        unsafe {
            core::ptr::copy(
                self.base.as_ptr().add(src),
                self.base.as_ptr().add(dst),
                len,
            )
        };
        Ok(())
    }

    /// Reads `n` bytes from `memory[offset..offset+n]` into `buffer`
    /// where `n` is the length of `buffer`.
    ///
    /// # Errors
    ///
    /// If this operation accesses out of bounds linear memory.
    pub fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let len_buffer = buffer.len();
        let end = offset
            .checked_add(len_buffer)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        let slice = self
            .data()
            .get(offset..end)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        buffer.copy_from_slice(slice);
        Ok(())
    }

    /// Reads `n` bytes into vec
    pub fn read_into_vec(&self, offset: usize, len_buffer: usize) -> Result<Vec<u8>, TrapCode> {
        let end = offset
            .checked_add(len_buffer)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        let slice = self
            .data()
            .get(offset..end)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        Ok(slice.to_vec())
    }

    /// Writes `n` bytes to `memory[offset..offset+n]` from `buffer`
    /// where `n` if the length of `buffer`.
    ///
    /// # Errors
    ///
    /// If this operation accesses out of bounds linear memory.
    pub fn write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.tracked_mut(offset, buffer.len())?
            .copy_from_slice(buffer);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(pages: u32) -> GlobalMemory {
        let mut memory = GlobalMemory::new(Pages::new_unchecked(0), Pages::new_unchecked(8));
        memory.grow(Pages::new(pages).unwrap()).unwrap();
        memory
    }

    /// The base pointer follows the buffer across reallocation, and the contents survive it.
    #[test]
    fn growth_keeps_contents_and_zeroes_the_new_pages() {
        let mut memory = memory(1);
        memory.write(65530, &[1, 2, 3, 4, 5, 6]).unwrap();
        for _ in 0..7 {
            assert!(memory.grow(Pages::new(1).unwrap()).is_some());
        }
        assert_eq!(memory.data().len(), 8 * 65536);
        assert_eq!(&memory.data()[65530..65536], &[1, 2, 3, 4, 5, 6]);
        assert!(memory.data()[65536..].iter().all(|byte| *byte == 0));
        assert!(memory.grow(Pages::new(1).unwrap()).is_none());
        // a moved memory still reads and writes its own buffer
        let mut moved = memory;
        moved.write(0, &[9]).unwrap();
        assert_eq!(moved.data()[0], 9);
    }

    #[test]
    fn windows_are_bounds_checked() {
        let mut memory = memory(1);
        assert_eq!(memory.tracked_mut(65532, 4).unwrap().len(), 4);
        assert_eq!(
            memory.tracked_mut(65533, 4).unwrap_err(),
            TrapCode::MemoryOutOfBounds
        );
        assert_eq!(
            memory.tracked_mut(usize::MAX, 2).unwrap_err(),
            TrapCode::MemoryOutOfBounds
        );
        // an empty window at the very end is in bounds, one past it is not
        assert!(memory.tracked_mut(65536, 0).unwrap().is_empty());
        assert!(memory.tracked_mut(65537, 0).is_err());
        assert_eq!(memory.store_window(65530, 2, 4).unwrap().len(), 4);
        assert!(memory.store_window(65530, 3, 4).is_err());
        assert!(memory.store_window(u32::MAX, 1, 1).is_err());
    }

    #[test]
    fn copy_within_handles_overlap_and_rejects_out_of_bounds() {
        let mut memory = memory(1);
        memory.write(0, &[1, 2, 3, 4, 5]).unwrap();
        memory.copy_within(0, 2, 5).unwrap();
        assert_eq!(&memory.data()[..7], &[1, 2, 1, 2, 3, 4, 5]);
        memory.copy_within(2, 0, 5).unwrap();
        assert_eq!(&memory.data()[..7], &[1, 2, 3, 4, 5, 4, 5]);
        assert!(memory.copy_within(65535, 0, 2).is_err());
        assert!(memory.copy_within(0, 65535, 2).is_err());
        assert_eq!(
            &memory.data()[..2],
            &[1, 2],
            "a rejected copy writes nothing"
        );
        assert!(memory.copy_within(65536, 65536, 0).is_ok());
    }

    #[test]
    fn a_zero_page_memory_has_no_bytes() {
        let mut memory = GlobalMemory::new(Pages::new_unchecked(0), Pages::new_unchecked(1));
        assert!(memory.data().is_empty());
        assert!(memory.tracked_mut(0, 0).unwrap().is_empty());
        assert!(memory.tracked_mut(0, 1).is_err());
        assert!(memory.read_into_vec(0, 0).unwrap().is_empty());
    }
}

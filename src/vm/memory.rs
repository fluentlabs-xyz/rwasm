use crate::types::{Pages, TrapCode};
use alloc::{vec, vec::Vec};

/// Shared linear memory backing store for a running module.
/// Tracks current size in Wasm pages and provides bounds-checked read/write helpers.
/// The buffer is pre-reserved and grown in page-sized steps.
///
/// With the `memory-pool` feature the buffer can instead be a slot leased from a
/// [`crate::MemoryPool`] (see [`GlobalMemory::pooled`]); `shared_memory` then stays empty and the
/// accessible prefix of the slot serves every access.
pub struct GlobalMemory {
    /// Underlying byte buffer for the linear memory.
    pub shared_memory: Vec<u8>,
    /// Current logical size of the linear memory in pages.
    pub current_pages: Pages,
    /// The maximum allowed size of the linear memory in pages.
    pub max_allowed_memory_pages: Pages,
    /// The pooled slot backing the memory instead of `shared_memory`, when leased.
    #[cfg(all(feature = "memory-pool", unix))]
    lease: Option<crate::MemoryLease>,
    /// Accessible bytes of the pooled slot: `current_pages` in bytes.
    #[cfg(all(feature = "memory-pool", unix))]
    len: usize,
}

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
        let shared_memory = vec![0; initial_len];
        Self {
            shared_memory,
            current_pages: initial_pages,
            max_allowed_memory_pages,
            #[cfg(all(feature = "memory-pool", unix))]
            lease: None,
            #[cfg(all(feature = "memory-pool", unix))]
            len: 0,
        }
    }

    /// Creates a zero-page memory backed by a pooled slot.
    ///
    /// The memory grows inside the slot without zeroing anything: a slot is all zeros when it is
    /// leased. A `memory.grow` past the slot's capacity fails like an allocation failure, so the
    /// pool's `slot_pages` should not be smaller than `max_allowed_memory_pages`.
    #[cfg(all(feature = "memory-pool", unix))]
    pub fn pooled(lease: crate::MemoryLease, max_allowed_memory_pages: Pages) -> Self {
        Self {
            shared_memory: Vec::new(),
            current_pages: Pages::new_unchecked(0),
            max_allowed_memory_pages,
            lease: Some(lease),
            len: 0,
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

    /// Marks the host pages of a write to `[offset, offset + len)` dirty.
    ///
    /// Every write path of the VM calls this after a bounds-checked write; it is a no-op for a
    /// `Vec` memory and in builds without the `memory-pool` feature.
    #[inline(always)]
    pub fn mark_dirty(&mut self, offset: usize, len: usize) {
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
        let additional_bytes = new_size.checked_sub(self.shared_memory.len())?;
        if self
            .shared_memory
            .try_reserve_exact(additional_bytes)
            .is_err()
        {
            return None;
        }
        self.shared_memory.resize(new_size, 0);
        self.current_pages = desired_pages;
        Some(current_pages)
    }

    /// Returns a shared slice to the bytes underlying to the byte buffer.
    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        #[cfg(all(feature = "memory-pool", unix))]
        if let Some(lease) = &self.lease {
            return lease.slot().as_slice(self.len);
        }
        self.shared_memory.as_ref()
    }

    /// Returns an exclusive slice to the bytes underlying to the byte buffer.
    ///
    /// Writes through the slice are not tracked; see [`GlobalMemory::mark_dirty`].
    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut [u8] {
        #[cfg(all(feature = "memory-pool", unix))]
        if let Some(lease) = &mut self.lease {
            return lease.slot_mut().as_mut_slice(self.len);
        }
        self.shared_memory.as_mut()
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
        let len_buffer = buffer.len();
        let end = offset
            .checked_add(len_buffer)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        let slice = self
            .data_mut()
            .get_mut(offset..end)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        slice.copy_from_slice(buffer);
        self.mark_dirty(offset, len_buffer);
        Ok(())
    }
}

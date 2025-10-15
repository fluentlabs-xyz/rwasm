use crate::types::{Pages, TrapCode, N_MAX_MEMORY_PAGES};
use bytes::BytesMut;

#[cfg(all(feature = "unix-memory", unix))]
pub mod mmap;

pub trait IGlobalMemory {
    /// Returns the number of pages in use by the linear memory.
    fn current_pages(&self) -> Pages;
    /// Grows the linear memory by the given number of new pages.
    ///
    /// Returns the number of pages before the operation upon success.
    ///
    /// # Errors
    ///
    /// If the linear memory grows beyond its maximum limit after
    /// the growth operation.
    fn grow(&mut self, additional: Pages) -> Option<Pages>;
    /// Returns a shared slice to the bytes underlying to the byte buffer.
    fn data(&self) -> &[u8];
    /// Returns an exclusive slice to the bytes underlying to the byte buffer.
    fn data_mut(&mut self) -> &mut [u8];
    /// Reads `n` bytes from `memory[offset..offset+n]` into `buffer`
    /// where `n` is the length of `buffer`.
    ///
    /// # Errors
    ///
    /// If this operation accesses out of bounds linear memory.
    fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let len_buffer = buffer.len();
        let slice = self
            .data()
            .get(offset..(offset + len_buffer))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        buffer.copy_from_slice(slice);
        Ok(())
    }
    /// Writes `n` bytes to `memory[offset..offset+n]` from `buffer`
    /// where `n` if the length of `buffer`.
    ///
    /// # Errors
    ///
    /// If this operation accesses out of bounds linear memory.
    fn write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let len_buffer = buffer.len();
        let slice = self
            .data_mut()
            .get_mut(offset..(offset + len_buffer))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        slice.copy_from_slice(buffer);
        Ok(())
    }
    /// Resets memory (can be useful for reuse)
    fn reset(&mut self);
}

pub const MEMORY_MAX_PAGES: Pages = Pages::new_unchecked(N_MAX_MEMORY_PAGES * 2);

/// Shared linear memory backing store for a running module.
/// Tracks current size in Wasm pages and provides bounds-checked read/write helpers.
/// The buffer is pre-reserved and grown in page-sized steps.

pub struct OnDemandGlobalMemory {
    /// Underlying byte buffer for the linear memory.
    pub shared_memory: BytesMut,
    /// Current logical size of the linear memory in pages given at creation.
    pub initial_pages: Pages,
    /// Current logical size of the linear memory in pages.
    pub current_pages: Pages,
}

impl OnDemandGlobalMemory {
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
        let shared_memory = BytesMut::zeroed(initial_len);
        Self {
            initial_pages,
            shared_memory,
            current_pages: initial_pages,
        }
    }
}

impl IGlobalMemory for OnDemandGlobalMemory {
    /// Returns the number of pages in use by the linear memory.
    fn current_pages(&self) -> Pages {
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
        self.shared_memory.resize(new_size, 0);
        self.current_pages = desired_pages;
        Some(current_pages)
    }
    /// Returns a shared slice to the bytes underlying to the byte buffer.
    fn data(&self) -> &[u8] {
        self.shared_memory.as_ref()
    }
    /// Returns an exclusive slice to the bytes underlying to the byte buffer.
    fn data_mut(&mut self) -> &mut [u8] {
        self.shared_memory.as_mut()
    }
    /// Resets memory (can be useful for reuse)
    fn reset(&mut self) {
        self.current_pages = self.initial_pages;
        self.shared_memory.resize(0, 0);
    }
}

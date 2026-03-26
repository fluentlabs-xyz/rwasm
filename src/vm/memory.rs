use crate::types::{Pages, TrapCode};
use alloc::vec::Vec;
use bytes::BytesMut;

/// Shared linear memory backing store for a running module.
/// Tracks current size in Wasm pages and provides bounds-checked read/write helpers.
/// The buffer is pre-reserved and grown in page-sized steps.
pub struct GlobalMemory {
    /// Underlying byte buffer for the linear memory.
    pub shared_memory: BytesMut,
    /// Current logical size of the linear memory in pages.
    pub current_pages: Pages,
    /// The maximum allowed size of the linear memory in pages.
    pub max_allowed_memory_pages: Pages,
}

impl GlobalMemory {
    pub fn new(initial_pages: Pages, max_allowed_memory_pages: Pages) -> Self {
        let initial_len = initial_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        if initial_len > max_allowed_memory_pages.to_bytes().unwrap() {
            unreachable!("rwasm: initial memory size is greater than the maximum");
        }
        let mut shared_memory = BytesMut::with_capacity(initial_len);
        shared_memory.resize(initial_len, 0);
        Self {
            shared_memory,
            current_pages: initial_pages,
            max_allowed_memory_pages,
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
        assert!(new_size >= self.shared_memory.len());
        self.shared_memory.resize(new_size, 0);
        self.current_pages = desired_pages;
        Some(current_pages)
    }

    /// Returns a shared slice to the bytes underlying to the byte buffer.
    pub fn data(&self) -> &[u8] {
        self.shared_memory.as_ref()
    }

    /// Returns an exclusive slice to the bytes underlying to the byte buffer.
    pub fn data_mut(&mut self) -> &mut [u8] {
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
        Ok(())
    }
}

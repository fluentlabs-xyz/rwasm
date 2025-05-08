use crate::types::{Pages, RwasmError, N_MAX_MEMORY_PAGES};
use bytes::BytesMut;

pub struct GlobalMemory {
    pub shared_memory: BytesMut,
    pub current_pages: Pages,
}

const MEMORY_MAX_PAGES: Pages = Pages::new_unchecked(N_MAX_MEMORY_PAGES);

impl GlobalMemory {
    pub fn new(initial_pages: Pages) -> Self {
        let initial_len = initial_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        let maximum_len = MEMORY_MAX_PAGES
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        if initial_len > maximum_len {
            unreachable!("rwasm: initial memory size is greater than the maximum");
        }
        let mut shared_memory = BytesMut::with_capacity(maximum_len);
        shared_memory.resize(initial_len, 0);
        Self {
            shared_memory,
            current_pages: initial_pages,
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
    pub fn grow(&mut self, additional: Pages) -> Result<Pages, RwasmError> {
        let current_pages = self.current_pages();
        if additional == Pages::from(0) {
            return Ok(current_pages);
        }
        let desired_pages = current_pages
            .checked_add(additional)
            .ok_or(RwasmError::GrowthOperationLimited)?;
        if desired_pages > MEMORY_MAX_PAGES {
            return Err(RwasmError::GrowthOperationLimited);
        }
        // At this point, it is okay to grow the underlying virtual memory
        // by the given number of additional pages.
        let new_size = desired_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        assert!(new_size >= self.shared_memory.len());
        self.shared_memory.resize(new_size, 0);
        self.current_pages = desired_pages;
        Ok(current_pages)
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
    pub fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), RwasmError> {
        let len_buffer = buffer.len();
        let slice = self
            .data()
            .get(offset..(offset + len_buffer))
            .ok_or(RwasmError::MemoryOutOfBounds)?;
        buffer.copy_from_slice(slice);
        Ok(())
    }

    /// Writes `n` bytes to `memory[offset..offset+n]` from `buffer`
    /// where `n` if the length of `buffer`.
    ///
    /// # Errors
    ///
    /// If this operation accesses out of bounds linear memory.
    pub fn write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), RwasmError> {
        let len_buffer = buffer.len();
        let slice = self
            .data_mut()
            .get_mut(offset..(offset + len_buffer))
            .ok_or(RwasmError::MemoryOutOfBounds)?;
        slice.copy_from_slice(buffer);
        Ok(())
    }
}

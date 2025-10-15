use crate::types::{Pages, TrapCode, N_MAX_MEMORY_PAGES};

#[cfg(all(feature = "unix-memory", unix))]
mod mmap;
mod simple;

cfg_if::cfg_if! {
    if #[cfg(all(feature = "unix-memory", unix))] {
        pub type GlobalMemory = mmap::MmapGlobalMemory;
    } else {
        pub type GlobalMemory = simple::SimpleGlobalMemory;
    }
}

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

    /// Resets memory (can be useful for reuse).
    fn reset(&mut self);

    /// Resizes memory to fit into required initial pages.
    fn resize_for(&mut self, initial_pages: Pages);
}

pub const MEMORY_MAX_PAGES: Pages = Pages::new_unchecked(N_MAX_MEMORY_PAGES);

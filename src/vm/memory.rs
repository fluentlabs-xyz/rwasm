use crate::types::{Pages, TrapCode, N_MAX_MEMORY_PAGES};
#[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
use crate::vm::memory_unix::rwmem::RwMemory;
#[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
use bytes::BytesMut;

/// Shared linear memory backing store for a running module.
/// Tracks current size in Wasm pages and provides bounds-checked read/write helpers.
/// The buffer is pre-reserved and grown in page-sized steps.
pub struct GlobalMemory {
    /// Underlying byte buffer for the linear memory.
    #[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
    pub shared_memory: BytesMut,
    ///
    #[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
    pub shared_memory_unix: RwMemory,
    /// Current logical size of the linear memory in pages given at creation.
    pub initial_pages: Pages,
    /// Current logical size of the linear memory in pages.
    pub current_pages: Pages,
}

const MEMORY_MAX_PAGES: Pages = Pages::new_unchecked(N_MAX_MEMORY_PAGES * 2);

impl GlobalMemory {
    pub fn new(initial_pages: Pages) -> Self {
        #[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
        {
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
        #[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
        {
            let shared_memory_unix = crate::vm::memory_unix::rwmem::Memory::new(
                initial_pages.into_inner(),
                MEMORY_MAX_PAGES.into_inner(),
            )
            .unwrap();
            Self {
                initial_pages,
                shared_memory_unix,
                current_pages: initial_pages,
            }
        }
    }

    /// Resets memory (can be useful for reuse)
    pub fn reset(&mut self) {
        self.current_pages = self.initial_pages;
        #[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
        {
            unsafe { self.shared_memory_unix.heap.recycle() }
        }
        #[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
        {
            self.shared_memory
                .resize(self.current_pages.to_bytes().unwrap(), 0);
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
        if desired_pages > MEMORY_MAX_PAGES {
            return None;
        }
        // At this point, it is okay to grow the underlying virtual memory
        // by the given number of additional pages.
        let new_size = desired_pages
            .to_bytes()
            .expect("rwasm: not supported target pointer width");
        #[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
        {
            assert!(new_size >= self.shared_memory.len());
            self.shared_memory.resize(new_size, 0);
            self.current_pages = desired_pages;
            Some(current_pages)
        }
        #[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
        {
            assert!(new_size >= self.shared_memory_unix.committed_len());
            if self
                .shared_memory_unix
                .grow(additional.into_inner())
                .is_err()
            {
                return None;
            };
            self.current_pages = desired_pages;
            Some(current_pages)
        }
    }

    /// Returns a shared slice to the bytes underlying to the byte buffer.
    pub fn data(&self) -> &[u8] {
        #[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
        {
            self.shared_memory.as_ref()
        }
        #[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
        {
            self.shared_memory_unix.as_slice()
        }
    }

    /// Returns an exclusive slice to the bytes underlying to the byte buffer.
    pub fn data_mut(&mut self) -> &mut [u8] {
        #[cfg(not(all(feature = "unix-memory", unix, not(target_arch = "wasm32"))))]
        {
            self.shared_memory.as_mut()
        }
        #[cfg(all(feature = "unix-memory", unix, not(target_arch = "wasm32")))]
        {
            self.shared_memory_unix.as_slice_mut()
        }
    }

    /// Reads `n` bytes from `memory[offset..offset+n]` into `buffer`
    /// where `n` is the length of `buffer`.
    ///
    /// # Errors
    ///
    /// If this operation accesses out of bounds linear memory.
    pub fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
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
    pub fn write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let len_buffer = buffer.len();
        let slice = self
            .data_mut()
            .get_mut(offset..(offset + len_buffer))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        slice.copy_from_slice(buffer);
        Ok(())
    }
}

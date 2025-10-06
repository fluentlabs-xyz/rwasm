#[cfg(all(feature = "unix-memory-pool", unix))]
pub mod rwmem {
    use core::ptr::NonNull;
    use core::{
        mem, ptr,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use libc::{
        c_void, madvise, mmap, mprotect, munmap, sigaction, sigaltstack, sigemptyset, siginfo_t,
        stack_t, MADV_DONTNEED, MAP_ANON, MAP_FAILED, MAP_PRIVATE, PROT_NONE, PROT_READ,
        PROT_WRITE, SA_ONSTACK, SA_SIGINFO, SIGSEGV,
    };

    pub const WASM_PAGE: usize = 64 * 1024;
    pub type Pages = u32; // 32-bit pointers / sizes in pages

    #[inline]
    fn ceil_to_pages(len: usize) -> usize {
        (len + WASM_PAGE - 1) / WASM_PAGE * WASM_PAGE
    }

    /// Linear memory reservation with front/back guards.
    pub struct GuardedHeap {
        base: NonNull<u8>,          // points to start of HEAP (after front guard)
        reserved_len: usize,        // total HEAP bytes reserved (without guards)
        committed_len: AtomicUsize, // bytes currently RW (multiple of page)
        front_guard: usize,         // guard size before heap
        back_guard: usize,          // guard size after heap
    }

    impl GuardedHeap {
        /// Reserve `[GUARD | HEAP | GUARD]` and commit `initial_pages`.
        /// `max_pages` caps growth; both guards are at least one page.
        pub unsafe fn new(initial_pages: Pages, max_pages: Pages) -> Result<Self, &'static str> {
            let guard = WASM_PAGE; // 64 KiB guard is enough to turn OOB into SIGSEGV fast
            let heap_res = (max_pages as usize) * WASM_PAGE;
            let map_len = guard + heap_res + guard;

            let addr = mmap(
                ptr::null_mut(),
                map_len,
                PROT_NONE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            );
            if addr == MAP_FAILED {
                return Err("mmap reserve failed");
            }

            // Commit the initial part of HEAP as RW
            let init_len = ceil_to_pages((initial_pages as usize) * WASM_PAGE);
            if init_len > 0 {
                let heap_ptr = (addr as usize + guard) as *mut c_void;
                if mprotect(heap_ptr, init_len, PROT_READ | PROT_WRITE) != 0 {
                    let _ = munmap(addr, map_len);
                    return Err("mprotect initial commit failed");
                }
                // Ensure zero pages on first touch (they already are zero, but this keeps the story)
                let _ = madvise(heap_ptr, init_len, MADV_DONTNEED);
            }

            Ok(Self {
                base: NonNull::new_unchecked((addr as usize + guard) as *mut u8),
                reserved_len: heap_res,
                committed_len: AtomicUsize::new(init_len),
                front_guard: guard,
                back_guard: guard,
            })
        }

        /// Pointer to start of linear memory. Your JIT/interpreter can do `base.add(u32_offset)`.
        #[inline]
        pub fn base(&self) -> *mut u8 {
            self.base.as_ptr()
        }

        /// Bytes currently committed RW.
        #[inline]
        pub fn committed_len(&self) -> usize {
            self.committed_len.load(Ordering::Relaxed)
        }

        /// Max bytes we can grow to (reserved).
        #[inline]
        pub fn reserved_len(&self) -> usize {
            self.reserved_len
        }

        /// Grow by `delta_pages`. Newly committed pages are logically zero via DONTNEED.
        pub unsafe fn grow(&self, delta_pages: Pages) -> Result<(), &'static str> {
            if delta_pages == 0 {
                return Ok(());
            }
            let add = (delta_pages as usize) * WASM_PAGE;

            let old = self.committed_len.load(Ordering::Relaxed);
            let new = old.checked_add(add).ok_or("overflow")?;
            if new > self.reserved_len {
                return Err("exceeds reserved");
            }

            let start = self.base.as_ptr().add(old) as *mut c_void;
            if mprotect(start, add, PROT_READ | PROT_WRITE) != 0 {
                return Err("mprotect grow failed");
            }
            // Make the kernel hand zero pages lazily on next touch
            let _ = madvise(start, add, MADV_DONTNEED);

            self.committed_len.store(new, Ordering::Release);
            Ok(())
        }

        /// Zero-and-forget: turn the committed range back into “fresh zero” without unmapping.
        pub unsafe fn recycle(&self) {
            let len = self.committed_len();
            if len == 0 {
                return;
            }
            let ptr = self.base.as_ptr() as *mut c_void;
            // Keep writable; just tell kernel we don't need contents.
            let _ = madvise(ptr, len, MADV_DONTNEED);
        }
    }

    impl Drop for GuardedHeap {
        fn drop(&mut self) {
            unsafe {
                let map_base = (self.base.as_ptr() as usize - self.front_guard) as *mut c_void;
                let map_len = self.front_guard + self.reserved_len + self.back_guard;
                let _ = munmap(map_base, map_len);
            }
        }
    }

    // ===== Trap trampoline (SIGSEGV -> Result::Err) =====

    // Per-thread jump buffer. We only need an address to jump back to.
    // We use `libc::sigsetjmp/siglongjmp` because unwinding across a signal is UB.
    #[repr(C)]
    struct JmpBuf {
        buf: [libc::c_int; 27],
    } // typical glibc size; we never touch fields

    thread_local! {
        static TLS_JMP: Jmp = Jmp::new();
    }

    struct Jmp {
        buf: JmpBuf,
    }
    impl Jmp {
        const fn new() -> Self {
            Self {
                buf: JmpBuf { buf: [0; 27] },
            }
        }
    }

    static mut SEGV_INSTALLED: bool = false;

    extern "C" fn segv_handler(_sig: libc::c_int, _si: *mut siginfo_t, _ctx: *mut c_void) {
        // Jump back to the last `run_with_memory_trap` call on this thread.
        TLS_JMP.with(|j| unsafe { setjmp::siglongjmp(j.buf.buf.as_ptr() as *mut _, 1) });
    }

    /// Call `f()` with a SIGSEGV→Err trampoline.
    /// Return `Ok` if no fault; `Err` if any memory fault happened inside.
    pub fn run_with_memory_trap<F, T>(f: F) -> Result<T, ()>
    where
        F: FnOnce() -> T,
    {
        unsafe {
            // One-time global install of SIGSEGV handler + altstack (so we can handle guard faults reliably).
            if !SEGV_INSTALLED {
                // alt stack (32 KiB is plenty)
                const ALT: usize = 32 * 1024;
                static mut ALTSTACK: *mut u8 = ptr::null_mut();
                if ALTSTACK.is_null() {
                    ALTSTACK = mmap(
                        ptr::null_mut(),
                        ALT,
                        PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANON,
                        -1,
                        0,
                    ) as *mut u8;
                }
                let ss = stack_t {
                    ss_sp: ALTSTACK as *mut c_void,
                    ss_flags: 0,
                    ss_size: ALT,
                };
                if sigaltstack(&ss, ptr::null_mut()) != 0 {
                    return Err(());
                }

                let mut sa: sigaction = mem::zeroed();
                sa.sa_sigaction = segv_handler as usize;
                sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
                sigemptyset(&mut sa.sa_mask);
                if libc::sigaction(SIGSEGV, &sa, ptr::null_mut()) != 0 {
                    return Err(());
                }
                SEGV_INSTALLED = true;
            }
        }

        // Establish jump point.
        let jumped =
            TLS_JMP.with(|j| unsafe { setjmp::sigsetjmp(j.buf.buf.as_ptr() as *mut _, 1) });
        if jumped != 0 {
            // We got here via siglongjmp from the handler => memory fault
            return Err(());
        }
        // Normal execution
        let out = f();
        Ok(out)
    }

    // Public facade you’ll likely call from your runtime:
    pub struct RwMemory {
        pub heap: GuardedHeap,
    }

    impl RwMemory {
        pub fn new(initial_pages: Pages, max_pages: Pages) -> Result<Self, &'static str> {
            Ok(Self {
                heap: unsafe { GuardedHeap::new(initial_pages, max_pages)? },
            })
        }
        #[inline]
        pub fn base(&self) -> *mut u8 {
            self.heap.base()
        }
        #[inline]
        pub fn as_slice(&self) -> &[u8] {
            unsafe { core::slice::from_raw_parts(self.base(), self.committed_len()) }
        }
        #[inline]
        pub fn as_slice_mut(&self) -> &mut [u8] {
            unsafe { core::slice::from_raw_parts_mut(self.base(), self.committed_len()) }
        }
        #[inline]
        pub fn committed_len(&self) -> usize {
            self.heap.committed_len()
        }
        #[inline]
        pub fn reserved_len(&self) -> usize {
            self.heap.reserved_len()
        }
        #[inline]
        pub fn grow(&self, delta_pages: Pages) -> Result<(), &'static str> {
            unsafe { self.heap.grow(delta_pages) }
        }
        #[inline]
        pub unsafe fn recycle(&self) {
            self.heap.recycle()
        }
    }

    pub use run_with_memory_trap as with_trap;
    pub use GuardedHeap as Heap;
    pub use RwMemory as Memory;
}

#[cfg(test)]
mod tests {
    use crate::vm::memory_pool_unix::rwmem::{RwMemory, WASM_PAGE};

    #[test]
    fn test_rw_memory() {
        let initial_pages = 1;
        let current_pages = initial_pages;
        let delta_pages = 1;
        let max_pages = 1024;
        let mem = RwMemory::new(initial_pages, max_pages).unwrap();
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

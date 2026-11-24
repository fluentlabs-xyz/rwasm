//! An mmap-backed pool of linear-memory slots (FLU-1501).
//!
//! A [`MemorySlot`] is one anonymous, private, read-write mapping of the whole address range a
//! store may ever grow its memory to (`slot_pages` Wasm pages). The mapping is reserved once and
//! never zeroed by hand: the kernel hands out demand-zero pages on first touch, so an instance
//! that declares 1 MiB of shadow stack and touches 8 KiB of it commits two host pages, not 256.
//!
//! A [`MemoryPool`] hands slots out as [`MemoryLease`]s. Dropping a lease returns the slot to the
//! all-zero state and puts it back in the pool, whatever the guest did with it: the reset is
//! driven only by host-owned metadata (the dirty bitmap and the high-water mark), never by guest
//! state, so a trap, an `OutOfFuel` or a halt in the middle of a store leaves nothing behind.
//!
//! Dirty host pages are tracked explicitly: [`crate::GlobalMemory`] hands out every write window
//! itself and marks it ([`MemorySlot::mark_dirty`]), so no write path can skip the tracking. The
//! design and the measurements are in `docs/research/flu-1501-mmap-cow-memory-pool.md`. The
//! module sits behind the `memory-pool` feature and is compiled on Unix hosts only; the `no_std`
//! build (the zkVM path) keeps the `Vec` memory.
//!
//! On Linux a slot is split: the first [`MemoryPoolConfig::small_page_prefix`] bytes, where every
//! contract keeps its stack and data, are opted out of transparent huge pages so that a sparse
//! write set costs 4 KiB pages; the rest, which only a memory-hungry call grows into, is
//! huge-page eligible so that filling it costs a fault per 2 MiB and not per 4 KiB.

mod global_memory;

use crate::N_DEFAULT_MAX_MEMORY_PAGES;
pub use global_memory::GlobalMemory;
use std::{
    io,
    mem::ManuallyDrop,
    ptr::NonNull,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

/// The largest linear memory a Wasm module can have, in bytes: 65536 pages of 64 KiB. No slot
/// is larger than this.
const MAX_SLOT_BYTES: u64 = 65536 * crate::N_BYTES_PER_MEMORY_PAGE as u64;

/// The host page size in bytes, read from the OS once.
///
/// Dirtiness is tracked at this granularity, not per 64 KiB Wasm page: a one-byte store marks one
/// host page (4 KiB on most Linux hosts, 16 KiB on Apple Silicon).
pub fn host_page_size() -> usize {
    static PAGE_SIZE: OnceLock<usize> = OnceLock::new();
    *PAGE_SIZE.get_or_init(|| {
        // SAFETY: `sysconf` has no preconditions.
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        usize::try_from(size)
            .ok()
            .filter(|size| size.is_power_of_two())
            .expect("rwasm: the host page size must be a power of two")
    })
}

/// The transparent huge page size of the host in bytes, or 0 where there is none (every
/// non-Linux host, and a Linux kernel built without THP).
pub fn huge_page_size() -> usize {
    #[cfg(target_os = "linux")]
    {
        static HUGE_PAGE_SIZE: OnceLock<usize> = OnceLock::new();
        return *HUGE_PAGE_SIZE.get_or_init(|| {
            std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/hpage_pmd_size")
                .ok()
                .and_then(|size| size.trim().parse::<usize>().ok())
                .filter(|size| size.is_power_of_two() && *size > host_page_size())
                .unwrap_or(0)
        });
    }
    #[allow(unreachable_code)]
    0
}

/// How a released slot is brought back to all zeros.
///
/// Every policy resets the *reachable* range only: `[0, high_water)`, where the high-water mark
/// is the largest memory size the lease reached through `memory.grow`. Nothing beyond it was ever
/// accessible to the guest.
///
/// The policy governs the small-page part of a slot. The huge-page part (see
/// [`MemoryPoolConfig::small_page_prefix`]) is always given back with one kernel call when the
/// lease reached into it: keeping it resident would pin 2 MiB per touched page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetPolicy {
    /// One kernel call over the reachable range: `madvise(MADV_DONTNEED)` on Linux,
    /// `madvise(MADV_ZERO)` on macOS (falling back to [`ResetPolicy::Remap`] where the kernel
    /// does not support it). The kernel walks the page tables and frees only the pages that are
    /// resident, so the cost follows the touched pages, not the range.
    Discard,
    /// One `mmap(MAP_FIXED | MAP_ANON)` over the reachable range. Portable, but it replaces the
    /// VM mapping instead of dropping pages from it.
    Remap,
    /// Bitmap-driven: [`ResetPolicy::Discard`] applied to each run of dirty host pages.
    DiscardDirty,
    /// Bitmap-driven: [`ResetPolicy::Remap`] applied to each run of dirty host pages.
    RemapDirty,
    /// Bitmap-driven: zero every dirty host page with `memset`. No kernel call; the pages stay
    /// resident, so the next lease of the slot does not fault them in again.
    Memset,
    /// [`ResetPolicy::Memset`] while the dirty set is at most `memset_up_to_pages` host pages,
    /// [`ResetPolicy::Discard`] beyond that.
    Adaptive { memset_up_to_pages: usize },
}

/// What one reset did; reported to the pool for measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResetStats {
    /// The primitive that did the work.
    pub method: &'static str,
    /// Host pages the reset covered.
    pub pages: usize,
    /// Kernel calls (or `memset` runs) it took.
    pub calls: usize,
    /// Host pages the dirty bitmap had set when the reset started.
    pub dirty_pages: usize,
}

/// Configuration of a [`MemoryPool`].
#[derive(Debug, Clone)]
pub struct MemoryPoolConfig {
    /// Capacity of every slot, in Wasm pages. The pool serves a memory only if a slot holds its
    /// whole page cap: a store that allows more pages than this keeps its `Vec` memory (see
    /// [`MemoryPool::lease_for`]), because a slot it could outgrow would fail a `memory.grow`
    /// that the `Vec` memory and the Wasmtime backend both serve. More than 65536 pages, the
    /// largest Wasm memory, is a slot that never maps (see [`MemorySlot::reserve`]).
    pub slot_pages: u32,
    /// Reset slots kept for reuse; a released slot beyond this is unmapped.
    pub max_free_slots: usize,
    /// How released slots are reset.
    pub reset_policy: ResetPolicy,
    /// Maintain the dirty bitmap on every write. The bitmap-driven policies need it; the
    /// range policies do not, and turning it off removes the tracking from the VM's store paths.
    pub track_dirty: bool,
    /// Read the reachable range back after every reset and refuse to pool a slot that is not all
    /// zeros. Costs a pass over the range; on by default in debug builds only.
    pub verify_reset: bool,
    /// Bytes at the start of a slot that never use transparent huge pages (Linux), rounded up to
    /// the huge page size; the rest of the slot is huge-page eligible (`MADV_HUGEPAGE`).
    /// `usize::MAX` keeps the whole slot on small pages, `0` makes all of it eligible. Ignored
    /// where there are no transparent huge pages.
    pub small_page_prefix: usize,
}

impl MemoryPoolConfig {
    /// Dirty bytes up to which the default policy zeroes pages by hand and keeps them resident.
    /// Equal to the default small-page prefix, so that on a split slot the small-page part is
    /// always zeroed in place and never faulted in again.
    ///
    /// This bounds what one reset zeroes, not what a slot keeps resident: zeroed pages stay
    /// resident and successive leases can dirty different ranges. On a split slot the residency
    /// is bounded by the prefix, since the rest is discarded on every release; a slot without a
    /// huge-page part can accumulate up to everything its leases have reached. A recycled slot
    /// takes those resident pages along to its next lease.
    pub const DEFAULT_MEMSET_UP_TO_BYTES: usize = Self::DEFAULT_SMALL_PAGE_PREFIX;

    /// The default [`MemoryPoolConfig::small_page_prefix`]: room for a contract's 1 MiB shadow
    /// stack, its data and a heap of a few megabytes.
    pub const DEFAULT_SMALL_PAGE_PREFIX: usize = 8 * 1024 * 1024;

    /// The default reset policy: `memset` up to [`Self::DEFAULT_MEMSET_UP_TO_BYTES`] of dirty
    /// pages, discard the reachable range beyond that.
    pub fn default_reset_policy() -> ResetPolicy {
        ResetPolicy::Adaptive {
            memset_up_to_pages: Self::DEFAULT_MEMSET_UP_TO_BYTES / host_page_size(),
        }
    }
}

impl Default for MemoryPoolConfig {
    fn default() -> Self {
        Self {
            slot_pages: N_DEFAULT_MAX_MEMORY_PAGES,
            max_free_slots: 8,
            reset_policy: Self::default_reset_policy(),
            track_dirty: true,
            // debug builds check every reset; a slot that is not all zeros is never pooled
            verify_reset: cfg!(debug_assertions),
            small_page_prefix: Self::DEFAULT_SMALL_PAGE_PREFIX,
        }
    }
}

/// Counters of a [`MemoryPool`].
#[derive(Debug, Default)]
pub struct MemoryPoolStats {
    /// Leases handed out.
    pub leases: AtomicU64,
    /// Leases declined because a slot cannot hold the requested page cap
    /// ([`MemoryPool::lease_for`]); the store ran those instances on a `Vec` memory.
    pub leases_declined: AtomicU64,
    /// Leases that failed because a new slot could not be mapped, or was refused as larger than
    /// any Wasm memory; the store ran those instances on a `Vec` memory as well.
    pub map_failures: AtomicU64,
    /// Slots mapped because the free list was empty.
    pub slots_reserved: AtomicU64,
    /// Slots returned to the free list after a successful reset.
    pub slots_recycled: AtomicU64,
    /// Slots unmapped: a failed reset, a failed verification, or a full free list.
    pub slots_unmapped: AtomicU64,
    /// Resets that failed or left non-zero bytes behind.
    pub reset_failures: AtomicU64,
}

impl MemoryPoolStats {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// One reserved slot of linear memory.
pub struct MemorySlot {
    base: NonNull<u8>,
    /// Bytes mapped, a multiple of the host page size.
    capacity: usize,
    /// `log2(host page size)`.
    page_shift: u32,
    /// One bit per host page of `capacity`.
    dirty: Vec<u64>,
    /// Bits set in `dirty`.
    dirty_pages: usize,
    /// Largest accessible length since the last reset, in bytes.
    high_water: usize,
    track_dirty: bool,
    /// First byte of the huge-page eligible part; `capacity` when the slot has none. A multiple
    /// of the huge page size, which on the hosts measured is a whole number of bitmap words
    /// (64 host pages); the reset does not depend on that, see `small_page_words`.
    split: usize,
    /// The host's huge page size, 0 when the slot has no huge-page part.
    huge_page: usize,
    /// Byte range `[start, end)` that a failed `mmap(MAP_FIXED)` of a reset may have left
    /// unmapped, and that `Drop` therefore leaves alone; see `remap_range`.
    hole: Option<(usize, usize)>,
}

// SAFETY: the slot exclusively owns its mapping; nothing else aliases it.
unsafe impl Send for MemorySlot {}
// SAFETY: shared references only read the mapping, and every write needs `&mut`.
unsafe impl Sync for MemorySlot {}

impl MemorySlot {
    /// Maps `capacity` bytes of anonymous private memory, rounded up to the host page size.
    ///
    /// The first `small_page_prefix` bytes (rounded up to the huge page size) never use
    /// transparent huge pages; the rest is huge-page eligible. A slot with a huge-page part is
    /// aligned to the huge page size.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a capacity above the largest Wasm memory (65536 pages, 4 GiB), which
    /// is refused without a mapping attempt; otherwise the error of the failed `mmap`.
    pub fn reserve(
        capacity: usize,
        track_dirty: bool,
        small_page_prefix: usize,
    ) -> io::Result<Self> {
        Self::reserve_with_huge_page(capacity, track_dirty, small_page_prefix, huge_page_size())
    }

    /// [`Self::reserve`] for a host whose transparent huge pages are `huge_page` bytes: a power
    /// of two above the host page size, or 0 for a host without them.
    fn reserve_with_huge_page(
        capacity: usize,
        track_dirty: bool,
        small_page_prefix: usize,
        huge_page: usize,
    ) -> io::Result<Self> {
        let page_size = host_page_size();
        let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
        let capacity = capacity
            .max(page_size)
            .checked_add(page_size - 1)
            .ok_or_else(invalid)?
            & !(page_size - 1);
        // A slot larger than the largest Wasm memory is never needed. Where the address space is
        // wide enough for the kernel to grant such a mapping, its dirty bitmap alone would take
        // gigabytes, so the request is refused before the kernel sees it.
        if capacity as u64 > MAX_SLOT_BYTES {
            return Err(invalid());
        }
        let split = if huge_page > 0 && small_page_prefix < capacity {
            (small_page_prefix.div_ceil(huge_page) * huge_page).min(capacity)
        } else {
            capacity
        };
        let huge_page = if split < capacity { huge_page } else { 0 };
        let base = map_aligned(capacity, huge_page.max(page_size))?;
        #[cfg(target_os = "linux")]
        {
            // Advisory: a kernel without THP reports EINVAL, which changes nothing.
            // SAFETY: both ranges lie inside the mapping just created.
            unsafe {
                if split > 0 {
                    libc::madvise(base.cast(), split, libc::MADV_NOHUGEPAGE);
                }
                if split < capacity {
                    libc::madvise(
                        base.add(split).cast(),
                        capacity - split,
                        libc::MADV_HUGEPAGE,
                    );
                }
            }
        }
        let pages = capacity >> page_size.trailing_zeros();
        Ok(Self {
            base: NonNull::new(base).expect("mmap returned null"),
            capacity,
            page_shift: page_size.trailing_zeros(),
            dirty: vec![0; pages.div_ceil(64)],
            dirty_pages: 0,
            high_water: 0,
            track_dirty,
            split,
            huge_page,
            hole: None,
        })
    }

    /// The first byte of the mapping.
    pub(crate) fn base_ptr(&self) -> NonNull<u8> {
        self.base
    }

    /// The offset the huge-page eligible part of the slot starts at, if it has one.
    pub fn huge_pages_from(&self) -> Option<usize> {
        (self.split < self.capacity).then_some(self.split)
    }

    /// Bytes this slot can serve.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Host pages marked dirty since the last reset.
    pub fn dirty_pages(&self) -> usize {
        self.dirty_pages
    }

    /// Largest length the lease has made accessible since the last reset.
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// Whether writes are tracked in the dirty bitmap.
    pub fn tracks_dirty(&self) -> bool {
        self.track_dirty
    }

    /// The first `len` bytes of the slot, clamped to the capacity.
    #[inline(always)]
    pub fn as_slice(&self, len: usize) -> &[u8] {
        // SAFETY: at most `capacity` bytes from `base`, inside a live, readable mapping owned by
        // `self`.
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), len.min(self.capacity)) }
    }

    /// The first `len` bytes of the slot, mutably, clamped to the capacity.
    ///
    /// Handing the bytes out counts them as reached ([`MemorySlot::note_accessible`]), so every
    /// reset that works on the reached range covers what is written through the slice. The
    /// writes themselves are not tracked: a caller that resets by the dirty bitmap marks them
    /// with [`MemorySlot::mark_dirty`]. The VM does not use this accessor; its memory goes
    /// through the write windows of [`crate::GlobalMemory`], which mark what they hand out.
    #[inline(always)]
    pub fn as_mut_slice(&mut self, len: usize) -> &mut [u8] {
        let len = len.min(self.capacity);
        self.note_accessible(len);
        // SAFETY: as in `as_slice`; `&mut self` makes the access exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr(), len) }
    }

    /// Records that `[0, len)` is (or was) accessible to the guest.
    #[inline(always)]
    pub fn note_accessible(&mut self, len: usize) {
        if len > self.high_water {
            self.high_water = len;
        }
    }

    /// Marks the host pages covering `[offset, offset + len)` dirty.
    ///
    /// The range is clamped to the mapping. The bitmap is padded to whole words, so a page past
    /// the mapping can still have a bit, and a set bit there would make the bitmap-driven resets
    /// zero or remap memory the slot does not own. The VM only marks ranges it has bounds-checked
    /// against the accessible memory, so nothing is clamped on its path. A scalar store covers
    /// one page, or two when it straddles a page boundary.
    #[inline(always)]
    pub fn mark_dirty(&mut self, offset: usize, len: usize) {
        if !self.track_dirty {
            return;
        }
        let end = offset.saturating_add(len).min(self.capacity);
        if offset >= end {
            return;
        }
        let first = offset >> self.page_shift;
        let last = (end - 1) >> self.page_shift;
        let mut page = first;
        loop {
            let word = &mut self.dirty[page >> 6];
            let bit = 1u64 << (page & 63);
            if *word & bit == 0 {
                *word |= bit;
                self.dirty_pages += 1;
            }
            if page == last {
                break;
            }
            page += 1;
        }
    }

    /// Calls `f(first_page, page_count)` for every run of consecutive dirty host pages.
    ///
    /// Skips clean words of the bitmap (64 pages each), so a sparse bitmap over a 64 MiB slot is
    /// scanned in a few hundred nanoseconds.
    pub fn for_each_dirty_run(&self, f: impl FnMut(usize, usize)) {
        self.dirty_runs_in(self.dirty.len(), f)
    }

    /// [`Self::for_each_dirty_run`] over the first `words` words of the bitmap.
    fn dirty_runs_in(&self, words: usize, mut f: impl FnMut(usize, usize)) {
        let mut run_start: Option<usize> = None;
        for (index, &word) in self.dirty[..words].iter().enumerate() {
            if word == 0 {
                if let Some(start) = run_start.take() {
                    f(start, index * 64 - start);
                }
                continue;
            }
            if word == u64::MAX {
                run_start.get_or_insert(index * 64);
                continue;
            }
            for bit in 0..64 {
                let page = index * 64 + bit;
                match (word & (1u64 << bit) != 0, run_start) {
                    (true, None) => run_start = Some(page),
                    (false, Some(start)) => {
                        f(start, page - start);
                        run_start = None;
                    }
                    _ => {}
                }
            }
        }
        if let Some(start) = run_start {
            f(start, words * 64 - start);
        }
    }

    /// Bitmap words that cover the small-page part of the slot.
    ///
    /// Rounded up: when the split is not a whole number of words, the last word also holds
    /// pages of the huge-page part. Resetting those by the bitmap as well is harmless, they are
    /// given back whenever the lease reached them. Rounding down would leave the small pages
    /// of that word out of every bitmap-driven reset and hand their bytes to the next lease.
    fn small_page_words(&self) -> usize {
        if self.split < self.capacity {
            (self.split >> self.page_shift).div_ceil(64)
        } else {
            self.dirty.len()
        }
    }

    /// Returns the reachable range to all zeros and clears the tracking metadata.
    ///
    /// The huge-page part, if the lease reached into it, is discarded with one kernel call; the
    /// small-page part follows `policy`.
    ///
    /// # Errors
    ///
    /// A failed kernel call, or non-zero bytes found by `verify`. The slot is then in an unknown
    /// state and must be unmapped, never pooled.
    pub fn reset(&mut self, policy: ResetPolicy, verify: bool) -> io::Result<ResetStats> {
        let page_size = 1usize << self.page_shift;
        let reach = ((self.high_water + page_size - 1) & !(page_size - 1)).min(self.capacity);
        let base = self.base.as_ptr();
        let mut stats = ResetStats {
            method: "none",
            pages: 0,
            calls: 0,
            dirty_pages: self.dirty_pages,
        };

        // the huge-page part: whole huge pages, up to the one holding the last reachable byte
        if reach > self.split {
            let end = (reach.div_ceil(self.huge_page) * self.huge_page).min(self.capacity);
            let len = end - self.split;
            match discard_range(base.wrapping_add(self.split), len) {
                Ok(method) => stats.method = method,
                Err(failure) => return Err(self.failed(self.split, len, failure)),
            }
            stats.pages += len >> self.page_shift;
            stats.calls += 1;
        }

        // the small-page part
        let reach = reach.min(self.split);
        let words = self.small_page_words();
        let policy = match policy {
            ResetPolicy::Adaptive { memset_up_to_pages } => {
                let dirty: usize = self.dirty[..words]
                    .iter()
                    .map(|word| word.count_ones() as usize)
                    .sum();
                if self.track_dirty && dirty <= memset_up_to_pages {
                    ResetPolicy::Memset
                } else {
                    ResetPolicy::Discard
                }
            }
            // without a bitmap the whole reachable range has to go
            ResetPolicy::DiscardDirty | ResetPolicy::RemapDirty | ResetPolicy::Memset
                if !self.track_dirty =>
            {
                ResetPolicy::Discard
            }
            policy => policy,
        };
        match policy {
            ResetPolicy::Discard | ResetPolicy::Remap if reach == 0 => {}
            ResetPolicy::Discard => {
                match discard_range(base, reach) {
                    Ok(method) => stats.method = method,
                    Err(failure) => return Err(self.failed(0, reach, failure)),
                }
                stats.pages += reach >> self.page_shift;
                stats.calls += 1;
            }
            ResetPolicy::Remap => {
                match remap_range(base, reach) {
                    Ok(method) => stats.method = method,
                    Err(failure) => return Err(self.failed(0, reach, failure)),
                }
                stats.pages += reach >> self.page_shift;
                stats.calls += 1;
            }
            ResetPolicy::DiscardDirty | ResetPolicy::RemapDirty | ResetPolicy::Memset => {
                // Each run is reset as the bitmap scan finds it; collecting the runs first cost
                // an allocation per reset, on a path that takes a few hundred nanoseconds.
                let page_shift = self.page_shift;
                // the first failed kernel call; the runs after it are left as they are
                let mut failure = None;
                self.dirty_runs_in(words, |first, count| {
                    if failure.is_some() {
                        return;
                    }
                    let ptr = base.wrapping_add(first << page_shift);
                    let len = count << page_shift;
                    let method = match policy {
                        ResetPolicy::DiscardDirty => discard_range(ptr, len),
                        ResetPolicy::RemapDirty => remap_range(ptr, len),
                        _ => {
                            // SAFETY: the run is inside the mapping owned by `self`; the bitmap
                            // the scan reads is not part of that mapping.
                            unsafe { std::ptr::write_bytes(ptr, 0, len) };
                            Ok("memset")
                        }
                    };
                    match method {
                        Ok(method) => {
                            stats.method = method;
                            stats.pages += count;
                            stats.calls += 1;
                        }
                        Err(err) => failure = Some((first << page_shift, len, err)),
                    }
                });
                if let Some((offset, len, err)) = failure {
                    return Err(self.failed(offset, len, err));
                }
            }
            ResetPolicy::Adaptive { .. } => unreachable!("resolved above"),
        }

        let reach = ((self.high_water + page_size - 1) & !(page_size - 1)).min(self.capacity);
        if self.dirty_pages > 0 {
            self.dirty.fill(0);
            self.dirty_pages = 0;
        }
        self.high_water = 0;
        if verify && !self.is_zeroed(reach) {
            return Err(io::Error::other(
                "rwasm: memory slot is not all zeros after reset",
            ));
        }
        Ok(stats)
    }

    /// Takes note of what a failed reset step leaves behind and returns its error.
    fn failed(&mut self, offset: usize, len: usize, failure: ResetFailure) -> io::Error {
        if failure.remap {
            self.hole = Some((offset, offset + len));
        }
        failure.error
    }

    /// Whether the first `len` bytes are all zero.
    pub fn is_zeroed(&self, len: usize) -> bool {
        let bytes = self.as_slice(len.min(self.capacity));
        let (head, words, tail) = unsafe { bytes.align_to::<u64>() };
        head.iter().all(|byte| *byte == 0)
            && words.iter().all(|word| *word == 0)
            && tail.iter().all(|byte| *byte == 0)
    }
}

impl Drop for MemorySlot {
    fn drop(&mut self) {
        // A range a failed `mmap(MAP_FIXED)` may have unmapped is not this slot's any more:
        // another thread can have mapped something of its own there since. It is left alone,
        // which leaks it if the kernel kept the old pages, and the rest of the slot is given
        // back around it. Without a hole the two bounds are the capacity and this is one call.
        let (hole_start, hole_end) = self.hole.unwrap_or((self.capacity, self.capacity));
        let base = self.base.as_ptr();
        // SAFETY: the mapping was created by `reserve` with exactly this base and length, and
        // both ranges lie inside it and outside the hole.
        unsafe {
            if hole_start > 0 {
                libc::munmap(base.cast(), hole_start);
            }
            if hole_end < self.capacity {
                libc::munmap(base.add(hole_end).cast(), self.capacity - hole_end);
            }
        }
    }
}

/// A failed kernel call of a reset.
struct ResetFailure {
    error: io::Error,
    /// The call was a `mmap(MAP_FIXED)`, which may have removed the old pages of its range
    /// before it failed.
    remap: bool,
}

impl ResetFailure {
    fn last_os_error(remap: bool) -> Self {
        Self {
            error: io::Error::last_os_error(),
            remap,
        }
    }
}

/// Drops the physical pages of `[ptr, ptr + len)`; the next access reads zeros.
#[cfg(target_os = "linux")]
fn discard_range(ptr: *mut u8, len: usize) -> Result<&'static str, ResetFailure> {
    if len == 0 {
        return Ok("madvise(MADV_DONTNEED)");
    }
    // SAFETY: the caller passes a sub-range of a live anonymous private mapping.
    if unsafe { libc::madvise(ptr.cast(), len, libc::MADV_DONTNEED) } != 0 {
        return Err(ResetFailure::last_os_error(false));
    }
    Ok("madvise(MADV_DONTNEED)")
}

/// Zero-fills `[ptr, ptr + len)` with `MADV_ZERO`, or remaps it where the kernel lacks it.
///
/// `MADV_ZERO` zeroes resident pages in place and leaves them resident. Following it with
/// `MADV_FREE_REUSABLE` was measured and rejected: the kernel takes the pages back right away
/// and every reuse faults again (0.75 µs per 16 KiB page), which made a 64 MiB range reset cost
/// 48 µs instead of 17 µs and a sparse touch afterwards 3 µs instead of 0.03 µs. macOS
/// `MADV_DONTNEED` is a hint that keeps the contents, so it is never used here.
#[cfg(target_vendor = "apple")]
fn discard_range(ptr: *mut u8, len: usize) -> Result<&'static str, ResetFailure> {
    use std::sync::atomic::AtomicBool;
    static MADV_ZERO_SUPPORTED: AtomicBool = AtomicBool::new(true);
    if len == 0 {
        return Ok("madvise(MADV_ZERO)");
    }
    if MADV_ZERO_SUPPORTED.load(Ordering::Relaxed) {
        // SAFETY: the caller passes a sub-range of a live anonymous private mapping.
        if unsafe { libc::madvise(ptr.cast(), len, libc::MADV_ZERO) } == 0 {
            return Ok("madvise(MADV_ZERO)");
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINVAL) && error.raw_os_error() != Some(libc::ENOTSUP)
        {
            return Err(ResetFailure {
                error,
                remap: false,
            });
        }
        MADV_ZERO_SUPPORTED.store(false, Ordering::Relaxed);
    }
    remap_range(ptr, len)
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
fn discard_range(ptr: *mut u8, len: usize) -> Result<&'static str, ResetFailure> {
    remap_range(ptr, len)
}

/// Replaces `[ptr, ptr + len)` with a fresh anonymous mapping.
///
/// `MAP_FIXED` is only ever applied inside a slot's own reservation, to pages the reset of its
/// small-page part covers. The new mapping is a new VMA, so the huge page opt-out is applied
/// again.
///
/// A fixed mapping is not atomic: the kernel removes what the range held and can still fail to
/// put the new mapping there, which leaves the range unmapped and free for any other `mmap` of
/// the process. A failure is therefore reported as a possible hole, and the slot never unmaps
/// that range again (`MemorySlot::drop`).
fn remap_range(ptr: *mut u8, len: usize) -> Result<&'static str, ResetFailure> {
    if len == 0 {
        return Ok("mmap(MAP_FIXED)");
    }
    // SAFETY: the caller passes a sub-range of a mapping this module reserved and owns, so the
    // fixed mapping replaces only pages that belong to the slot.
    let mapped = unsafe {
        libc::mmap(
            ptr.cast(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_FIXED | libc::MAP_NORESERVE,
            -1,
            0,
        )
    };
    if mapped == libc::MAP_FAILED {
        return Err(ResetFailure::last_os_error(true));
    }
    assert_eq!(mapped.cast::<u8>(), ptr, "MAP_FIXED moved the mapping");
    #[cfg(target_os = "linux")]
    // SAFETY: the range was just mapped.
    unsafe {
        libc::madvise(mapped, len, libc::MADV_NOHUGEPAGE);
    }
    Ok("mmap(MAP_FIXED)")
}

/// Maps `len` bytes of anonymous private read-write memory at an address that is a multiple of
/// `align` (a power of two, at least the host page size).
fn map_aligned(len: usize, align: usize) -> io::Result<*mut u8> {
    let map = |len: usize| -> io::Result<*mut u8> {
        // SAFETY: an anonymous mapping at an address of the kernel's choice has no preconditions.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(ptr.cast())
    };
    if align <= host_page_size() {
        return map(len);
    }
    // over-reserve by the alignment and give the unused head and tail back
    let total = len
        .checked_add(align)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let ptr = map(total)?;
    // The bytes up to the next multiple of `align`, computed from the address. This is not
    // `pointer::align_offset`: that method may return `usize::MAX` instead of an offset, and
    // the ranges unmapped below have to be exact.
    let head = ptr.addr().wrapping_neg() & (align - 1);
    let tail = total - head - len;
    // SAFETY: the head and the tail are page-aligned sub-ranges of the mapping just created and
    // nothing refers to them.
    unsafe {
        if head > 0 {
            libc::munmap(ptr.cast(), head);
        }
        if tail > 0 {
            libc::munmap(ptr.add(head + len).cast(), tail);
        }
    }
    Ok(ptr.wrapping_add(head))
}

struct PoolInner {
    config: MemoryPoolConfig,
    free: Mutex<Vec<MemorySlot>>,
    stats: MemoryPoolStats,
    last_reset: Mutex<Option<ResetStats>>,
}

/// A pool of reset [`MemorySlot`]s shared by the stores of a host.
///
/// Cloning shares the pool. Leasing pops a free slot or reserves a new one; the pool never
/// limits the number of slots in flight, that is the host's job (Fluentbase bounds the memory
/// of its suspended frames already).
#[derive(Clone)]
pub struct MemoryPool {
    inner: Arc<PoolInner>,
}

impl MemoryPool {
    /// Creates an empty pool.
    pub fn new(config: MemoryPoolConfig) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                config,
                free: Mutex::new(Vec::new()),
                stats: MemoryPoolStats::default(),
                last_reset: Mutex::new(None),
            }),
        }
    }

    /// The pool's configuration.
    pub fn config(&self) -> &MemoryPoolConfig {
        &self.inner.config
    }

    /// The pool's counters.
    pub fn stats(&self) -> &MemoryPoolStats {
        &self.inner.stats
    }

    /// What the most recent reset did.
    pub fn last_reset(&self) -> Option<ResetStats> {
        *self
            .inner
            .last_reset
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Slots currently waiting on the free list.
    pub fn free_slots(&self) -> usize {
        self.inner
            .free
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Capacity of a slot in bytes, saturating where the target's address space cannot express
    /// it (such a slot never maps).
    pub fn slot_capacity(&self) -> usize {
        (self.inner.config.slot_pages as usize)
            .saturating_mul(crate::N_BYTES_PER_MEMORY_PAGE as usize)
    }

    /// Leases an all-zero slot.
    ///
    /// # Errors
    ///
    /// The `mmap` of a new slot failed.
    pub fn lease(&self) -> io::Result<MemoryLease> {
        let free = self
            .inner
            .free
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop();
        let slot = match free {
            Some(slot) => slot,
            None => MemorySlot::reserve(
                self.slot_capacity(),
                self.inner.config.track_dirty,
                self.inner.config.small_page_prefix,
            )
            .inspect(|_| MemoryPoolStats::bump(&self.inner.stats.slots_reserved))
            .inspect_err(|_| MemoryPoolStats::bump(&self.inner.stats.map_failures))?,
        };
        MemoryPoolStats::bump(&self.inner.stats.leases);
        Ok(MemoryLease {
            slot: ManuallyDrop::new(slot),
            pool: self.inner.clone(),
        })
    }

    /// Leases an all-zero slot for a memory that may grow to `max_pages` Wasm pages.
    ///
    /// Returns `None`, and counts the lease as declined, when the pool's slots are smaller than
    /// that. The caller then backs the memory some other way: a memory growing inside a slot it
    /// can outgrow stops at the slot's capacity instead of at `max_pages`, which the guest sees
    /// as a `memory.grow` that returns `-1` early.
    ///
    /// # Errors
    ///
    /// The `mmap` of a new slot failed.
    pub fn lease_for(&self, max_pages: u32) -> io::Result<Option<MemoryLease>> {
        if max_pages > self.inner.config.slot_pages {
            MemoryPoolStats::bump(&self.inner.stats.leases_declined);
            return Ok(None);
        }
        self.lease().map(Some)
    }
}

impl PoolInner {
    /// Resets `slot` and pools it, or unmaps it when the free list has no room for it or the
    /// reset cannot vouch for it.
    fn release(&self, mut slot: MemorySlot) {
        // A slot the free list has no room for is unmapped as it is: `munmap` gives every page
        // back, so a reset first would be wasted work. The list can fill up between this check
        // and the push below, which is why the push checks again.
        let free_slots = self.free.lock().unwrap_or_else(|e| e.into_inner()).len();
        if free_slots >= self.config.max_free_slots {
            MemoryPoolStats::bump(&self.stats.slots_unmapped);
            return;
        }
        match slot.reset(self.config.reset_policy, self.config.verify_reset) {
            Ok(stats) => {
                *self.last_reset.lock().unwrap_or_else(|e| e.into_inner()) = Some(stats);
                let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
                if free.len() < self.config.max_free_slots {
                    free.push(slot);
                    MemoryPoolStats::bump(&self.stats.slots_recycled);
                    return;
                }
            }
            Err(_) => MemoryPoolStats::bump(&self.stats.reset_failures),
        }
        MemoryPoolStats::bump(&self.stats.slots_unmapped);
        drop(slot);
    }
}

/// A slot on loan from a [`MemoryPool`]; returns and resets it when dropped.
pub struct MemoryLease {
    /// Taken out exactly once, in `Drop`; no `Option` check on the VM's memory access path.
    slot: ManuallyDrop<MemorySlot>,
    pool: Arc<PoolInner>,
}

impl MemoryLease {
    /// The leased slot.
    #[inline(always)]
    pub fn slot(&self) -> &MemorySlot {
        &self.slot
    }

    /// The leased slot, mutably. Crate-private: a slot that goes back to the pool must have
    /// every write recorded, which [`crate::GlobalMemory::pooled`] guarantees for the holder of
    /// a lease and the raw accessors of [`MemorySlot`] do not.
    #[inline(always)]
    pub(crate) fn slot_mut(&mut self) -> &mut MemorySlot {
        &mut self.slot
    }
}

impl Drop for MemoryLease {
    fn drop(&mut self) {
        // SAFETY: `slot` is taken here and nowhere else, and `self` is never used again.
        let slot = unsafe { ManuallyDrop::take(&mut self.slot) };
        self.pool.release(slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(pages: usize) -> MemorySlot {
        MemorySlot::reserve(pages * host_page_size(), true, usize::MAX).unwrap()
    }

    const POLICIES: [ResetPolicy; 6] = [
        ResetPolicy::Discard,
        ResetPolicy::Remap,
        ResetPolicy::DiscardDirty,
        ResetPolicy::RemapDirty,
        ResetPolicy::Memset,
        ResetPolicy::Adaptive {
            memset_up_to_pages: 1,
        },
    ];

    #[test]
    fn marks_host_pages_of_a_write() {
        let page = host_page_size();
        let mut slot = slot(8);
        slot.mark_dirty(page * 2 + 1, 1);
        slot.mark_dirty(page * 6 - 1, 2); // straddles pages 5 and 6
        slot.mark_dirty(0, 0); // empty write marks nothing
        assert_eq!(slot.dirty_pages(), 3);
        let mut runs = Vec::new();
        slot.for_each_dirty_run(|first, count| runs.push((first, count)));
        assert_eq!(runs, [(2, 1), (5, 2)]);
    }

    /// `mark_dirty` is a safe function, so a range outside the mapping has to be harmless. A
    /// one-page slot still has a 64-bit bitmap word; a mark of the page after the mapping used
    /// to stick there and send the bitmap-driven resets past the slot.
    #[test]
    fn marks_outside_the_mapping_are_ignored() {
        let page = host_page_size();
        let mut slot = slot(1);
        slot.mark_dirty(page, 1); // the page right after the mapping
        slot.mark_dirty(page * 63, page); // the last bit of the bitmap word
        slot.mark_dirty(usize::MAX, 2); // a range that overflows
        slot.mark_dirty(usize::MAX - 1, usize::MAX);
        assert_eq!(slot.dirty_pages(), 0);
        // a range that starts inside the mapping is cut at its end
        slot.mark_dirty(page - 1, page * 10);
        assert_eq!(slot.dirty_pages(), 1);
        let mut runs = Vec::new();
        slot.for_each_dirty_run(|first, count| runs.push((first, count)));
        assert_eq!(runs, [(0, 1)]);
        slot.note_accessible(page);
        slot.as_mut_slice(page)[page - 1] = 7;
        slot.reset(ResetPolicy::Memset, true).unwrap();
        assert!(slot.is_zeroed(page));
    }

    #[test]
    fn every_policy_returns_the_reachable_range_to_zero() {
        let page = host_page_size();
        for policy in POLICIES {
            let mut slot = slot(16);
            slot.note_accessible(page * 12);
            for offset in [0, page * 3 + 7, page * 11 + page - 1] {
                slot.as_mut_slice(page * 12)[offset] = 0xAB;
                slot.mark_dirty(offset, 1);
            }
            let stats = slot.reset(policy, true).unwrap();
            assert!(stats.pages > 0, "{policy:?}: {stats:?}");
            assert_eq!(stats.dirty_pages, 3, "{policy:?}");
            assert!(slot.is_zeroed(page * 16), "{policy:?}");
            assert_eq!(slot.dirty_pages(), 0);
            assert_eq!(slot.high_water(), 0);
        }
    }

    #[test]
    fn dirty_runs_cross_bitmap_words() {
        let page = host_page_size();
        let mut slot = slot(4 * 64 + 8);
        slot.mark_dirty(page * 60, page * 70); // pages 60..130: spans three words
        slot.mark_dirty(page * 200, 1);
        slot.mark_dirty(page * (4 * 64 + 7), 1); // the last page
        let mut runs = Vec::new();
        slot.for_each_dirty_run(|first, count| runs.push((first, count)));
        assert_eq!(runs, [(60, 70), (200, 1), (4 * 64 + 7, 1)]);
        assert_eq!(slot.dirty_pages(), 72);
    }

    /// A slot with a huge-page part: aligned, and both parts come back as zeros under every
    /// policy whether the lease reached into the huge-page part or not. Without transparent huge
    /// pages (macOS) the slot has no such part and the same writes go through the policy alone.
    #[test]
    fn split_slot_resets_both_parts() {
        let page = host_page_size();
        let huge = huge_page_size();
        let capacity = if huge > 0 { huge * 4 } else { page * 512 };
        for policy in POLICIES {
            for reach in [capacity / 8, capacity] {
                let mut slot = MemorySlot::reserve(capacity, true, 1).unwrap();
                if huge > 0 {
                    assert_eq!(slot.huge_pages_from(), Some(huge));
                    assert_eq!(slot.base_ptr().as_ptr() as usize % huge, 0);
                } else {
                    assert_eq!(slot.huge_pages_from(), None);
                }
                slot.note_accessible(reach);
                for offset in [0, reach / 2 + 3, reach - 1] {
                    slot.as_mut_slice(reach)[offset] = 0xCD;
                    slot.mark_dirty(offset, 1);
                }
                let stats = slot.reset(policy, true).unwrap();
                assert!(stats.calls > 0, "{policy:?} reach={reach}: {stats:?}");
                assert!(slot.is_zeroed(capacity), "{policy:?} reach={reach}");
                assert_eq!(slot.dirty_pages(), 0);
                // and again: a reset slot is reusable
                slot.note_accessible(capacity);
                slot.as_mut_slice(capacity)[capacity - 1] = 1;
                slot.mark_dirty(capacity - 1, 1);
                slot.reset(policy, true).unwrap();
                assert!(slot.is_zeroed(capacity), "{policy:?} second reset");
            }
        }
    }

    /// The reset must not depend on the huge page being a whole number of bitmap words. With a
    /// huge page of four host pages the split lands inside the first word; rounding the
    /// small-page words down then left the small pages out of every bitmap-driven reset.
    #[test]
    fn small_page_part_is_reset_when_the_split_is_inside_a_bitmap_word() {
        let page = host_page_size();
        let capacity = page * 256;
        for policy in POLICIES {
            let mut slot = MemorySlot::reserve_with_huge_page(capacity, true, 1, page * 4).unwrap();
            assert_eq!(slot.huge_pages_from(), Some(page * 4), "{policy:?}");
            slot.note_accessible(capacity);
            // two small pages, the first page past the split and two pages further up
            for offset in [page + 5, page * 3, page * 4, page * 100, capacity - 1] {
                slot.as_mut_slice(capacity)[offset] = 0xEE;
                slot.mark_dirty(offset, 1);
            }
            slot.reset(policy, true)
                .unwrap_or_else(|err| panic!("{policy:?}: {err}"));
            assert!(slot.is_zeroed(capacity), "{policy:?}");
        }
    }

    /// A slot with a huge-page part starts on a multiple of the huge page size and is mapped
    /// over exactly its capacity. Built through `reserve_with_huge_page`, so the aligned mapping
    /// is covered on hosts without transparent huge pages as well.
    #[test]
    fn split_slots_are_aligned_and_mapped_over_their_whole_capacity() {
        let page = host_page_size();
        for align_pages in [2, 8, 64, 512] {
            let align = page * align_pages;
            let capacity = align * 3;
            // several slots at once, so that the kernel hands out different addresses
            let mut slots: Vec<MemorySlot> = (0..8)
                .map(|_| MemorySlot::reserve_with_huge_page(capacity, true, 1, align).unwrap())
                .collect();
            for slot in &mut slots {
                assert_eq!(slot.base_ptr().as_ptr() as usize % align, 0, "{align}");
                assert_eq!(slot.capacity(), capacity);
                let bytes = slot.as_mut_slice(capacity);
                bytes[0] = 1;
                bytes[capacity - 1] = 1;
                assert_eq!((bytes[0], bytes[capacity - 1]), (1, 1));
            }
        }
    }

    /// A slot larger than the largest Wasm memory is refused without asking the kernel, on every
    /// host: a wide address space could grant the mapping, and the bitmap for it would follow.
    #[test]
    fn a_slot_larger_than_any_wasm_memory_is_refused() {
        // a 32-bit target cannot even express the size
        if let Ok(largest) = usize::try_from(MAX_SLOT_BYTES) {
            let err = MemorySlot::reserve(largest + host_page_size(), true, usize::MAX)
                .err()
                .expect("the slot must not map");
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        }

        let pool = MemoryPool::new(MemoryPoolConfig {
            slot_pages: 65537,
            ..MemoryPoolConfig::default()
        });
        assert_eq!(
            pool.lease().err().map(|err| err.kind()),
            Some(io::ErrorKind::InvalidInput)
        );
        assert!(pool.lease_for(1).is_err());
        assert_eq!(pool.stats().map_failures.load(Ordering::Relaxed), 2);
        assert_eq!(pool.stats().slots_reserved.load(Ordering::Relaxed), 0);
    }

    /// A write through the raw slice is reset by the range policies even when the caller never
    /// told the slot how far it reached: handing the slice out did.
    #[test]
    fn raw_slice_writes_are_inside_the_reset_range() {
        for policy in POLICIES {
            let pool = MemoryPool::new(MemoryPoolConfig {
                slot_pages: 1,
                reset_policy: policy,
                verify_reset: true,
                ..MemoryPoolConfig::default()
            });
            let mut lease = pool.lease().unwrap();
            lease.slot_mut().as_mut_slice(16)[3] = 0xAA;
            lease.slot_mut().mark_dirty(3, 1);
            assert_eq!(lease.slot().high_water(), 16);
            drop(lease);
            assert_eq!(
                pool.stats().reset_failures.load(Ordering::Relaxed),
                0,
                "{policy:?}"
            );
            let lease = pool.lease().unwrap();
            assert_eq!(pool.stats().slots_reserved.load(Ordering::Relaxed), 1);
            assert!(
                lease.slot().is_zeroed(lease.slot().capacity()),
                "{policy:?}"
            );
        }
    }

    /// A slot that lost a range to a failed remap gives back the pages around it and leaves the
    /// range itself alone: by then it may hold a mapping of someone else.
    #[test]
    fn drop_leaves_a_possible_hole_alone() {
        let page = host_page_size();
        // a failed call that cannot leave a hole records nothing
        let mut intact = slot(1);
        intact.failed(
            0,
            page,
            ResetFailure {
                error: io::Error::from(io::ErrorKind::InvalidInput),
                remap: false,
            },
        );
        assert!(intact.hole.is_none());
        drop(intact);

        // what `reset` records when the remap of pages 2 to 4 fails
        let mut holed = slot(8);
        let base = holed.base_ptr().as_ptr();
        let failure = ResetFailure {
            error: io::Error::from(io::ErrorKind::OutOfMemory),
            remap: true,
        };
        let error = holed.failed(2 * page, 3 * page, failure);
        assert_eq!(error.kind(), io::ErrorKind::OutOfMemory);
        assert_eq!(holed.hole, Some((2 * page, 5 * page)));
        drop(holed);
        // The kernel kept the old pages here, so the range is still mapped (and is this test's
        // to give back). `mincore` fails with ENOMEM on a range that is not fully mapped.
        let mut residency = vec![0u8; 3];
        // SAFETY: `mincore` only inspects the page tables of the range.
        let mapped = unsafe {
            libc::mincore(
                base.add(2 * page).cast(),
                3 * page,
                residency.as_mut_ptr().cast(),
            )
        };
        assert_eq!(mapped, 0, "{}", io::Error::last_os_error());
        // SAFETY: the range is the part of the slot's mapping its drop left alone.
        unsafe { libc::munmap(base.add(2 * page).cast(), 3 * page) };
    }

    #[test]
    fn untracked_slot_still_resets_the_reachable_range() {
        let page = host_page_size();
        let mut slot = MemorySlot::reserve(page * 4, false, usize::MAX).unwrap();
        slot.note_accessible(page * 4);
        slot.as_mut_slice(page * 4)[page * 3] = 1;
        slot.mark_dirty(page * 3, 1);
        assert_eq!(slot.dirty_pages(), 0);
        slot.reset(ResetPolicy::Memset, true).unwrap();
        assert!(slot.is_zeroed(page * 4));
    }

    #[test]
    fn pool_recycles_reset_slots_and_caps_the_free_list() {
        let pool = MemoryPool::new(MemoryPoolConfig {
            slot_pages: 1,
            max_free_slots: 1,
            verify_reset: true,
            ..MemoryPoolConfig::default()
        });
        let mut first = pool.lease().unwrap();
        let second = pool.lease().unwrap();
        first.slot_mut().note_accessible(64);
        first.slot_mut().as_mut_slice(64)[5] = 9;
        first.slot_mut().mark_dirty(5, 1);
        drop(first);
        drop(second);
        assert_eq!(pool.free_slots(), 1);
        assert_eq!(pool.stats().slots_reserved.load(Ordering::Relaxed), 2);
        assert_eq!(pool.stats().slots_recycled.load(Ordering::Relaxed), 1);
        assert_eq!(pool.stats().slots_unmapped.load(Ordering::Relaxed), 1);
        // the slot the free list had no room for was unmapped as it was: the last reset is
        // still the one of the first slot, with its dirty page
        assert_eq!(pool.last_reset().map(|stats| stats.dirty_pages), Some(1));
        let reused = pool.lease().unwrap();
        assert_eq!(pool.stats().slots_reserved.load(Ordering::Relaxed), 2);
        assert!(reused.slot().is_zeroed(64));
    }
}

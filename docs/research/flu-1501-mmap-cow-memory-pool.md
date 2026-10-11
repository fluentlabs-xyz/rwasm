# mmap-backed memory pools for rwasm instance reuse (FLU-1501)

Research report and implementation, 2026-09-30, revised 2026-10-01 after the hardening pass
(section 13). The implementation is the `memory-pool` cargo feature: `src/vm/memory_pool.rs`, the
pooled backing and the tracked-write API in `src/vm/memory.rs`, the lease plumbing in
`src/vm/store.rs` and `src/strategy/module.rs`, the equivalence tests in `tests/memory_pool.rs`,
the pooled executor in the `resume_equivalence` fuzz target and the measurement harness in
`benches/memory_pool.rs`. The pool is inert without the feature, on non-Unix targets and in
`no_std` builds.

## 0. Summary and recommendation

**Recommendation: implement, in a modified form; the implementation is on this branch.** Pool the
linear memory only, lease one slot per instance, and reset a released slot from host-owned
metadata. Do not build an instance template, a copy-on-write initial image or a "reset the whole
store" primitive: every other piece of instance state is rebuilt per call for well under a
microsecond, and re-running the compiled prologue on an all-zero slot reproduces the
post-instantiation image by construction.

What the measurements say (Linux arm64 with 4 KiB pages in a Docker VM, macOS on an Apple M5 Max
with 16 KiB pages; one contract call = new store, instantiation, the call, release):

| call | today (`Vec`) Linux / macOS | pooled, default configuration Linux / macOS |
|---|---:|---:|
| F: instantiate a 17-page contract, no call | 4.55 / 4.52 µs | 0.39 / 0.54 µs |
| D: typical call, 64 KiB of stack + 8 KiB of data | 5.54 / 5.21 µs | 1.83 / 1.59 µs |
| E: a call that writes its whole 1 MiB stack | 18.1 / 12.2 µs | 18.6 / 12.4 µs |
| A: grow to 64 MiB, write 4 pages | 886 / 317 µs | 22.0 / 1.15 µs |
| B: grow to 64 MiB, write 256 pages 256 KiB apart | 750 / 331 µs | 389 / 50.6 µs |
| C: grow to 64 MiB, fill all of it | 1694 / 801 µs | 1340 / 1007 µs |

- Today's cost is `memset` of the *declared* memory (`GlobalMemory::grow` reserves and
  `resize`s a `Vec`): 4 µs per MiB when the allocator recycles the block, 880 µs for a 64 MiB
  memory on Linux, where the block comes from fresh `mmap` pages every time.
- With a pooled slot the cost follows the *touched* pages. Every row is at parity or better on
  Linux; the one row that loses is the dense 64 MiB fill on macOS (1.26x), a development
  platform.
- The reset primitive matters more than the ticket assumed. On both platforms a page fault costs
  more than zeroing the page by hand (Linux: 330 ns per 4 KiB fault against 20 to 40 ns of
  `memset`; macOS: 0.6 to 2 µs per 16 KiB fault). So the best policy for the common case is not
  `madvise(MADV_DONTNEED)` but `memset` of the dirty pages, which keeps them resident for the
  next lease; the kernel primitives take over where memory has to be given back.
- Page size matters as much. A slot on 4 KiB pages makes a sparse working set cheap and a dense
  one expensive (16384 faults to fill 64 MiB, 4.6x slower than today); transparent huge pages do
  the opposite. Slots are therefore split on Linux: small pages for the first 8 MiB, where every
  contract's stack and data live, huge-page eligible above. The dense fill is then faster than
  today (1340 against 1694 µs), and a sparse touch of the upper part costs a 2 MiB fault
  (row A: 22.0 µs, against 0.78 µs on small pages and 886 µs today).
- Dirty tracking cannot be skipped by a write path: the memory hands out every write window
  itself and marks it (section 5). Its cost is inside the run-to-run noise on both platforms
  (within 3% on a loop that is nothing but stores, 0% on code without stores).
- The correctness invariant holds. `tests/memory_pool.rs` runs eight prior executions (sparse and
  dense writes, growth, bulk operations with segment drops, a trap, `OutOfFuel`, an abandoned and
  a resumed interruption, a dense write set followed by a trap) under every reset policy, with
  and without tracking, on small-page and split slots, then compares the recycled instance
  against a fresh `Vec` instance step by step: post-instantiation image, prologue fuel, results,
  fuel, memory, tables and globals after each of 22 probe calls including another trap. The
  `resume_equivalence` fuzz target does the same for generated modules on slots recycled between
  fuzz iterations. Every reset in both is read back and none failed.

Open: Linux x86-64 numbers (no such machine was in reach; the harness runs unchanged there, see
appendix B), the two-line Fluentbase change that turns the pool on (section 11.2), an audit
round over the new `unsafe` code, and the Wasmtime side (section 11.4).

## 1. Proposed memory-pool architecture

### 1.1 What is expensive and what is not

Per contract call Fluentbase creates a `ContractRuntime`, which is a `RwasmStore` and an
`RwasmInstance` (`fluentbase/crates/runtime/src/executor.rs`, `contract_runtime.rs`). Instance
creation does the following (`src/vm/instance.rs`, `src/vm/store.rs`):

| step | what it allocates | cost, 17-page contract |
|---|---|---:|
| `RwasmStore::new` | two empty hash maps, two empty bitsets, a 0-page `GlobalMemory` | ~0.2 µs |
| `begin_instantiation` | swaps in an empty memory, tables, globals and segment flags | ~0.1 µs |
| the compiled prologue (`ExecutionEngine::entrypoint`) | value stack (32 slots) and call stack; runs `memory.grow`, `memory.init` per active data segment, `table.grow`/`table.init`, `global.set` | 0.1 µs plus the memory |
| the `memory.grow` inside it | `GlobalMemory::grow`: `try_reserve_exact` and `resize(_, 0)`, i.e. `memset` of the declared size | 4 µs (1 MiB) to 860 µs (64 MiB) |
| the `memory.init`s inside it | `memcpy` of the module's data segments | 12 bytes to 50 KiB for every Fluentbase module but two |
| drop | `free` (a `munmap` for large blocks) | in the `alloc+reset` column of appendix A |

The only step whose cost scales with something the guest controls is the memory. Every
Fluentbase contract and system runtime declares 17 pages: `wasm-ld` reserves a 1 MiB shadow
stack (`contracts/.cargo/config.toml`, `-zstack-size=1048576`) and places 1 KiB to 50 KiB of data
behind it (survey of the 37 modules in `target/contracts`; `secp256k1` carries 1 MiB of tables
and `checkmate` 750 KiB). A call touches a few pages of the stack and the data; a call that grows
memory is fuel-priced per page but zeroes the whole growth up front.

That is the case the ticket describes: reset cost proportional to the total allocated memory.
The rest of the instance state is a few hundred bytes and is rebuilt per call today; the
pool leaves that as it is.

### 1.2 Lease model

```
RwasmModule (Arc, immutable)
      │
      ▼
RwasmStore<T>  ── one per contract call, owns tables/globals/segment flags/fuel/host context
      │
      ├── GlobalMemory ─┬─ Vec<u8>           (no_std, zkVM, hosts without a pool)
      │                 └─ MemoryLease ──▶ MemorySlot (mmap, dirty bitmap, high-water mark)
      │                                          ▲ lease / release
      └── memory_pool: Option<MemoryPool> ───────┘
                                                 │
                                       MemoryPool: Mutex<Vec<MemorySlot>> free list, config, stats
```

- `RwasmStore::with_memory_pool(pool)` records the pool. Nothing is leased yet.
- `RwasmStore::begin_instantiation` (called by `RwasmInstance::new`) leases a slot instead of
  allocating a `Vec` (`fresh_memory`). The prologue then grows and initializes it exactly as it
  grows and initializes a `Vec`: `GlobalMemory::grow` on a pooled memory only widens the accessible
  prefix, because a leased slot is all zeros by contract.
- The previous instance's memory goes into `pending_instance` as before and is dropped when the
  replacement commits, or restored when it rolls back. Dropping a `GlobalMemory` drops its
  `MemoryLease`, which resets the slot and returns it to the pool; a slot whose reset fails, or
  that fails verification when enabled, is unmapped instead of pooled.
- Dropping the store at the end of the call releases the last slot the same way. A suspended
  frame keeps its store and therefore its slot, as it keeps its `Vec` today. Nested frames are
  separate stores and get separate leases with no extra code.

A store that is never instantiated through `RwasmInstance` (the legacy direct engine API and a
few tests) keeps its `Vec`.

### 1.3 Why not an instance template and a full reset

The ticket lists everything a reusable instance would have to restore: memory, memory size,
globals, tables, dropped segments, stacks, fuel, trap state. In rwasm none of that lives in the
memory slot: it lives in the store (`tables`, `global_variables`, `empty_data_segments`,
`empty_elem_segments`, `last_signature`, `resumable_context`, `consumed_fuel`) and in the
per-execution `ValueStack`/`CallStack`, which `ExecutionEngine` allocates per call anyway. All of
it is rebuilt from scratch by `RwasmStore::new` plus the prologue in about half a microsecond
(row "instantiate only" above: 0.39 µs with a pooled slot, and that includes leasing the slot,
running the prologue and releasing the slot).

A template that snapshots the post-instantiation state and copies it back would have to be kept
in sync with the prologue's semantics (which segments it drops, which fuel it charges) and would
introduce a second way of producing an instance, which is exactly the kind of divergence the
zkVM path cannot afford. Re-running the prologue is deterministic, already fuel-metered
(Fluentbase relies on the prologue's `memory.grow` charge for the initial memory,
`rwasm_initializer_charges_initial_memory_fuel`), and produces the fresh state by definition.
`RwasmStore::reset(keep_flags)` keeps its current fuzzing semantics and is not involved.

So the "fresh-instance-equivalent primitive" of the ticket is: *a new store, a leased slot that
is all zeros, the prologue*. The only invariant the pool has to keep is "a leased slot reads as
zeros everywhere", and that is what the reset and the tests are about.

### 1.4 API

```rust
let pool = MemoryPool::new(MemoryPoolConfig {
    slot_pages: N_DEFAULT_MAX_MEMORY_PAGES, // 64 MiB of address space per slot
    max_free_slots: 8,
    reset_policy: MemoryPoolConfig::default_reset_policy(), // memset ≤ 8 MiB dirty, discard above
    track_dirty: true,
    verify_reset: cfg!(debug_assertions), // read the range back after every reset
    small_page_prefix: 8 << 20,           // Linux: huge-page eligible above 8 MiB
});

// through the strategy layer, as Fluentbase creates its executors
let mut executor = definition.create_executor_with_memory_pool(
    linker.clone(), ctx, handler, fuel, Some(pages), &pool)?;   // leases a slot
executor.execute("main", &params, &mut results)?;
drop(executor);                                                   // resets and returns it

// or on a store directly
let mut store = RwasmStore::new(linker.clone(), ctx, handler, fuel, Some(pages))
    .with_memory_pool(pool.clone());
let instance = linker.instantiate(&mut store, engine, module)?;
```

`MemorySlot` is the `VirtualMemoryBackend` of the ticket, with three differences: `grow` is a
bookkeeping call (`note_accessible`) because the whole slot is mapped read-write up front and
bounds are enforced by the interpreter, not by page protection; `reset` takes a policy; and the
dirty bitmap lives in the slot. `MemoryPool::stats()` and `last_reset()` expose counters for the
host and for the harness.

## 2. Linux design: `mmap` and `madvise`

**Reservation.** One `mmap(NULL, slot_pages * 64 KiB, PROT_READ | PROT_WRITE, MAP_PRIVATE |
MAP_ANON | MAP_NORESERVE)` per slot, followed by `madvise(MADV_NOHUGEPAGE)`. The slot is never
`mprotect`ed: rwasm bounds-checks every access in software against `current_pages`, so page
protection would add a syscall per `memory.grow` and buy nothing. `MAP_NORESERVE` keeps 64 MiB
slots out of the overcommit accounting; the Docker VM runs with `overcommit_memory = 1` anyway.

**Growth.** `GlobalMemory::grow` checks the store's `max_allowed_memory_pages` (as today), then
the slot's capacity, then sets the accessible length. No zeroing, no syscall. Untouched pages
cost nothing until written; a read of an untouched page maps the shared zero page.

**Reset.** Measured over a 64 MiB slot (table 1 in appendix A, Linux). Each cell is the reset
in µs, then the µs the *next* lease spends touching the same pages again, then the number of
kernel calls (or `memset` runs):

| primitive | 4 dirty pages | 256 dirty pages, 256 KiB apart | every page (16384) |
|---|---:|---:|---:|
| one kernel call over the reachable range | 1.71 / 1.17 (1) | 24.9 / 99.1 (1) | 756 / 5768 (1) |
| one `mmap(MAP_FIXED)` over the reachable range | 2.87 / 2.64 (1) | 23.3 / 96.4 (1) | 795 / 5863 (1) |
| kernel call per dirty run | 2.25 / 1.15 (4) | 127 / 80.9 (256) | 746 / 5713 (1) |
| `mmap(MAP_FIXED)` per dirty run | 5.90 / 1.12 (4) | 355 / 87.7 (256) | 808 / 5933 (1) |
| `memset` of the dirty pages | 0.40 / 0.02 (4) | 15.1 / 16.8 (256) | 301 / 222 (1) |
| `munmap` + `mmap` a new slot | 3.67 / 2.59 | 27.6 / 110 | 782 / 5747 |

- `MADV_DONTNEED` over the whole reachable range costs about 1.5 µs plus 46 ns per resident page:
  the kernel walks the page tables and skips empty page-middle directories two megabytes at a
  time, so a sparse working set is cheap without any bitmap. Per-run `madvise` calls cost
  0.4 to 0.5 µs each and lose against one range call as soon as there are more than a handful
  of runs (127 µs for 256 runs against 25 µs). `MADV_DONTNEED` takes `mmap_lock` for reading;
  `mmap(MAP_FIXED)` takes it for writing and splits/merges VMAs, and it creates a new VMA that
  has forgotten `MADV_NOHUGEPAGE` (the pool re-applies it; the first run of the harness,
  without that, showed a 4-page touch after a remap costing 28 µs because each touch faulted a
  2 MiB huge page).
- The discard is only half the price: every discarded page that the next call touches again
  faults, at about 330 ns per 4 KiB page in this VM. For the typical contract (18 dirty pages)
  that is 6 µs, more than the `Vec` path costs in total. `memset` of the same 18 pages takes
  about 1 µs, bitmap scan included, and keeps them resident, so the next call pays nothing.
- Hence the default policy, `Adaptive { memset_up_to_pages }`: zero the dirty pages by hand
  while the dirty set is at most 8 MiB (2048 pages here), otherwise one `MADV_DONTNEED` over the
  reachable range. Resident memory per pooled slot is bounded by the threshold.

**Split slots and transparent huge pages.** The VM runs with THP `always`. A `Vec` of 64 MiB is
populated with 2 MiB pages (33 faults for the whole `memory.fill`), a slot on 4 KiB pages with
16384 faults (5.7 ms), which made the dense row 4.6x worse than today
(7825 against 1694 µs). Huge pages everywhere would do the reverse to
the common case: every touched region would cost a 2 MiB zero-fill and hold 2 MiB resident.

So a slot is split (`MemoryPoolConfig::small_page_prefix`, 8 MiB by default). The prefix is
opted out of huge pages (`MADV_NOHUGEPAGE`) and follows the reset policy; it holds the 1 MiB
shadow stack, the data and a few megabytes of heap, which is all a typical contract touches. The
rest is huge-page eligible (`MADV_HUGEPAGE`), the slot is aligned to the huge page size for it,
and whatever a lease reached there is given back with one `MADV_DONTNEED` on release, because
keeping it would pin 2 MiB per touched page. The default `memset` threshold equals the prefix,
so the small-page part is always zeroed in place and never faulted in again. Effect on the
64 MiB workloads, Linux, µs per call:

| call | `Vec` today | pool, small pages only | pool, split at 8 MiB |
|---|---:|---:|---:|
| A: 4 sparse writes | 886 | 0.78 | 22.0 |
| B: 256 writes 256 KiB apart | 750 | 24.5 | 389 |
| C: fill 64 MiB | 1694 | 7825 | 1340 |

The split trades the sparse-but-far cases (a 2 MiB fault, 9 to 13 µs, per touched huge page) for
the dense one, and with it no workload is slower than today. A host that knows its contracts
never fill large memories can set the prefix to `usize::MAX` and keep the first column. Where
the kernel has no THP, or on macOS, the slot has no huge-page part and the setting is ignored.
The huge page size is read from `/sys/kernel/mm/transparent_hugepage/hpage_pmd_size`.

**Other primitives considered.** `MADV_FREE` after `memset` would let the kernel reclaim the
zeroed pages lazily (a reclaimed page reads back as zeros, so it is safe); it was not needed to
bound RSS with the 8 MiB threshold. `MADV_POPULATE_WRITE` on growth would prefault, which is the
`memset` cost again. Wasmtime 45 finds resident pages with the `PAGEMAP_SCAN` ioctl
(`crates/wasmtime/src/runtime/vm/sys/unix/pagemap.rs` in the fork) instead of a bitmap; that is
a Linux 6.7+ feature and needs `/proc/self/pagemap`, and the range `MADV_DONTNEED` already costs
the same as such a scan would save. A `memfd` copy-on-write image is discussed in section 6.

## 3. macOS design: `mmap` and page replacement

**Reservation** is the same call (`MAP_ANON | MAP_PRIVATE | MAP_NORESERVE`); there is no huge
page opt-out to apply. Apple Silicon pages are 16 KiB, so the bitmap has 4096 bits per 64 MiB.

**Reset.** `madvise(MADV_DONTNEED)` on Darwin is a hint that keeps the contents, so it is never
used. Two primitives return a range to zeros (cells as in the Linux table: reset µs / next touch
µs / calls):

| primitive | 4 dirty pages | 64 dirty pages, 256 KiB apart | every page (4096) |
|---|---:|---:|---:|
| one kernel call over the reachable range | 17.9 / 0.03 (1) | 24.1 / 10.1 (1) | 292 / 64.0 (1) |
| one `mmap(MAP_FIXED)` over the reachable range | 27.8 / 5.04 (1) | 25.2 / 47.7 (1) | 284 / 2152 (1) |
| kernel call per dirty run | 1.27 / 0.04 (4) | 17.8 / 10.2 (64) | 309 / 68.4 (1) |
| `mmap(MAP_FIXED)` per dirty run | 7.14 / 5.49 (4) | 53.8 / 49.0 (64) | 358 / 2355 (1) |
| `memset` of the dirty pages | 0.78 / 0.04 (4) | 10.3 / 9.74 (64) | 257 / 77.7 (1) |
| `munmap` + `mmap` a new slot | 28.7 / 5.87 | 25.6 / 44.3 | 300 / 2156 |

- `MADV_ZERO` (Darwin, `libc::MADV_ZERO = 11`, available on this kernel) zero-fills the
  resident pages in place and leaves them resident, at `memset` speed, plus a range walk of
  15 to 18 µs per 64 MiB. Per dirty run it is 0.3 µs a call. Because the pages stay resident the
  next lease touches them for free (0.03 µs for 4 pages). The pool probes it once and falls
  back to remapping on `EINVAL`/`ENOTSUP`.
- `mmap(MAP_FIXED)` inside the slot costs about 30 µs for 64 MiB regardless of residency and
  makes the next lease fault every page again at 0.6 to 2 µs each. It is the portable fallback,
  not the primary primitive. `MAP_FIXED` is only ever issued for `[base, base + reach)` of a
  slot this module mapped, which is the ticket's safety invariant.
- Following `MADV_ZERO` with `madvise(MADV_FREE_REUSABLE)`, so that the kernel could take the
  zeroed pages back, was measured and rejected: the kernel takes them back at once and every
  reuse faults again (0.75 µs per page), the 64 MiB range reset went from 15 to 48 µs, a sparse
  touch afterwards from 0.03 to 3 µs, the typical call (D) from 1.6 to 12.8 µs with the range
  policy, and the resident size reported by `proc_pidinfo` did not move. The numbers come from
  a harness run with that variant; the code path is gone.
- Default policy as on Linux: `memset` up to 8 MiB of dirty pages (512 pages here), the range
  `MADV_ZERO` above. Resident memory of a pooled slot is then bounded by the threshold under
  `memset` and by the dirty set under `MADV_ZERO`; a host that needs a hard bound after a dense
  call can lower `max_free_slots` or use the remap fallback for that tier.

The `Vec` baseline behaves differently on macOS: `malloc` recycles a freed 64 MiB block, so the
`Vec` path pays no page faults on the next call, only the `memset` (270 to 330 µs per 64 MiB), and
`calloc` does not help (about the same: the recycled block is zeroed by hand).

## 4. Host page size

`host_page_size()` reads `sysconf(_SC_PAGESIZE)` once and requires a power of two. A slot stores
`page_shift = log2(page size)`; `mark_dirty(offset, len)` marks `offset >> shift` through
`(offset + len - 1) >> shift`, so a one-byte store marks one host page (4 KiB on Linux, 16 KiB on
Apple Silicon), a scalar store that straddles a boundary marks two, and a `memory.fill` marks the
pages it covers. Nothing is tracked at 64 KiB granularity. The reset rounds the high-water mark
up to a host page. The unit tests assert the run arithmetic across bitmap words and the
integration tests compute their expectations from `host_page_size()`, so they pass on both page
sizes; the harness prints the size it ran with.

## 5. Dirty-page tracking coverage

Tracking is not something a write path can forget. `GlobalMemory` no longer hands out its buffer
for writing: the `shared_memory` field and `data_mut()` are gone, and the only way to write is a
window the memory returns for an exact range, after bounds-checking it and marking its host
pages. A caller cannot reach a byte outside its window, and a window cannot exist unmarked.

| path | where | window |
|---|---|---|
| `i32.store`, `i32.store8`, `i32.store16` (and `i64.store*`, which the compiler lowers to pairs of `i32` stores) | `RwasmExecutor::execute_store_wrap`, `src/vm/executor.rs` | `store_window(address, offset, len)` |
| `f32.store`, `f64.store` | `src/vm/executor/fpu.rs` | `store_window`, 4 or 8 bytes |
| `memory.fill` | `src/vm/executor/memory.rs` | `tracked_mut(d, n)` |
| `memory.copy` | same | `copy_within(src, dst, n)`, which marks `[dst, dst + n)` |
| `memory.init` | same | `tracked_mut(dst, n)` |
| host writes: `StoreTr::memory_write` on `RwasmStore` and `RwasmCaller`, syscall handlers, `TypedCaller` | `GlobalMemory::write`, `src/vm/memory.rs` | `tracked_mut(offset, len)` |
| `memory.grow` | `GlobalMemory::grow` | none: the new pages are zero; the high-water mark moves |

Reads (`data()`, `memory_read`, the tracer, `memory_snapshot`) do not mark. Table and global
operations do not touch memory. A window that ends up unwritten (the write fails afterwards, for
example `memory.init` from a dropped segment) is marked all the same, which costs a reset of
clean pages and nothing else. The bounds checks and trap codes are the ones the old paths had;
the whole test suite, the spec suite (`e2e`, 93 files) and the fuzzers run on the new paths in
every build, since the window API is not feature-gated.

Both backings are reached through one base pointer and length, so loads and stores do not
branch on where the memory lives; only the mark itself checks for a lease. The unit tests of
the `Vec` path pass under Miri with Stacked Borrows, and the executor-level memory tests
(`tests/memory.rs`: stores, `memory.fill`, `memory.copy`, `memory.init`) pass under Tree Borrows.
Under Stacked Borrows those executor tests stop at a violation in `ValueStackPtr`
(`src/vm/value_stack.rs`), which is present on `devel` as well and has nothing to do with the
memory.

Cost (harness section 3: three instances measured in 25 interleaved rounds, best round each, ns
per loop iteration, loop = one store and 6 other opcodes):

| loop | Linux `Vec` | Linux pooled, tracking off | Linux pooled, tracking on | macOS `Vec` | macOS pooled, tracking off | macOS pooled, tracking on |
|---|---:|---:|---:|---:|---:|---:|
| sequential `i32.store` in 64 KiB | 35.0 | 33.2 | 35.9 (+2.5%) | 25.0 | 25.1 | 25.1 (+0.7%) |
| `i32.store` on a new page each time | 37.4 | 33.0 | 38.1 (+1.7%) | 25.1 | 25.3 | 25.4 (+1.5%) |
| integer loop, no memory access | 29.4 | 28.1 | 28.5 (-3.3%) | 23.4 | 24.1 | 23.4 (+0.1%) |

The differences are inside the noise of the harness (a few percent between runs, in both
directions, including on the loop that does not touch memory). An earlier version of the pooled
access path went through an `Option` check per access and cost 13% to 15% on Linux, and a second
one branched on the backing and cost 5% to 10% on macOS; the single base pointer removed both,
and made the `Vec` path itself a little faster than it was.

Why keep explicit tracking at all, given that the range primitives do not need it: the bitmap is
what allows the `memset` policy (zero 18 pages instead of discarding them and faulting them back
in), it makes the `Adaptive` decision, and `dirty_host_pages()` gives the host a working-set
figure for free. Signal- or `mprotect`-based tracking was not tried, as the ticket asked.

## 6. Initial-data restoration

None is needed. A slot is returned to all zeros, not to the post-instantiation image, and the next
instance's prologue copies the data segments again through `memory.init`. The image is therefore
never stored anywhere and cannot drift from what the compiler emits; `tests/memory_pool.rs`
checks that the recycled and the fresh instance have byte-identical memory right after
instantiation.

The price is the `memcpy` of the data segments per instantiation, which every backend pays today
as well. For 35 of the 37 surveyed Fluentbase modules that is at most 53 KiB, under 2 µs. The
two outliers (1 MiB and 750 KiB of tables) cost about 5 µs on either platform (harness row E:
`memset` of 1 MiB).

The ticket's advanced form, a `memfd`/file image mapped `MAP_PRIVATE` so that instantiation is a
mapping and data pages come in by copy-on-write faults, was evaluated on the numbers and
rejected: a CoW fault costs 330 ns per 4 KiB on Linux and 0.6 to 2 µs per 16 KiB on macOS,
against 40 ns to `memcpy` 4 KiB. Even the 1 MiB outlier is cheaper to copy (4 µs) than to fault
in (256 faults, 85 µs), and an image would also have to be reset with `MADV_DONTNEED` (which
re-faults the image pages) or tracked separately from the zero pages. Wasmtime's `memory_init_cow`
exists for modules whose heap images are tens of megabytes; rwasm's are kilobytes. The
"simple implementation" of the ticket (copy the page-local initial bytes back on reset) is
subsumed: the prologue does it for the pages that need it, and it is not on the reset path.

## 7. Instance-state reset requirements

For a recycled slot to be indistinguishable from a fresh instance, every item of the ticket's
list is covered by construction:

| state | where it lives | on the next call |
|---|---|---|
| linear memory contents | the slot | reset to zeros by policy; prologue re-initializes data |
| logical memory size after `memory.grow` | `GlobalMemory::current_pages`, `len` | new `GlobalMemory` starts at 0 pages; prologue grows |
| mutable globals | `RwasmStore::global_variables` | new store; prologue sets initializers |
| tables | `RwasmStore::tables` | new store; prologue grows and initializes |
| dropped data / element segments | `RwasmStore::empty_*_segments` | new store (empty bitsets); prologue drops active segments |
| value stack, call stack, stack pointers | `ExecutionEngine::execute` allocates per call | fresh per call, as today |
| fuel | `RwasmStore::consumed_fuel`, `fuel_limit` | new store |
| trap state, `last_signature`, parked execution | `RwasmStore` | new store |
| host-side slot metadata (bitmap, high-water mark) | `MemorySlot` | cleared by `reset` |

Two things in the slot survive a lease on purpose and are invisible to the guest: the mapping
itself and, under the `memset` tier, the residency of the zeroed pages.

## 8. Abnormal halt and trap lifecycle

```
lease ──▶ instantiate ──▶ execute ──┬─ Ok / result
                                    ├─ trap (Unreachable, MemoryOutOfBounds, ...)
                                    ├─ OutOfFuel
                                    ├─ InterruptionCalled ──▶ resume ... (slot stays leased)
                                    └─ interruption abandoned (store dropped while parked)
                                              │
                          drop(store) / replace instance ──▶ MemoryLease::drop
                                              │
                                   MemorySlot::reset(policy, verify)
                                       ├─ Ok, free list not full ──▶ pooled (all zeros)
                                       ├─ Ok, free list full ──▶ munmap
                                       └─ Err (syscall failed / verification failed) ──▶ munmap
```

The reset reads only host-owned state: the bitmap and the high-water mark the VM maintained on
every write and growth, whatever the guest did afterwards. A trap in the middle of a store has
already marked the page. `OutOfFuel` inside `memory.fill` is charged before the fill runs, so
the fill either happened and is marked or did not happen. An interrupted execution parks its
stacks in the store and keeps the lease; cancelling it (`RwasmStore::reset`) or dropping the
store releases the slot like any other exit. A replacement instantiation that traps rolls back
to the previous instance and its slot (`rollback_instantiation`), and the slot of the failed
attempt is released. Nothing in this path consults guest memory, the guest stack pointer or the
guest allocator.

The `Prior` cases of `tests/memory_pool.rs` exercise each arrow, including "grow to 64 pages,
run out of fuel while writing every page, then trap", after which the slot is leased again and
compared against a fresh instance across 22 probe calls, one of which traps again.

If a reset cannot vouch for the slot it is destroyed: a failed `madvise`/`mmap`, or a non-zero
byte found by `verify_reset`. The pool counts these (`reset_failures`, `slots_unmapped`); a host
can watch the counter. `verify_reset` reads the reachable range back after every reset; it is on
by default in debug builds and in the fuzz target, off in release builds.

## 9. Benchmarks

Platforms: Linux 6.12 arm64 in a Docker Desktop VM (16 vCPUs, 8 GiB, 4 KiB pages, 2 MiB huge
pages, THP `always`) and macOS 26.5 on the Apple M5 Max host of that VM (16 KiB pages); Linux
x86-64 was not available. Run-to-run variance is a few percent; the tables show one run each.
Release build, `cargo bench --bench memory_pool --features memory-pool`. The full tables are in
appendix A; the harness prints them.

**Fresh allocation versus reset (harness section 1, 64 MiB slot).** Today's path is the `Vec
resize` row: `try_reserve_exact` plus `resize(len, 0)`. The `Vec calloc` row is what
`alloc_zeroed` would do instead; on Linux it turns the 64 MiB case from 880 µs into 5 µs of
allocation plus huge-page faults on touch (19 µs for 4 touches), on macOS it changes nothing
(the allocator zeroes the recycled block itself). It is a one-line quick win for the first
`memory.grow` on Linux that needs no pool; noted, not implemented.

**The VM path (harness section 2).** New store, instantiation, one call, drop, µs per call, pool
in its default configuration:

| call | Linux `Vec` | Linux pool | Linux pool, small pages only | macOS `Vec` | macOS pool |
|---|---:|---:|---:|---:|---:|
| F: instantiate a 17-page contract, no call | 4.55 | 0.39 | 0.52 | 4.52 | 0.54 |
| D: typical call, 64 KiB of stack + 8 KiB of data | 5.54 | 1.83 | 1.98 | 5.21 | 1.59 |
| E: a call that writes its whole 1 MiB stack | 18.1 | 18.6 | 19.3 | 12.2 | 12.4 |
| A: grow to 64 MiB, write 4 pages | 886 | 22.0 | 0.78 | 317 | 1.15 |
| B: grow to 64 MiB, write 256 pages 256 KiB apart | 750 | 389 | 24.5 | 331 | 50.6 |
| C: grow to 64 MiB, fill all of it | 1694 | 1340 | 7825 | 801 | 1007 |

Reading the rows: F and D are the cases every call goes through and the pool removes the `memset`
of the declared memory; E writes as much as it declares, so both zero 1 MiB; A and B are the
ticket's sparse and moderate workloads; C is the dense one, where zeroing and faulting 64 MiB
dominates whatever the backend.

**Where resetting stops paying.** In the small-page part `memset` costs 18 ns per 4 KiB page
and keeps the page; `MADV_DONTNEED` costs 46 ns per resident page and the next lease pays a
330 ns fault to get it back. Keeping pages is therefore cheaper at any size, and the 8 MiB
threshold is set by how much memory a pooled slot may hold, not by speed. In the huge-page part
a touched page costs 9 to 13 µs to fault and is always given back. Destroying and remapping a
slot (`munmap` + `mmap`, the "mmap+munmap per instance" rows) is never cheaper than
`MADV_DONTNEED` over it (782 against 756 µs dense, 3.67 against 1.71 µs sparse), so the adaptive
"recreate above a threshold" policy of the ticket is not needed.

## 10. Memory, RSS and page-fault measurements

- **Virtual address space.** One slot reserves `slot_pages × 64 KiB` (64 MiB at the Fluentbase
  cap) with `MAP_NORESERVE`. A process with `max_free_slots = 8` and `n` live frames reserves
  `(8 + n) × 64 MiB`; at Fluentbase's transaction-wide bound of 1.5 GiB of in-flight logical
  memory that is at most 24 live slots, 2 GiB of address space, which is nothing on a 48-bit
  host.
- **Resident memory.** A pooled slot keeps at most the `memset` threshold resident, 8 MiB, and
  only if an instance dirtied that much; the huge-page part is always released. The `RSS after`
  columns of appendix A show it: on Linux the process stays at 2.3 to 4 MiB through the sparse
  and contract workloads and ends at 10 MiB after the dense one (the 8 MiB prefix), with no
  growth across 400 leases per row. `max_free_slots = 8` therefore bounds idle RSS at 64 MiB. On
  macOS the `Vec` rows sit at 66 MiB because `malloc` keeps the freed 64 MiB block; the pool rows
  add one 64 MiB slot per pool (four pools in section 2, hence 325 MiB at the end), resident
  after the dense row because `MADV_ZERO` zeroes in place. That is the one place where macOS
  needs the remap fallback or a small free list to give memory back; `MADV_FREE_REUSABLE` is
  not the answer (section 3).
- **Page faults.** `faults/iter` and `faults/call` are `getrusage` minor faults. The pool in its
  default configuration faults 0 times per typical call (rows D, E, F); today's `Vec` path faults
  0 for small memories (the allocator recycles the block) and 544 per 64 MiB memory on Linux (the
  block is mapped fresh every time and mostly populated with huge pages). A dense fill of a
  split slot takes 129 faults; on small pages it took 16384.
- **Reservation versus commit.** Nothing is committed at lease time; a leased slot that the
  prologue grows to 17 pages commits the data segment's page(s) only (one host page for every
  surveyed contract but two), which `memory_dirty_host_pages()` reports as 1.

## 11. Integration design

### 11.1 rwasm

The surface: `MemoryPool`, `MemoryPoolConfig`, `ResetPolicy`,
`StrategyDefinition::create_executor_with_memory_pool`, `RwasmStore::with_memory_pool`,
`RwasmStore::memory_dirty_host_pages`, `MemoryPool::stats`. `create_executor` is unchanged, so
existing callers are not touched; the Wasmtime strategy ignores the pool, its memories are
Wasmtime's.

The feature stays optional and Unix-only. The `no_std` build (the zkVM guest, where there is no
`mmap` and instantiation happens once per proof) keeps the `Vec` backing, and marking compiles
to nothing there.

### 11.2 Fluentbase

- One `MemoryPool` per process (or per `RuntimeExecutor`), created with `slot_pages =
  N_DEFAULT_MAX_MEMORY_PAGES`, the per-frame cap the store is created with today
  (`ContractRuntime::new` passes `Some(N_DEFAULT_MAX_MEMORY_PAGES)`), so a slot always holds what
  the store may grow to.
- `ContractRuntime::new` calls `strategy.create_executor_with_memory_pool(.., &pool)` instead of
  `create_executor`. That is the whole change: leases follow the store's
  lifetime, nested frames are separate stores, a suspended frame keeps its slot while parked,
  and `frame_memory_size_bytes()` keeps reporting the logical size, so the transaction-wide
  in-flight bound (`MAX_IN_FLIGHT_MEMORY_BYTES`, FLU-1047/FLU-935) is unchanged and remains an
  upper bound on resident memory.
- Fluentbase never sees dirty pages, policies or platform calls. It may read
  `pool.stats()` for metrics (`slots_reserved`, `reset_failures`).
- Threads: the pool is a `Mutex<Vec<MemorySlot>>` behind an `Arc`; lease and release are two
  short critical sections per call. Per-thread pools (as `COMPILED_RUNTIMES` is thread-local)
  would remove even that if it ever shows up.
- Pool sizing: `max_free_slots` bounds idle address space and idle RSS (≤ 4 MiB each under the
  default policy); slots in flight are bounded by the frame limit that already exists.

### 11.3 What must not change

Fuel: the prologue runs as before and charges what it charged. Bytecode: untouched. Observable
semantics: the equivalence tests are the contract. They run in CI (the suite runs a second time
with the feature on), and the `resume_equivalence` fuzz target now runs every generated module a
third time on memory leased from process-wide pools, one per reset policy, whose slots are handed
from one fuzz iteration to the next. The pooled run must observe what the `Vec` run observes
(outcome, fuel, memory after every call), and every reset is read back; a stray byte or a
non-zero slot aborts the fuzzer.

### 11.4 The Wasmtime side

System runtimes run on Wasmtime and are reused across calls without any reset
(`SystemRuntime::execute` swaps the context and calls again; an abnormal exit evicts the cached
instance). The rwasm engine configures Wasmtime with `memory_init_cow(false)` and the default
on-demand allocator (`src/wasmtime/engine.rs`). Wasmtime's pooling allocator with CoW images is
the mechanism the ticket cites; enabling it and re-instantiating per call would give system
runtimes the same "fresh instance per call" guarantee at a few microseconds. That is a separate
change with its own compatibility-hash consequences for compiled artifacts and is out of scope
here, as the ticket asked for a native rwasm design.

## 12. Recommendation

Implement, with these modifications to the ticket's design:

1. Pool the linear memory slot and nothing else; keep one store per call and re-run the prologue.
   No instance template, no `reset_to_initial_instance_state()`, no changes to
   `RwasmStore::reset`.
2. Reset the small-page part by dirty bitmap with `memset` up to a resident-memory threshold
   (8 MiB), and by one kernel call over the reachable range above it: `madvise(MADV_DONTNEED)`
   on Linux, `madvise(MADV_ZERO)` on macOS, `mmap(MAP_FIXED)` inside the slot only as the
   fallback. Do not issue one kernel call per dirty run, do not remap as the primary primitive,
   and do not follow `MADV_ZERO` with `MADV_FREE_REUSABLE`.
3. Keep explicit dirty tracking, enforced by the memory's own write API rather than by its
   callers.
4. Drop the copy-on-write initial image: rwasm data segments are kilobytes and a CoW fault costs
   more than the copy.
5. Split slots on Linux: small pages for the first 8 MiB, huge-page eligible above, released on
   every reset.

Independent quick win, with or without the pool: allocate the first `memory.grow` with
`alloc_zeroed` instead of `resize`; on Linux that alone takes a 64 MiB instantiation from
880 µs to about 25 µs.

## 13. Status after the hardening pass

The first version of this report called the code a prototype and listed what kept it from being
turned on. What was done about each item:

| item | state |
|---|---|
| A write path that forgets to mark its pages leaks one instance into the next | Closed by construction: `GlobalMemory::data_mut` and the public buffer are gone, every write goes through a window the memory bounds-checks and marks (section 5). |
| Coverage was eight hand-written scenarios | The `resume_equivalence` fuzz target runs every module on pooled memory recycled across iterations, five reset policies, verified resets; the 6281-entry corpus and ten more minutes of fuzzing (11380 runs) pass. CI runs the whole suite a second time with the feature on. Debug builds verify every reset. |
| Not reachable from the strategy layer | `StrategyDefinition::create_executor_with_memory_pool`; `create_executor` is unchanged. |
| Dense memory use 4x slower on Linux | Split slots; the dense fill is now faster than today and no measured workload is slower on Linux. |
| 5% to 10% slower store loops on macOS | One base pointer for both backings; within noise now. |
| New `unsafe` code not audited | Narrowed and documented; the `Vec` path passes Miri (section 5). The mmap code cannot run under Miri and still wants a reviewer. |
| The `e2e` suite was not run | Run: 93 of 93 spec files pass on the new write paths. |
| No Linux x86-64 numbers | Open. `madvise` on x86-64 flushes TLBs with cross-CPU interrupts where arm64 broadcasts in hardware, so the kernel-call costs and the multi-threaded behaviour have to be re-measured there before the defaults are final. |
| Fluentbase does not use it yet | Open, in the Fluentbase repository: one pool in the runtime executor and `create_executor_with_memory_pool` in `ContractRuntime::new`. |

## Appendix A. Raw harness output

### A.1 Linux arm64 (Docker Desktop VM, kernel 6.12, 4 KiB pages, THP `always`)

# memory pool measurements

os=linux arch=aarch64 host_page=4096 huge_page=2097152 thp=[always] madvise never quick=false

## 1. Slot reset primitives (64 MiB slot, host page 4 KiB)

| workload | backend / policy | dirty pages | touch µs | alloc+reset µs | faults/iter | reset method | calls | RSS after MiB |
|---|---|---:|---:|---:|---:|---|---:|---:|
| A sparse: 4 pages of 64 MiB | Vec resize (today) | - | 0.16 | 876.41 | 544.0 | alloc+drop | - | 2.3 |
| A sparse: 4 pages of 64 MiB | Vec calloc | - | 19.44 | 5.28 | 4.0 | alloc+drop | - | 2.3 |
| A sparse: 4 pages of 64 MiB | mmap+munmap per instance | - | 2.59 | 3.67 | 4.0 | mmap/munmap | - | 2.3 |
| A sparse: 4 pages of 64 MiB | pool Discard | 4 | 1.17 | 1.71 | 4.0 | madvise(MADV_DONTNEED) | 1 | 2.3 |
| A sparse: 4 pages of 64 MiB | pool Remap | 4 | 2.64 | 2.87 | 4.0 | mmap(MAP_FIXED) | 1 | 2.3 |
| A sparse: 4 pages of 64 MiB | pool DiscardDirty | 4 | 1.15 | 2.25 | 4.0 | madvise(MADV_DONTNEED) | 4 | 2.3 |
| A sparse: 4 pages of 64 MiB | pool RemapDirty | 4 | 1.12 | 5.90 | 4.0 | mmap(MAP_FIXED) | 4 | 2.3 |
| A sparse: 4 pages of 64 MiB | pool Memset | 4 | 0.02 | 0.40 | 0.0 | memset | 4 | 2.3 |
| A sparse: 4 pages of 64 MiB | pool Adaptive(2048) | 4 | 0.03 | 0.43 | 0.0 | memset | 4 | 2.3 |
| A sparse: 4 pages of 64 MiB | pool default (split slot) | 4 | 18.70 | 3.52 | 2.0 | memset | 3 | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec resize (today) | - | 37.70 | 949.31 | 544.0 | alloc+drop | - | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec calloc | - | 458.55 | 56.38 | 47.0 | alloc+drop | - | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | mmap+munmap per instance | - | 109.73 | 27.58 | 256.0 | mmap/munmap | - | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Discard | 256 | 99.14 | 24.85 | 256.0 | madvise(MADV_DONTNEED) | 1 | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Remap | 256 | 96.41 | 23.32 | 256.0 | mmap(MAP_FIXED) | 1 | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool DiscardDirty | 256 | 80.93 | 127.00 | 256.0 | madvise(MADV_DONTNEED) | 256 | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool RemapDirty | 256 | 87.72 | 354.90 | 256.0 | mmap(MAP_FIXED) | 256 | 2.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Memset | 256 | 16.83 | 15.08 | 0.5 | memset | 256 | 3.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Adaptive(2048) | 256 | 15.67 | 14.92 | 0.5 | memset | 256 | 3.3 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool default (split slot) | 256 | 341.99 | 40.80 | 28.1 | memset | 33 | 2.4 |
| C dense: every page of 64 MiB | Vec resize (today) | - | 93.53 | 881.79 | 544.0 | alloc+drop | - | 2.3 |
| C dense: every page of 64 MiB | Vec calloc | - | 604.14 | 62.17 | 543.0 | alloc+drop | - | 2.3 |
| C dense: every page of 64 MiB | mmap+munmap per instance | - | 5746.60 | 782.47 | 16384.0 | mmap/munmap | - | 2.3 |
| C dense: every page of 64 MiB | pool Discard | 16384 | 5767.55 | 756.20 | 16384.0 | madvise(MADV_DONTNEED) | 1 | 2.3 |
| C dense: every page of 64 MiB | pool Remap | 16384 | 5863.00 | 794.53 | 16384.0 | mmap(MAP_FIXED) | 1 | 2.3 |
| C dense: every page of 64 MiB | pool DiscardDirty | 16384 | 5713.48 | 746.34 | 16384.0 | madvise(MADV_DONTNEED) | 1 | 2.3 |
| C dense: every page of 64 MiB | pool RemapDirty | 16384 | 5932.61 | 807.93 | 16384.0 | mmap(MAP_FIXED) | 1 | 2.3 |
| C dense: every page of 64 MiB | pool Memset | 16384 | 222.09 | 300.83 | 327.7 | memset | 1 | 66.3 |
| C dense: every page of 64 MiB | pool Adaptive(2048) | 16384 | 5817.71 | 773.30 | 16384.0 | madvise(MADV_DONTNEED) | 1 | 2.3 |
| C dense: every page of 64 MiB | pool default (split slot) | 16384 | 440.92 | 76.02 | 69.0 | memset | 2 | 10.3 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec resize (today) | - | 0.54 | 5.19 | 1.1 | alloc+drop | - | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec calloc | - | 0.53 | 4.62 | 0.0 | alloc+drop | - | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | mmap+munmap per instance | - | 5.93 | 2.17 | 18.0 | mmap/munmap | - | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Discard | 18 | 5.94 | 1.53 | 18.0 | madvise(MADV_DONTNEED) | 1 | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Remap | 18 | 5.95 | 2.34 | 18.0 | mmap(MAP_FIXED) | 1 | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | pool DiscardDirty | 18 | 6.10 | 1.65 | 18.0 | madvise(MADV_DONTNEED) | 1 | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | pool RemapDirty | 18 | 5.95 | 2.64 | 18.0 | mmap(MAP_FIXED) | 1 | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Memset | 18 | 0.56 | 0.99 | 0.0 | memset | 1 | 3.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Adaptive(2048) | 18 | 0.56 | 1.01 | 0.0 | memset | 1 | 3.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool default (split slot) | 18 | 0.60 | 0.93 | 0.0 | memset | 1 | 3.2 |
| E contract: whole 1 MiB stack | Vec resize (today) | - | 12.57 | 5.31 | 0.0 | alloc+drop | - | 3.1 |
| E contract: whole 1 MiB stack | Vec calloc | - | 12.61 | 5.27 | 0.0 | alloc+drop | - | 3.1 |
| E contract: whole 1 MiB stack | mmap+munmap per instance | - | 79.80 | 11.18 | 258.0 | mmap/munmap | - | 3.1 |
| E contract: whole 1 MiB stack | pool Discard | 258 | 80.11 | 9.69 | 258.0 | madvise(MADV_DONTNEED) | 1 | 3.1 |
| E contract: whole 1 MiB stack | pool Remap | 258 | 87.21 | 12.00 | 258.0 | mmap(MAP_FIXED) | 1 | 3.1 |
| E contract: whole 1 MiB stack | pool DiscardDirty | 258 | 81.41 | 9.51 | 258.0 | madvise(MADV_DONTNEED) | 1 | 3.1 |
| E contract: whole 1 MiB stack | pool RemapDirty | 258 | 79.76 | 10.06 | 258.0 | mmap(MAP_FIXED) | 1 | 3.1 |
| E contract: whole 1 MiB stack | pool Memset | 258 | 13.29 | 5.41 | 0.5 | memset | 1 | 4.1 |
| E contract: whole 1 MiB stack | pool Adaptive(2048) | 258 | 13.21 | 5.37 | 0.5 | memset | 1 | 4.1 |
| E contract: whole 1 MiB stack | pool default (split slot) | 258 | 13.78 | 5.45 | 0.5 | memset | 1 | 4.1 |

## 2. VM path: new store + instantiate + one call + drop

| call | memory | µs/call | of which execute µs | faults/call | dirty pages | RSS after MiB |
|---|---|---:|---:|---:|---:|---:|
| A grow + 4 sparse writes | Vec (today) | 885.98 | 818.60 | 544.0 | - | 4.3 |
| A grow + 4 sparse writes | pool default | 22.00 | 18.40 | 2.0 | 4 | 4.3 |
| A grow + 4 sparse writes | pool, small pages only | 0.78 | 0.13 | 0.0 | 4 | 4.3 |
| A grow + 4 sparse writes | pool, Discard only | 23.91 | 19.14 | 4.0 | 4 | 4.3 |
| A grow + 4 sparse writes | pool, Memset only | 22.75 | 18.89 | 2.0 | 4 | 4.3 |
| B grow + 256 strided writes | Vec (today) | 750.42 | 697.31 | 33.0 | - | 4.4 |
| B grow + 256 strided writes | pool default | 389.42 | 346.01 | 28.1 | 256 | 4.5 |
| B grow + 256 strided writes | pool, small pages only | 24.50 | 9.48 | 0.6 | 256 | 5.5 |
| B grow + 256 strided writes | pool, Discard only | 390.82 | 346.50 | 60.0 | 256 | 5.5 |
| B grow + 256 strided writes | pool, Memset only | 372.00 | 330.78 | 28.1 | 256 | 5.6 |
| C grow + fill 64 MiB | Vec (today) | 1694.35 | 1638.97 | 33.0 | - | 5.6 |
| C grow + fill 64 MiB | pool default | 1339.88 | 1256.88 | 128.8 | 16384 | 13.4 |
| C grow + fill 64 MiB | pool, small pages only | 7824.74 | 7008.28 | 16371.2 | 16384 | 12.4 |
| C grow + fill 64 MiB | pool, Discard only | 2071.66 | 1953.76 | 2076.0 | 16384 | 12.4 |
| C grow + fill 64 MiB | pool, Memset only | 1320.29 | 1238.10 | 128.8 | 16384 | 20.3 |
| D contract: 64 KiB stack + 8 KiB data | Vec (today) | 5.54 | 0.66 | 0.0 | - | 20.3 |
| D contract: 64 KiB stack + 8 KiB data | pool default | 1.83 | 0.68 | 0.0 | 18 | 20.3 |
| D contract: 64 KiB stack + 8 KiB data | pool, small pages only | 1.98 | 0.69 | 0.0 | 18 | 20.4 |
| D contract: 64 KiB stack + 8 KiB data | pool, Discard only | 7.86 | 5.81 | 18.0 | 18 | 20.4 |
| D contract: 64 KiB stack + 8 KiB data | pool, Memset only | 1.86 | 0.67 | 0.0 | 18 | 20.4 |
| E contract: whole 1 MiB stack | Vec (today) | 18.14 | 12.63 | 0.0 | - | 20.4 |
| E contract: whole 1 MiB stack | pool default | 18.55 | 13.20 | 0.0 | 258 | 20.4 |
| E contract: whole 1 MiB stack | pool, small pages only | 19.32 | 13.63 | 0.6 | 258 | 21.3 |
| E contract: whole 1 MiB stack | pool, Discard only | 88.98 | 79.31 | 258.0 | 258 | 21.3 |
| E contract: whole 1 MiB stack | pool, Memset only | 18.66 | 13.24 | 0.0 | 258 | 21.3 |
| F instantiate only (no call) | Vec (today) | 4.55 | 0.01 | 0.0 | - | 21.3 |
| F instantiate only (no call) | pool default | 0.39 | 0.02 | 0.0 | 1 | 21.3 |
| F instantiate only (no call) | pool, small pages only | 0.52 | 0.01 | 0.0 | 1 | 21.3 |
| F instantiate only (no call) | pool, Discard only | 1.41 | 0.01 | 1.0 | 1 | 21.3 |
| F instantiate only (no call) | pool, Memset only | 0.41 | 0.01 | 0.0 | 1 | 21.3 |

## 3. Dirty tracking overhead (ns per loop iteration)

| loop | Vec | pool, tracking off | pool, tracking on | tracking on vs Vec |
|---|---:|---:|---:|---:|
| sequential i32.store within 64 KiB | 34.97 | 33.16 | 35.85 | +2.5% |
| i32.store on a new page each time | 37.41 | 33.04 | 38.06 | +1.7% |
| integer loop, no memory access | 29.44 | 28.13 | 28.48 | -3.3% |

### A.2 macOS 26.5, Apple M5 Max (16 KiB pages)

# memory pool measurements

os=macos arch=aarch64 host_page=16384 huge_page=0 thp=n/a quick=false

## 1. Slot reset primitives (64 MiB slot, host page 16 KiB)

| workload | backend / policy | dirty pages | touch µs | alloc+reset µs | faults/iter | reset method | calls | RSS after MiB |
|---|---|---:|---:|---:|---:|---|---:|---:|
| A sparse: 4 pages of 64 MiB | Vec resize (today) | - | 0.02 | 310.23 | 8.2 | alloc+drop | - | 65.9 |
| A sparse: 4 pages of 64 MiB | Vec calloc | - | 0.01 | 251.37 | 0.0 | alloc+drop | - | 65.9 |
| A sparse: 4 pages of 64 MiB | mmap+munmap per instance | - | 5.87 | 28.73 | 4.0 | mmap/munmap | - | 65.9 |
| A sparse: 4 pages of 64 MiB | pool Discard | 4 | 0.03 | 17.86 | 0.0 | madvise(MADV_ZERO) | 1 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool Remap | 4 | 5.04 | 27.83 | 4.0 | mmap(MAP_FIXED) | 1 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool DiscardDirty | 4 | 0.04 | 1.27 | 0.0 | madvise(MADV_ZERO) | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool RemapDirty | 4 | 5.49 | 7.14 | 4.0 | mmap(MAP_FIXED) | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool Memset | 4 | 0.04 | 0.78 | 0.0 | memset | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool Adaptive(512) | 4 | 0.03 | 0.76 | 0.0 | memset | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool default (split slot) | 4 | 0.03 | 0.76 | 0.0 | memset | 4 | 130.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec resize (today) | - | 13.33 | 253.40 | 0.0 | alloc+drop | - | 66.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec calloc | - | 14.12 | 270.41 | 0.0 | alloc+drop | - | 66.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | mmap+munmap per instance | - | 44.34 | 25.65 | 64.0 | mmap/munmap | - | 66.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Discard | 64 | 10.14 | 24.14 | 0.1 | madvise(MADV_ZERO) | 1 | 130.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Remap | 64 | 47.70 | 25.21 | 64.0 | mmap(MAP_FIXED) | 1 | 130.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool DiscardDirty | 64 | 10.17 | 17.79 | 0.1 | madvise(MADV_ZERO) | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool RemapDirty | 64 | 48.96 | 53.76 | 64.0 | mmap(MAP_FIXED) | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Memset | 64 | 9.74 | 10.29 | 0.1 | memset | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Adaptive(512) | 64 | 10.21 | 10.80 | 0.1 | memset | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool default (split slot) | 64 | 9.96 | 10.55 | 0.1 | memset | 64 | 130.1 |
| C dense: every page of 64 MiB | Vec resize (today) | - | 19.17 | 251.22 | 0.0 | alloc+drop | - | 66.1 |
| C dense: every page of 64 MiB | Vec calloc | - | 16.71 | 253.98 | 0.0 | alloc+drop | - | 66.1 |
| C dense: every page of 64 MiB | mmap+munmap per instance | - | 2155.67 | 299.86 | 4096.0 | mmap/munmap | - | 66.1 |
| C dense: every page of 64 MiB | pool Discard | 4096 | 63.97 | 292.40 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool Remap | 4096 | 2151.59 | 284.05 | 4096.0 | mmap(MAP_FIXED) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool DiscardDirty | 4096 | 68.41 | 309.35 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool RemapDirty | 4096 | 2355.03 | 358.00 | 4096.0 | mmap(MAP_FIXED) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool Memset | 4096 | 77.65 | 256.92 | 81.9 | memset | 1 | 130.1 |
| C dense: every page of 64 MiB | pool Adaptive(512) | 4096 | 69.44 | 327.18 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool default (split slot) | 4096 | 72.29 | 324.45 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec resize (today) | - | 0.62 | 4.59 | 0.1 | alloc+drop | - | 67.1 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec calloc | - | 0.56 | 5.28 | 0.0 | alloc+drop | - | 67.1 |
| D contract: 64 KiB of stack + 8 KiB of data | mmap+munmap per instance | - | 3.21 | 1.86 | 5.0 | mmap/munmap | - | 67.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Discard | 5 | 0.57 | 0.86 | 0.0 | madvise(MADV_ZERO) | 1 | 68.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Remap | 5 | 4.45 | 3.05 | 5.0 | mmap(MAP_FIXED) | 1 | 68.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool DiscardDirty | 5 | 0.57 | 0.73 | 0.0 | madvise(MADV_ZERO) | 1 | 68.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool RemapDirty | 5 | 4.33 | 2.53 | 5.0 | mmap(MAP_FIXED) | 1 | 68.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Memset | 5 | 0.56 | 0.71 | 0.0 | memset | 1 | 68.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Adaptive(512) | 5 | 0.58 | 0.73 | 0.0 | memset | 1 | 68.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool default (split slot) | 5 | 0.58 | 0.72 | 0.0 | memset | 1 | 68.2 |
| E contract: whole 1 MiB stack | Vec resize (today) | - | 7.64 | 4.30 | 0.0 | alloc+drop | - | 67.2 |
| E contract: whole 1 MiB stack | Vec calloc | - | 7.83 | 5.20 | 0.0 | alloc+drop | - | 67.2 |
| E contract: whole 1 MiB stack | mmap+munmap per instance | - | 41.48 | 4.91 | 65.0 | mmap/munmap | - | 67.2 |
| E contract: whole 1 MiB stack | pool Discard | 65 | 7.99 | 4.78 | 0.1 | madvise(MADV_ZERO) | 1 | 68.2 |
| E contract: whole 1 MiB stack | pool Remap | 65 | 41.93 | 6.12 | 65.0 | mmap(MAP_FIXED) | 1 | 68.2 |
| E contract: whole 1 MiB stack | pool DiscardDirty | 65 | 7.85 | 4.78 | 0.1 | madvise(MADV_ZERO) | 1 | 68.2 |
| E contract: whole 1 MiB stack | pool RemapDirty | 65 | 42.42 | 6.32 | 65.0 | mmap(MAP_FIXED) | 1 | 68.2 |
| E contract: whole 1 MiB stack | pool Memset | 65 | 7.71 | 4.14 | 0.1 | memset | 1 | 68.2 |
| E contract: whole 1 MiB stack | pool Adaptive(512) | 65 | 8.30 | 4.49 | 0.1 | memset | 1 | 68.2 |
| E contract: whole 1 MiB stack | pool default (split slot) | 65 | 7.56 | 4.14 | 0.1 | memset | 1 | 68.2 |

## 2. VM path: new store + instantiate + one call + drop

| call | memory | µs/call | of which execute µs | faults/call | dirty pages | RSS after MiB |
|---|---|---:|---:|---:|---:|---:|
| A grow + 4 sparse writes | Vec (today) | 317.44 | 262.69 | 0.0 | - | 68.4 |
| A grow + 4 sparse writes | pool default | 1.15 | 0.15 | 0.0 | 4 | 68.5 |
| A grow + 4 sparse writes | pool, small pages only | 1.15 | 0.13 | 0.0 | 4 | 68.6 |
| A grow + 4 sparse writes | pool, Discard only | 14.48 | 0.17 | 0.0 | 4 | 68.6 |
| A grow + 4 sparse writes | pool, Memset only | 1.19 | 0.15 | 0.0 | 4 | 68.7 |
| B grow + 256 strided writes | Vec (today) | 330.99 | 277.42 | 0.0 | - | 68.7 |
| B grow + 256 strided writes | pool default | 50.59 | 8.51 | 0.6 | 256 | 72.7 |
| B grow + 256 strided writes | pool, small pages only | 53.48 | 8.40 | 0.6 | 256 | 76.6 |
| B grow + 256 strided writes | pool, Discard only | 39.27 | 8.31 | 0.6 | 256 | 80.5 |
| B grow + 256 strided writes | pool, Memset only | 51.43 | 9.45 | 0.6 | 256 | 84.5 |
| C grow + fill 64 MiB | Vec (today) | 800.78 | 747.25 | 0.0 | - | 84.5 |
| C grow + fill 64 MiB | pool default | 1007.19 | 656.62 | 192.0 | 4096 | 144.5 |
| C grow + fill 64 MiB | pool, small pages only | 1009.65 | 672.11 | 192.0 | 4096 | 204.5 |
| C grow + fill 64 MiB | pool, Discard only | 993.64 | 662.51 | 192.0 | 4096 | 264.5 |
| C grow + fill 64 MiB | pool, Memset only | 915.79 | 639.04 | 192.0 | 4096 | 324.5 |
| D contract: 64 KiB stack + 8 KiB data | Vec (today) | 5.21 | 0.64 | 0.0 | - | 324.5 |
| D contract: 64 KiB stack + 8 KiB data | pool default | 1.59 | 0.63 | 0.0 | 5 | 324.5 |
| D contract: 64 KiB stack + 8 KiB data | pool, small pages only | 1.54 | 0.62 | 0.0 | 5 | 324.5 |
| D contract: 64 KiB stack + 8 KiB data | pool, Discard only | 5.85 | 0.64 | 0.0 | 5 | 324.5 |
| D contract: 64 KiB stack + 8 KiB data | pool, Memset only | 1.59 | 0.64 | 0.0 | 5 | 324.5 |
| E contract: whole 1 MiB stack | Vec (today) | 12.24 | 7.73 | 0.0 | - | 324.5 |
| E contract: whole 1 MiB stack | pool default | 12.40 | 7.87 | 0.0 | 65 | 324.5 |
| E contract: whole 1 MiB stack | pool, small pages only | 12.12 | 7.68 | 0.0 | 65 | 324.5 |
| E contract: whole 1 MiB stack | pool, Discard only | 13.20 | 7.96 | 0.0 | 65 | 324.5 |
| E contract: whole 1 MiB stack | pool, Memset only | 12.39 | 7.86 | 0.0 | 65 | 324.5 |
| F instantiate only (no call) | Vec (today) | 4.52 | 0.01 | 0.0 | - | 324.5 |
| F instantiate only (no call) | pool default | 0.54 | 0.01 | 0.0 | 1 | 324.5 |
| F instantiate only (no call) | pool, small pages only | 0.53 | 0.01 | 0.0 | 1 | 324.5 |
| F instantiate only (no call) | pool, Discard only | 5.37 | 0.01 | 0.0 | 1 | 324.5 |
| F instantiate only (no call) | pool, Memset only | 0.52 | 0.02 | 0.0 | 1 | 324.5 |

## 3. Dirty tracking overhead (ns per loop iteration)

| loop | Vec | pool, tracking off | pool, tracking on | tracking on vs Vec |
|---|---:|---:|---:|---:|
| sequential i32.store within 64 KiB | 24.96 | 25.05 | 25.14 | +0.7% |
| i32.store on a new page each time | 25.06 | 25.30 | 25.44 | +1.5% |
| integer loop, no memory access | 23.38 | 24.14 | 23.40 | +0.1% |

## Appendix B. Reproducing

```bash
# tests: fresh-instance equivalence under every policy and slot layout, with reset verification
cargo test --features memory-pool --test memory_pool
cargo test --features memory-pool --lib memory

# harness, macOS or Linux host (about three minutes; --quick for a smoke run)
cargo bench --bench memory_pool --no-default-features --features memory-pool

# tests and harness in a Linux container on a Mac
docker run --rm --platform linux/arm64 -v "$PWD":/src -v rwasm-linux-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.93-slim-bookworm \
  bash -c "rustup target add wasm32-unknown-unknown && \
           cargo test --release --no-default-features --features memory-pool --test memory_pool && \
           cargo bench --bench memory_pool --no-default-features --features memory-pool"

# fuzzing: pooled, recycled memory against the Vec reference (needs a nightly of rustc 1.93+)
cd fuzz && cargo +nightly fuzz run resume_equivalence -- -max_total_time=600

# Miri over the Vec path of the memory (the nested wasm build of the fib example is skipped)
BUILD_RS_WASM_INNER=1 cargo +nightly miri test --no-default-features --features std \
  --lib vm::memory::tests
```

## Appendix C. File map

| file | what |
|---|---|
| `src/vm/memory_pool.rs` | `host_page_size`, `huge_page_size`, `MemorySlot` (reserve, split layout, `mark_dirty`, `for_each_dirty_run`, `reset`, `is_zeroed`), `ResetPolicy`, `MemoryPool`, `MemoryLease`, platform `discard_range`/`remap_range`/`map_aligned`, unit tests |
| `src/vm/memory.rs` | `GlobalMemory`: base pointer for both backings, `pooled`, the tracked-write API (`tracked_mut`, `store_window`, `copy_within`, `write`), `dirty_host_pages`, unit tests |
| `src/vm/store.rs` | `with_memory_pool`, `memory_pool`, `memory_dirty_host_pages`, `fresh_memory` used by `begin_instantiation` |
| `src/strategy/module.rs` | `create_executor_with_memory_pool` |
| `src/vm/executor.rs`, `src/vm/executor/fpu.rs`, `src/vm/executor/memory.rs` | stores and bulk operations on write windows |
| `tests/memory_pool.rs` | equivalence, image, capacity, replacement and strategy-layer tests |
| `fuzz/fuzz_targets/resume_equivalence.rs` | the pooled third executor and the reset check |
| `.github/workflows/ci.yml` | the second test run with the feature on |
| `benches/memory_pool.rs` | the harness behind appendix A |

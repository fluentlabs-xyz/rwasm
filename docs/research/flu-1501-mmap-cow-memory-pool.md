# mmap-backed memory pools for rwasm instance reuse (FLU-1501)

Research report and prototype, 2026-09-30. The prototype is the `memory-pool` cargo feature on
this branch: `src/vm/memory_pool.rs`, the pooled backing in `src/vm/memory.rs`, the lease
plumbing in `src/vm/store.rs`, the dirty marks in the VM's write paths, the equivalence tests in
`tests/memory_pool.rs` and the measurement harness in `benches/memory_pool.rs`. Everything is
inert without the feature, on non-Unix targets and in `no_std` builds.

## 0. Summary and recommendation

**Recommendation: implement, in a modified form.** Pool the linear memory only, lease one slot
per instance, and reset a released slot from host-owned metadata. Do not build an instance
template, a copy-on-write initial image or a "reset the whole store" primitive: every other piece
of instance state is rebuilt per call for well under a microsecond, and re-running the compiled
prologue on an all-zero slot reproduces the post-instantiation image by construction.

What the measurements say (Linux arm64 with 4 KiB pages in a Docker VM, macOS on an Apple M5 Max
with 16 KiB pages; one contract call = new store, instantiation, the call, release):

| call | today (`Vec`) Linux / macOS | pooled, default policy Linux / macOS |
|---|---:|---:|
| F: instantiate a 17-page contract, no call | 4.78 / 4.28 µs | 0.44 / 0.48 µs |
| D: typical call, 64 KiB of stack + 8 KiB of data | 5.34 / 5.14 µs | 1.79 / 1.55 µs |
| E: a call that writes its whole 1 MiB stack | 17.4 / 11.8 µs | 18.6 / 11.9 µs |
| A: grow to 64 MiB, write 4 pages | 859 / 319 µs | 0.73 / 1.14 µs |
| B: grow to 64 MiB, write 1 MiB in 256 KiB strides | 695 / 319 µs | 24.2 / 49.7 µs |
| C: grow to 64 MiB, fill all of it | 1866 / 774 µs | 7361 / 951 µs |

- Today's cost is `memset` of the *declared* memory (`GlobalMemory::grow` reserves and
  `resize`s a `Vec`): 4 µs per MiB when the allocator recycles the block, 860 µs for a 64 MiB
  memory on Linux, where the block comes from fresh `mmap` pages every time.
- With a pooled slot the cost follows the *touched* pages. For the sparse and the typical case
  the pool is 3x to 1000x cheaper; for a call that writes every page it is at parity (macOS) or
  worse (Linux, 4x, because slots opt out of transparent huge pages and each 4 KiB page then
  faults separately; section 2 discusses the fix).
- The reset primitive matters more than the ticket assumed. On both platforms a page fault costs
  more than zeroing the page by hand (Linux: 330 ns per 4 KiB fault against 40 ns of `memset`;
  macOS: 0.6 to 2 µs per 16 KiB fault). So the best policy for the common case is not
  `madvise(MADV_DONTNEED)` but `memset` of the dirty pages, which keeps them resident for the next
  lease; the kernel primitives take over above a dirty-set threshold (4 MiB by default), where
  the memory has to be given back.
- Explicit dirty tracking costs nothing measurable on Linux (within 1% on a loop that is nothing
  but stores) and 5% to 10% on such a loop on macOS across three runs, of which the bitmap itself
  is 1 to 3 points and the rest is the pooled memory access path (section 5 has the fix); 0% on
  code without stores. It is what makes the `memset` policy possible and it is kept.
- The correctness invariant holds: `tests/memory_pool.rs` runs eight prior executions (sparse and
  dense writes, growth, bulk operations with segment drops, a trap, `OutOfFuel`, an abandoned and
  a resumed interruption, a dense write set followed by a trap) under every reset policy with and
  without tracking, then compares the recycled instance against a fresh `Vec` instance step by
  step: post-instantiation image, prologue fuel, results, fuel, memory, tables and globals after
  each of 18 probe calls including another trap. The pool verifies every reset in these tests
  (`verify_reset`) and reported no failure.

Not done here: Linux x86-64 numbers (no such machine in reach; the harness runs unchanged there,
see appendix B), the Fluentbase integration (designed in section 11, a few lines of code) and
the Wasmtime side (section 11.4).

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
prototype leaves that as it is.

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
(row "instantiate only" above: 0.44 µs with a pooled slot, and that includes leasing the slot,
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

### 1.4 Prototype API

```rust
let pool = MemoryPool::new(MemoryPoolConfig {
    slot_pages: N_DEFAULT_MAX_MEMORY_PAGES, // 64 MiB of address space per slot
    max_free_slots: 8,
    reset_policy: MemoryPoolConfig::default_reset_policy(), // memset ≤ 4 MiB dirty, discard above
    track_dirty: true,
    verify_reset: false,   // read the range back after every reset (tests)
    no_huge_pages: true,   // MADV_NOHUGEPAGE on Linux
});
let mut store = RwasmStore::new(linker.clone(), ctx, handler, fuel, Some(pages))
    .with_memory_pool(pool.clone());
let instance = linker.instantiate(&mut store, engine, module)?; // leases a slot
instance.execute(&mut store, &params, &mut results)?;
drop(store);                                                      // resets and returns it
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
| one kernel call over the reachable range | 1.65 / 1.10 (1) | 17.3 / 77.4 (1) | 704 / 5498 (1) |
| one `mmap(MAP_FIXED)` over the reachable range | 2.43 / 1.58 (1) | 19.0 / 86.5 (1) | 730 / 5556 (1) |
| kernel call per dirty run | 2.20 / 1.07 (4) | 124 / 78.0 (256) | 705 / 5505 (1) |
| `mmap(MAP_FIXED)` per dirty run | 5.69 / 1.11 (4) | 329 / 81.4 (256) | 720 / 5550 (1) |
| `memset` of the dirty pages | 0.43 / 0.02 (4) | 14.6 / 15.8 (256) | 291 / 226 (1) |
| `munmap` + `mmap` a new slot | 2.81 / 1.57 | 19.5 / 86.3 | 734 / 5532 |

- `MADV_DONTNEED` over the whole reachable range costs about 1.5 µs plus 43 ns per resident page:
  the kernel walks the page tables and skips empty page-middle directories two megabytes at a
  time, so a sparse working set is cheap without any bitmap. Per-run `madvise` calls cost
  0.4 to 0.5 µs each and lose against one range call as soon as there are more than a handful
  of runs (124 µs for 256 runs against 17 µs). `MADV_DONTNEED` takes `mmap_lock` for reading;
  `mmap(MAP_FIXED)` takes it for writing and splits/merges VMAs, and it creates a new VMA that
  has forgotten `MADV_NOHUGEPAGE` (the prototype re-applies it; the first run of the harness,
  without that, showed a 4-page touch after a remap costing 28 µs because each touch faulted a
  2 MiB huge page).
- The discard is only half the price: every discarded page that the next call touches again
  faults, at about 330 ns per 4 KiB page in this VM. For the typical contract (18 dirty pages)
  that is 6 µs, more than the `Vec` path costs in total. `memset` of the same 18 pages takes
  about 1 µs, bitmap scan included, and keeps them resident, so the next call pays nothing.
- Hence the default policy, `Adaptive { memset_up_to_pages }`: zero the dirty pages by hand
  while the dirty set is at most 4 MiB (1024 pages here), otherwise one `MADV_DONTNEED` over the
  reachable range. Resident memory per pooled slot is bounded by the threshold.

**Transparent huge pages.** The VM runs with THP `always`. A `Vec` of 64 MiB is populated with
2 MiB pages (33 faults for the whole `memory.fill`), a slot with `MADV_NOHUGEPAGE` with 4 KiB
pages (16384 faults, 5.5 ms). That is why the dense row is 4x worse on the pool than on `Vec`,
and it is the only row where the pool loses. Without the opt-out a sparse touch would cost a
2 MiB zero-fill per touched region (the `Vec calloc` row: 27 µs for 4 touches, 8 MiB resident).
The fix to evaluate next is a split slot: 4 KiB granularity for the first few MiB, where every
contract's stack and data live, `MADV_HUGEPAGE` for the rest, which only memory-hungry calls
reach; a dense fill then costs 32 faults, and a sparse touch above the split costs at most 32
2 MiB pages (about 230 µs, still 4x better than today's 860 µs). Not implemented.

**Other primitives considered.** `MADV_FREE` after `memset` would let the kernel reclaim the
zeroed pages lazily (a reclaimed page reads back as zeros, so it is safe); it was not needed to
bound RSS with the 4 MiB threshold. `MADV_POPULATE_WRITE` on growth would prefault, which is the
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
| one kernel call over the reachable range | 14.8 / 0.03 (1) | 19.8 / 10.2 (1) | 311 / 100.0 (1) |
| one `mmap(MAP_FIXED)` over the reachable range | 31.2 / 8.54 (1) | 30.0 / 53.9 (1) | 384 / 2353 (1) |
| kernel call per dirty run | 1.21 / 0.04 (4) | 18.5 / 10.4 (64) | 296 / 82.2 (1) |
| `mmap(MAP_FIXED)` per dirty run | 11.1 / 9.21 (4) | 67.3 / 68.5 (64) | 450 / 2563 (1) |
| `memset` of the dirty pages | 1.15 / 0.04 (4) | 11.3 / 10.7 (64) | 270 / 86.8 (1) |
| `munmap` + `mmap` a new slot | 41.8 / 14.6 | 28.9 / 54.5 | 542 / 2599 |

- `MADV_ZERO` (Darwin, `libc::MADV_ZERO = 11`, available on this kernel) zero-fills the
  resident pages in place and leaves them resident, at `memset` speed, plus a range walk of
  about 15 µs per 64 MiB. Per dirty run it is 0.3 µs a call. Because the pages stay resident the
  next lease touches them for free (0.03 µs for 4 pages). The prototype probes it once and falls
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
- Default policy as on Linux: `memset` up to 4 MiB of dirty pages (256 pages here), the range
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

Every path that writes to linear memory marks the pages it wrote, after the bounds-checked write:

| path | where | mark |
|---|---|---|
| `i32.store`, `i32.store8`, `i32.store16` (and `i64.store*`, which the compiler lowers to pairs of `i32` stores) | `RwasmExecutor::execute_store_wrap`, `src/vm/executor.rs` | `offset + address`, `len` |
| `f32.store`, `f64.store` | `src/vm/executor/fpu.rs` | 4 or 8 bytes |
| `memory.fill` | `src/vm/executor/memory.rs` | `[d, d + n)` |
| `memory.copy` | same | `[dst, dst + n)` |
| `memory.init` | same | `[dst, dst + n)` |
| host writes: `StoreTr::memory_write` on `RwasmStore` and `RwasmCaller`, syscall handlers, `TypedCaller` | `GlobalMemory::write`, `src/vm/memory.rs` | the written range |
| `memory.grow` | `GlobalMemory::grow` | none: the new pages are zero; the high-water mark moves |

Reads (`execute_load_extend`, `memory_read`, the tracer, `memory_snapshot`) do not mark. Table
and global operations do not touch memory. `GlobalMemory::data_mut` is the only untracked way to
write and it has exactly the callers listed above (`grep data_mut src/vm`). Without the feature
`mark_dirty` is an empty inline function.

Cost (harness section 3, best of five, ns per loop iteration, loop = store + 6 other opcodes):

| loop | Linux `Vec` | Linux pooled, tracking on | macOS `Vec` | macOS pooled, tracking on |
|---|---:|---:|---:|---:|
| sequential `i32.store` in 64 KiB | 34.8 | 34.6 (-0.5%) | 26.9 | 29.4 (+9.1%) |
| `i32.store` on a new page each time | 35.8 | 36.0 (+0.8%) | 27.4 | 30.3 (+10.4%) |
| integer loop, no memory access | 26.1 | 26.0 (-0.1%) | 26.9 | 26.2 (-2.3%) |

On macOS "tracking off" already costs 29.0 and 29.3 ns against 26.9 and 27.4 for `Vec`, so most
of the 9% to 10% is the pooled `data_mut()` path (a branch and a pointer/length pair instead of
a `Vec`) and the bitmap itself adds 1.5 to 3 points; three runs of the harness put the total
between 5% and 10% on these store-only loops, and a real contract executes far fewer stores per
instruction. The access-path part can be removed by keeping a raw base pointer and length in
`GlobalMemory` for both backings, at the price of making the `pub shared_memory: Vec<u8>` field
private; left for the implementation. A first version of the prototype went through an `Option`
check per access and cost 13% to 15% on Linux, which is what the ticket's 5% target is about;
it is gone.

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
compared against a fresh instance across 18 probe calls, one of which traps again.

If a reset cannot vouch for the slot it is destroyed: a failed `madvise`/`mmap`, or a non-zero
byte found by `verify_reset`. The pool counts these (`reset_failures`, `slots_unmapped`); a host
can watch the counter. `verify_reset` is a debugging and testing aid (a pass over the reachable
range); it is not meant for production.

## 9. Benchmarks

Platforms: Linux 6.12 arm64 in a Docker Desktop VM (16 vCPUs, 8 GiB, 4 KiB pages, THP `always`)
and macOS 26.5 on the Apple M5 Max host of that VM (16 KiB pages); Linux x86-64 was not
available. Run-to-run variance is a few percent on the VM path and up to 5 points on the
tracking loops; the tables show one run each. Release build,
`cargo bench --bench memory_pool --features memory-pool`. The full tables are in appendix A; the
harness prints them.

**Fresh allocation versus reset (harness section 1, 64 MiB slot).** Today's path is the `Vec
resize` row: `try_reserve_exact` plus `resize(len, 0)`. The `Vec calloc` row is what
`alloc_zeroed` would do instead; on Linux it turns the 64 MiB case from 860 µs into 6 µs of
allocation plus huge-page faults on touch (27 µs for 4 touches), on macOS it changes nothing
(the allocator zeroes the recycled block itself). It is a one-line quick win for the first
`memory.grow` on Linux that needs no pool; noted, not implemented.

**The VM path (harness section 2).** New store, instantiation, one call, drop:

| call | Linux `Vec` | Linux pool | macOS `Vec` | macOS pool |
|---|---:|---:|---:|---:|
| F: instantiate a 17-page contract, no call | 4.78 | 0.44 | 4.28 | 0.48 |
| D: typical call, 64 KiB of stack + 8 KiB of data | 5.34 | 1.79 | 5.14 | 1.55 |
| E: a call that writes its whole 1 MiB stack | 17.4 | 18.6 | 11.8 | 11.9 |
| A: grow to 64 MiB, write 4 pages | 859 | 0.73 | 319 | 1.14 |
| B: grow to 64 MiB, write 1 MiB in 256 KiB strides | 695 | 24.2 | 319 | 49.7 |
| C: grow to 64 MiB, fill all of it | 1866 | 7361 | 774 | 951 |

(µs per call, pool = default `Adaptive` policy.) Reading the rows: F and D are the cases every
call goes through and the pool removes the `memset` of the declared memory; E writes as much as
it declares, so both zero 1 MiB; A and B are the ticket's sparse and moderate workloads at their
best; C is the dense workload, where zeroing and faulting 64 MiB dominates whatever the backend,
and on Linux the huge-page effect of section 2 shows.

The point where recreating beats resetting: on Linux the `memset` tier costs 18 ns per 4 KiB
page, `MADV_DONTNEED` 43 ns per resident page plus the 330 ns fault the next call pays; the
break-even for "keep resident" is therefore far above the 4 MiB threshold in time and the
threshold is set by how much RSS a pooled slot may retain, not by speed. A dense 64 MiB working
set costs 700 µs to discard, 290 µs to `memset`, and 5.5 ms to fault back in; with today's `Vec`
it costs 860 µs to allocate and 85 µs to fault (huge pages). Destroying and remapping the slot
(`munmap` + `mmap`, the "mmap+munmap per instance" rows) is never cheaper than `MADV_DONTNEED`
over it (734 vs 704 µs dense, 2.8 vs 1.7 µs sparse).

## 10. Memory, RSS and page-fault measurements

- **Virtual address space.** One slot reserves `slot_pages × 64 KiB` (64 MiB at the Fluentbase
  cap) with `MAP_NORESERVE`. A process with `max_free_slots = 8` and `n` live frames reserves
  `(8 + n) × 64 MiB`; at Fluentbase's transaction-wide bound of 1.5 GiB of in-flight logical
  memory that is at most 24 live slots, 2 GiB of address space, which is nothing on a 48-bit
  host.
- **Resident memory.** The `RSS after` columns of appendix A: on Linux RSS stays at the harness
  baseline (2.2 to 4 MiB) for every policy except `Memset` after the dense workload, where the
  slot keeps 64 MiB resident by design (66 MiB); the default policy discards above 4 MiB and
  ends at 2.2 MiB. Across 400 leases per row there is no growth. On macOS the `Vec` rows sit at
  66 MiB because `malloc` keeps the freed 64 MiB block; the pool rows add one 64 MiB slot per
  pool (four pools in section 2, hence 327 MiB at the end), resident after the dense row because
  `MADV_ZERO` zeroes in place. That is the one place where macOS needs the remap fallback or a
  small free list to give memory back; `MADV_FREE_REUSABLE` is not the answer (section 3).
- **Page faults.** `faults/iter` and `faults/call` are `getrusage` minor faults. The pool with
  the default policy faults 0 to 0.6 times per typical call (rows D, E, F); today's `Vec` path
  faults 0 for small memories (the allocator recycles the block) and 544 per 64 MiB memory on
  Linux (the block is mapped fresh every time and mostly populated with huge pages). Discard
  policies fault once per touched page on the next lease, which is why they lose to `memset` on
  small working sets.
- **Reservation versus commit.** Nothing is committed at lease time; a leased slot that the
  prologue grows to 17 pages commits the data segment's page(s) only (one host page for every
  surveyed contract but two), which `memory_dirty_host_pages()` reports as 1.

## 11. Integration design

### 11.1 rwasm

The prototype's surface is what an implementation would keep: `MemoryPool`, `MemoryPoolConfig`,
`ResetPolicy`, `RwasmStore::with_memory_pool`, `RwasmStore::memory_dirty_host_pages`,
`MemoryPool::stats`. Two additions for the strategy layer: `StrategyDefinition::create_executor`
gets a pool parameter (or a builder on `StrategyExecutor`) so that `StrategyExecutor::Rwasm`
stores lease from it, and `GlobalMemory` should carry a base pointer and length for both backings
so that the hot path has no backing branch (section 5). The Wasmtime executor ignores the pool;
its memories are Wasmtime's.

The feature stays optional and Unix-only. The `no_std` build (the zkVM guest, where there is no
`mmap` and instantiation happens once per proof) keeps the `Vec` backing unchanged, and the
`mark_dirty` calls compile to nothing there.

### 11.2 Fluentbase

- One `MemoryPool` per process (or per `RuntimeExecutor`), created with `slot_pages =
  N_DEFAULT_MAX_MEMORY_PAGES`, the per-frame cap the store is created with today
  (`ContractRuntime::new` passes `Some(N_DEFAULT_MAX_MEMORY_PAGES)`), so a slot always holds what
  the store may grow to.
- `ContractRuntime::new` calls `.with_memory_pool(pool.clone())` on the store (through the
  strategy layer once it has the parameter). That is the whole change: leases follow the store's
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
semantics: the equivalence tests are the contract, and they should move into the fuzzers
(`fuzz/fuzz_targets/resume_equivalence.rs` already compares two executors; a third one on a
pooled store, recycled between runs, is a small addition).

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
2. Reset by dirty bitmap with `memset` up to a resident-memory threshold (4 MiB), and by one
   kernel call over the reachable range above it: `madvise(MADV_DONTNEED)` on Linux,
   `madvise(MADV_ZERO)` on macOS, `mmap(MAP_FIXED)` inside the slot only as the fallback. Do not
   issue one kernel call per dirty run, do not remap as the primary primitive, and do not follow
   `MADV_ZERO` with `MADV_FREE_REUSABLE`.
3. Keep explicit dirty tracking in the VM's write paths; it is measured at ≤1% (Linux) and 5% to
   10% (macOS, of which the bitmap is 1 to 3 points) on pure store loops and 0% elsewhere, and
   it is what makes 2 possible. Remove the access-path share with a base pointer in
   `GlobalMemory` before shipping.
4. Drop the copy-on-write initial image: rwasm data segments are kilobytes and a CoW fault costs
   more than the copy.
5. Keep `MADV_NOHUGEPAGE` on the slots, and evaluate a split slot (4 KiB pages for the first
   MiBs, huge pages above) to recover the dense-workload loss on Linux before the feature is
   turned on for nodes.
6. Before shipping: the base-pointer path in `GlobalMemory`, the `create_executor` parameter,
   Linux x86-64 numbers on a node-like machine (the harness runs as is), and the pooled executor
   in the fuzzers.

Independent quick win, with or without the pool: allocate the first `memory.grow` with
`alloc_zeroed` instead of `resize`; on Linux that alone takes a 64 MiB instantiation from
860 µs to about 30 µs.

## Appendix A. Raw harness output

### A.1 Linux arm64 (Docker Desktop VM, kernel 6.12, 4 KiB pages, THP `always`)

# memory pool measurements

os=linux arch=aarch64 host_page=4096 thp=[always] madvise never quick=false

## 1. Slot reset primitives (64 MiB slot, host page 4 KiB)

| workload | backend / policy | dirty pages | touch µs | alloc+reset µs | faults/iter | reset method | calls | RSS after MiB |
|---|---|---:|---:|---:|---:|---|---:|---:|
| A sparse: 4 pages of 64 MiB | Vec resize (today) | - | 0.16 | 860.32 | 544.0 | alloc+drop | - | 2.2 |
| A sparse: 4 pages of 64 MiB | Vec calloc | - | 27.43 | 6.17 | 4.0 | alloc+drop | - | 2.2 |
| A sparse: 4 pages of 64 MiB | mmap+munmap per instance | - | 1.57 | 2.81 | 4.0 | mmap/munmap | - | 2.2 |
| A sparse: 4 pages of 64 MiB | pool Discard | 4 | 1.10 | 1.65 | 4.0 | madvise(MADV_DONTNEED) | 1 | 2.2 |
| A sparse: 4 pages of 64 MiB | pool Remap | 4 | 1.58 | 2.43 | 4.0 | mmap(MAP_FIXED) | 1 | 2.2 |
| A sparse: 4 pages of 64 MiB | pool DiscardDirty | 4 | 1.07 | 2.20 | 4.0 | madvise(MADV_DONTNEED) | 4 | 2.2 |
| A sparse: 4 pages of 64 MiB | pool RemapDirty | 4 | 1.11 | 5.69 | 4.0 | mmap(MAP_FIXED) | 4 | 2.2 |
| A sparse: 4 pages of 64 MiB | pool Memset | 4 | 0.02 | 0.43 | 0.0 | memset | 4 | 2.2 |
| A sparse: 4 pages of 64 MiB | pool Adaptive(1024) | 4 | 0.02 | 0.37 | 0.0 | memset | 4 | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec resize (today) | - | 31.36 | 861.83 | 544.0 | alloc+drop | - | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec calloc | - | 354.14 | 44.23 | 47.0 | alloc+drop | - | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | mmap+munmap per instance | - | 86.32 | 19.49 | 256.0 | mmap/munmap | - | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Discard | 256 | 77.38 | 17.30 | 256.0 | madvise(MADV_DONTNEED) | 1 | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Remap | 256 | 86.50 | 18.96 | 256.0 | mmap(MAP_FIXED) | 1 | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool DiscardDirty | 256 | 78.03 | 124.20 | 256.0 | madvise(MADV_DONTNEED) | 256 | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool RemapDirty | 256 | 81.37 | 329.24 | 256.0 | mmap(MAP_FIXED) | 256 | 2.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Memset | 256 | 15.76 | 14.64 | 0.5 | memset | 256 | 3.2 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Adaptive(1024) | 256 | 14.97 | 14.25 | 0.5 | memset | 256 | 3.2 |
| C dense: every page of 64 MiB | Vec resize (today) | - | 84.53 | 833.96 | 544.0 | alloc+drop | - | 2.2 |
| C dense: every page of 64 MiB | Vec calloc | - | 585.45 | 60.59 | 543.0 | alloc+drop | - | 2.2 |
| C dense: every page of 64 MiB | mmap+munmap per instance | - | 5531.89 | 734.06 | 16384.0 | mmap/munmap | - | 2.2 |
| C dense: every page of 64 MiB | pool Discard | 16384 | 5498.19 | 703.70 | 16384.0 | madvise(MADV_DONTNEED) | 1 | 2.2 |
| C dense: every page of 64 MiB | pool Remap | 16384 | 5556.21 | 729.51 | 16384.0 | mmap(MAP_FIXED) | 1 | 2.2 |
| C dense: every page of 64 MiB | pool DiscardDirty | 16384 | 5505.07 | 704.85 | 16384.0 | madvise(MADV_DONTNEED) | 1 | 2.2 |
| C dense: every page of 64 MiB | pool RemapDirty | 16384 | 5549.77 | 720.11 | 16384.0 | mmap(MAP_FIXED) | 1 | 2.2 |
| C dense: every page of 64 MiB | pool Memset | 16384 | 225.88 | 291.48 | 327.7 | memset | 1 | 66.2 |
| C dense: every page of 64 MiB | pool Adaptive(1024) | 16384 | 5583.27 | 728.01 | 16384.0 | madvise(MADV_DONTNEED) | 1 | 2.2 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec resize (today) | - | 0.72 | 6.11 | 1.1 | alloc+drop | - | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec calloc | - | 0.54 | 4.74 | 0.0 | alloc+drop | - | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | mmap+munmap per instance | - | 5.63 | 2.34 | 18.0 | mmap/munmap | - | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Discard | 18 | 5.67 | 1.46 | 18.0 | madvise(MADV_DONTNEED) | 1 | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Remap | 18 | 5.61 | 2.25 | 18.0 | mmap(MAP_FIXED) | 1 | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | pool DiscardDirty | 18 | 5.74 | 1.58 | 18.0 | madvise(MADV_DONTNEED) | 1 | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | pool RemapDirty | 18 | 5.70 | 2.55 | 18.0 | mmap(MAP_FIXED) | 1 | 3.0 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Memset | 18 | 0.58 | 1.00 | 0.0 | memset | 1 | 3.1 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Adaptive(1024) | 18 | 0.59 | 1.01 | 0.0 | memset | 1 | 3.1 |
| E contract: whole 1 MiB stack | Vec resize (today) | - | 12.36 | 5.27 | 0.0 | alloc+drop | - | 3.0 |
| E contract: whole 1 MiB stack | Vec calloc | - | 12.52 | 5.26 | 0.0 | alloc+drop | - | 3.0 |
| E contract: whole 1 MiB stack | mmap+munmap per instance | - | 76.25 | 9.88 | 258.0 | mmap/munmap | - | 3.0 |
| E contract: whole 1 MiB stack | pool Discard | 258 | 77.07 | 9.24 | 258.0 | madvise(MADV_DONTNEED) | 1 | 3.0 |
| E contract: whole 1 MiB stack | pool Remap | 258 | 74.63 | 9.43 | 258.0 | mmap(MAP_FIXED) | 1 | 3.0 |
| E contract: whole 1 MiB stack | pool DiscardDirty | 258 | 78.83 | 9.52 | 258.0 | madvise(MADV_DONTNEED) | 1 | 3.0 |
| E contract: whole 1 MiB stack | pool RemapDirty | 258 | 77.33 | 9.81 | 258.0 | mmap(MAP_FIXED) | 1 | 3.0 |
| E contract: whole 1 MiB stack | pool Memset | 258 | 12.90 | 5.23 | 0.5 | memset | 1 | 4.0 |
| E contract: whole 1 MiB stack | pool Adaptive(1024) | 258 | 12.66 | 4.97 | 0.5 | memset | 1 | 4.0 |

## 2. VM path: new store + instantiate + one call + drop

| call | memory | µs/call | of which execute µs | faults/call | dirty pages | RSS after MiB |
|---|---|---:|---:|---:|---:|---:|
| A grow + 4 sparse writes | Vec (today) | 858.95 | 794.50 | 544.0 | - | 4.3 |
| A grow + 4 sparse writes | pool Discard | 3.12 | 0.90 | 4.0 | 4 | 4.3 |
| A grow + 4 sparse writes | pool DiscardDirty | 3.57 | 0.90 | 4.0 | 4 | 4.3 |
| A grow + 4 sparse writes | pool Memset | 0.72 | 0.13 | 0.0 | 4 | 4.3 |
| A grow + 4 sparse writes | pool Adaptive(1024) | 0.73 | 0.13 | 0.0 | 4 | 4.3 |
| B grow + 256 strided writes | Vec (today) | 695.38 | 646.58 | 33.0 | - | 4.3 |
| B grow + 256 strided writes | pool Discard | 94.44 | 76.60 | 256.0 | 256 | 4.3 |
| B grow + 256 strided writes | pool DiscardDirty | 201.57 | 76.42 | 256.0 | 256 | 4.3 |
| B grow + 256 strided writes | pool Memset | 23.83 | 9.84 | 0.6 | 256 | 5.3 |
| B grow + 256 strided writes | pool Adaptive(1024) | 24.20 | 10.09 | 0.6 | 256 | 6.3 |
| C grow + fill 64 MiB | Vec (today) | 1866.30 | 1807.72 | 33.0 | - | 6.3 |
| C grow + fill 64 MiB | pool Discard | 7879.77 | 7028.17 | 16384.0 | 16384 | 6.3 |
| C grow + fill 64 MiB | pool DiscardDirty | 7428.60 | 6662.98 | 16384.0 | 16384 | 6.3 |
| C grow + fill 64 MiB | pool Memset | 1600.33 | 1303.98 | 806.4 | 16384 | 69.3 |
| C grow + fill 64 MiB | pool Adaptive(1024) | 7360.88 | 6627.50 | 16371.2 | 16384 | 68.3 |
| D contract: 64 KiB stack + 8 KiB data | Vec (today) | 5.34 | 0.69 | 0.0 | - | 68.3 |
| D contract: 64 KiB stack + 8 KiB data | pool Discard | 7.40 | 5.45 | 18.0 | 18 | 68.3 |
| D contract: 64 KiB stack + 8 KiB data | pool DiscardDirty | 7.41 | 5.47 | 18.0 | 18 | 68.3 |
| D contract: 64 KiB stack + 8 KiB data | pool Memset | 1.78 | 0.63 | 0.0 | 18 | 68.3 |
| D contract: 64 KiB stack + 8 KiB data | pool Adaptive(1024) | 1.79 | 0.63 | 0.0 | 18 | 68.4 |
| E contract: whole 1 MiB stack | Vec (today) | 17.39 | 12.19 | 0.0 | - | 68.4 |
| E contract: whole 1 MiB stack | pool Discard | 83.22 | 74.10 | 258.0 | 258 | 68.4 |
| E contract: whole 1 MiB stack | pool DiscardDirty | 83.52 | 74.30 | 258.0 | 258 | 68.4 |
| E contract: whole 1 MiB stack | pool Memset | 18.21 | 12.82 | 0.0 | 258 | 68.4 |
| E contract: whole 1 MiB stack | pool Adaptive(1024) | 18.58 | 13.29 | 0.6 | 258 | 69.3 |
| F instantiate only (no call) | Vec (today) | 4.78 | 0.01 | 0.0 | - | 69.3 |
| F instantiate only (no call) | pool Discard | 1.42 | 0.01 | 1.0 | 1 | 69.3 |
| F instantiate only (no call) | pool DiscardDirty | 1.25 | 0.02 | 1.0 | 1 | 69.3 |
| F instantiate only (no call) | pool Memset | 0.45 | 0.01 | 0.0 | 1 | 69.3 |
| F instantiate only (no call) | pool Adaptive(1024) | 0.44 | 0.02 | 0.0 | 1 | 69.3 |

## 3. Dirty tracking overhead (ns per loop iteration)

| loop | Vec | pool, tracking off | pool, tracking on | tracking on vs Vec |
|---|---:|---:|---:|---:|
| sequential i32.store within 64 KiB | 34.79 | 34.93 | 34.62 | -0.5% |
| i32.store on a new page each time | 35.78 | 36.22 | 36.05 | +0.8% |
| integer loop, no memory access | 26.05 | 25.94 | 26.03 | -0.1% |

### A.2 macOS 26.5, Apple M5 Max (16 KiB pages)

# memory pool measurements

os=macos arch=aarch64 host_page=16384 thp=n/a quick=false

## 1. Slot reset primitives (64 MiB slot, host page 16 KiB)

| workload | backend / policy | dirty pages | touch µs | alloc+reset µs | faults/iter | reset method | calls | RSS after MiB |
|---|---|---:|---:|---:|---:|---|---:|---:|
| A sparse: 4 pages of 64 MiB | Vec resize (today) | - | 0.05 | 331.72 | 8.2 | alloc+drop | - | 65.9 |
| A sparse: 4 pages of 64 MiB | Vec calloc | - | 0.06 | 282.50 | 0.0 | alloc+drop | - | 65.9 |
| A sparse: 4 pages of 64 MiB | mmap+munmap per instance | - | 14.64 | 41.82 | 4.0 | mmap/munmap | - | 66.0 |
| A sparse: 4 pages of 64 MiB | pool Discard | 4 | 0.03 | 14.79 | 0.0 | madvise(MADV_ZERO) | 1 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool Remap | 4 | 8.54 | 31.20 | 4.0 | mmap(MAP_FIXED) | 1 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool DiscardDirty | 4 | 0.04 | 1.21 | 0.0 | madvise(MADV_ZERO) | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool RemapDirty | 4 | 9.21 | 11.13 | 4.0 | mmap(MAP_FIXED) | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool Memset | 4 | 0.04 | 1.15 | 0.0 | memset | 4 | 130.0 |
| A sparse: 4 pages of 64 MiB | pool Adaptive(256) | 4 | 0.08 | 1.55 | 0.0 | memset | 4 | 130.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec resize (today) | - | 18.76 | 271.30 | 0.0 | alloc+drop | - | 66.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | Vec calloc | - | 22.66 | 294.44 | 0.0 | alloc+drop | - | 66.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | mmap+munmap per instance | - | 54.53 | 28.95 | 64.0 | mmap/munmap | - | 66.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Discard | 64 | 10.20 | 19.82 | 0.1 | madvise(MADV_ZERO) | 1 | 130.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Remap | 64 | 53.91 | 30.01 | 64.0 | mmap(MAP_FIXED) | 1 | 130.0 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool DiscardDirty | 64 | 10.40 | 18.50 | 0.1 | madvise(MADV_ZERO) | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool RemapDirty | 64 | 68.46 | 67.32 | 64.0 | mmap(MAP_FIXED) | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Memset | 64 | 10.67 | 11.33 | 0.1 | memset | 64 | 130.1 |
| B spread: 1 MiB in 256 KiB strides of 64 MiB | pool Adaptive(256) | 64 | 10.63 | 11.28 | 0.1 | memset | 64 | 130.1 |
| C dense: every page of 64 MiB | Vec resize (today) | - | 25.07 | 279.22 | 0.0 | alloc+drop | - | 66.1 |
| C dense: every page of 64 MiB | Vec calloc | - | 27.06 | 281.11 | 0.0 | alloc+drop | - | 66.1 |
| C dense: every page of 64 MiB | mmap+munmap per instance | - | 2599.21 | 541.51 | 4096.0 | mmap/munmap | - | 66.1 |
| C dense: every page of 64 MiB | pool Discard | 4096 | 99.95 | 310.88 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool Remap | 4096 | 2352.84 | 384.50 | 4096.0 | mmap(MAP_FIXED) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool DiscardDirty | 4096 | 82.24 | 296.29 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool RemapDirty | 4096 | 2563.13 | 450.16 | 4096.0 | mmap(MAP_FIXED) | 1 | 130.1 |
| C dense: every page of 64 MiB | pool Memset | 4096 | 86.77 | 270.44 | 81.9 | memset | 1 | 130.1 |
| C dense: every page of 64 MiB | pool Adaptive(256) | 4096 | 91.31 | 352.94 | 81.9 | madvise(MADV_ZERO) | 1 | 130.1 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec resize (today) | - | 0.60 | 4.50 | 0.1 | alloc+drop | - | 67.2 |
| D contract: 64 KiB of stack + 8 KiB of data | Vec calloc | - | 0.65 | 5.45 | 0.0 | alloc+drop | - | 67.2 |
| D contract: 64 KiB of stack + 8 KiB of data | mmap+munmap per instance | - | 4.38 | 3.46 | 5.0 | mmap/munmap | - | 67.2 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Discard | 5 | 0.55 | 0.79 | 0.0 | madvise(MADV_ZERO) | 1 | 68.3 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Remap | 5 | 5.14 | 3.76 | 5.0 | mmap(MAP_FIXED) | 1 | 68.3 |
| D contract: 64 KiB of stack + 8 KiB of data | pool DiscardDirty | 5 | 0.55 | 0.70 | 0.0 | madvise(MADV_ZERO) | 1 | 68.3 |
| D contract: 64 KiB of stack + 8 KiB of data | pool RemapDirty | 5 | 4.16 | 2.69 | 5.0 | mmap(MAP_FIXED) | 1 | 68.3 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Memset | 5 | 0.54 | 0.51 | 0.0 | memset | 1 | 68.3 |
| D contract: 64 KiB of stack + 8 KiB of data | pool Adaptive(256) | 5 | 0.68 | 0.69 | 0.0 | memset | 1 | 68.3 |
| E contract: whole 1 MiB stack | Vec resize (today) | - | 8.00 | 4.57 | 0.0 | alloc+drop | - | 67.2 |
| E contract: whole 1 MiB stack | Vec calloc | - | 8.71 | 5.64 | 0.1 | alloc+drop | - | 68.1 |
| E contract: whole 1 MiB stack | mmap+munmap per instance | - | 44.50 | 8.47 | 65.0 | mmap/munmap | - | 68.1 |
| E contract: whole 1 MiB stack | pool Discard | 65 | 8.29 | 4.92 | 0.1 | madvise(MADV_ZERO) | 1 | 69.2 |
| E contract: whole 1 MiB stack | pool Remap | 65 | 48.80 | 8.88 | 65.0 | mmap(MAP_FIXED) | 1 | 69.2 |
| E contract: whole 1 MiB stack | pool DiscardDirty | 65 | 8.30 | 5.06 | 0.1 | madvise(MADV_ZERO) | 1 | 69.2 |
| E contract: whole 1 MiB stack | pool RemapDirty | 65 | 50.97 | 10.04 | 65.0 | mmap(MAP_FIXED) | 1 | 69.2 |
| E contract: whole 1 MiB stack | pool Memset | 65 | 8.43 | 4.73 | 0.1 | memset | 1 | 69.2 |
| E contract: whole 1 MiB stack | pool Adaptive(256) | 65 | 9.29 | 5.01 | 0.1 | memset | 1 | 69.2 |

## 2. VM path: new store + instantiate + one call + drop

| call | memory | µs/call | of which execute µs | faults/call | dirty pages | RSS after MiB |
|---|---|---:|---:|---:|---:|---:|
| A grow + 4 sparse writes | Vec (today) | 319.22 | 268.45 | 0.2 | - | 70.5 |
| A grow + 4 sparse writes | pool Discard | 15.04 | 0.15 | 0.0 | 4 | 70.6 |
| A grow + 4 sparse writes | pool DiscardDirty | 1.68 | 0.14 | 0.0 | 4 | 70.7 |
| A grow + 4 sparse writes | pool Memset | 1.14 | 0.14 | 0.0 | 4 | 70.7 |
| A grow + 4 sparse writes | pool Adaptive(256) | 1.14 | 0.15 | 0.0 | 4 | 70.8 |
| B grow + 256 strided writes | Vec (today) | 319.25 | 266.31 | 0.0 | - | 70.8 |
| B grow + 256 strided writes | pool Discard | 39.40 | 9.21 | 0.6 | 256 | 74.7 |
| B grow + 256 strided writes | pool DiscardDirty | 76.08 | 9.41 | 0.6 | 256 | 78.7 |
| B grow + 256 strided writes | pool Memset | 54.28 | 9.64 | 0.6 | 256 | 82.6 |
| B grow + 256 strided writes | pool Adaptive(256) | 49.71 | 9.38 | 0.6 | 256 | 86.6 |
| C grow + fill 64 MiB | Vec (today) | 774.49 | 723.32 | 0.0 | - | 86.6 |
| C grow + fill 64 MiB | pool Discard | 972.62 | 616.65 | 192.0 | 4096 | 146.6 |
| C grow + fill 64 MiB | pool DiscardDirty | 930.50 | 616.80 | 192.0 | 4096 | 206.6 |
| C grow + fill 64 MiB | pool Memset | 897.58 | 630.38 | 192.0 | 4096 | 266.6 |
| C grow + fill 64 MiB | pool Adaptive(256) | 950.97 | 636.68 | 192.0 | 4096 | 326.6 |
| D contract: 64 KiB stack + 8 KiB data | Vec (today) | 5.14 | 0.62 | 0.0 | - | 326.6 |
| D contract: 64 KiB stack + 8 KiB data | pool Discard | 5.78 | 0.63 | 0.0 | 5 | 326.6 |
| D contract: 64 KiB stack + 8 KiB data | pool DiscardDirty | 1.70 | 0.69 | 0.0 | 5 | 326.6 |
| D contract: 64 KiB stack + 8 KiB data | pool Memset | 1.52 | 0.62 | 0.0 | 5 | 326.6 |
| D contract: 64 KiB stack + 8 KiB data | pool Adaptive(256) | 1.55 | 0.62 | 0.0 | 5 | 326.6 |
| E contract: whole 1 MiB stack | Vec (today) | 11.80 | 7.45 | 0.0 | - | 326.6 |
| E contract: whole 1 MiB stack | pool Discard | 12.69 | 7.67 | 0.0 | 65 | 326.6 |
| E contract: whole 1 MiB stack | pool DiscardDirty | 12.45 | 7.60 | 0.0 | 65 | 326.6 |
| E contract: whole 1 MiB stack | pool Memset | 11.82 | 7.50 | 0.0 | 65 | 326.6 |
| E contract: whole 1 MiB stack | pool Adaptive(256) | 11.87 | 7.57 | 0.0 | 65 | 326.6 |
| F instantiate only (no call) | Vec (today) | 4.28 | 0.01 | 0.0 | - | 326.6 |
| F instantiate only (no call) | pool Discard | 5.03 | 0.01 | 0.0 | 1 | 326.6 |
| F instantiate only (no call) | pool DiscardDirty | 0.57 | 0.01 | 0.0 | 1 | 326.6 |
| F instantiate only (no call) | pool Memset | 0.47 | 0.01 | 0.0 | 1 | 326.6 |
| F instantiate only (no call) | pool Adaptive(256) | 0.48 | 0.01 | 0.0 | 1 | 326.6 |

## 3. Dirty tracking overhead (ns per loop iteration)

| loop | Vec | pool, tracking off | pool, tracking on | tracking on vs Vec |
|---|---:|---:|---:|---:|
| sequential i32.store within 64 KiB | 26.94 | 28.98 | 29.41 | +9.1% |
| i32.store on a new page each time | 27.45 | 29.32 | 30.30 | +10.4% |
| integer loop, no memory access | 26.85 | 26.52 | 26.24 | -2.3% |

## Appendix B. Reproducing

```bash
# tests: fresh-instance equivalence under every policy, with reset verification
cargo test --features memory-pool --test memory_pool
cargo test --features memory-pool --lib memory_pool

# harness, macOS or Linux host (about two minutes; --quick for a smoke run)
cargo bench --bench memory_pool --no-default-features --features memory-pool

# harness in a Linux container on a Mac
docker run --rm --platform linux/arm64 -v "$PWD":/src -v rwasm-linux-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.93-slim-bookworm \
  bash -c "rustup target add wasm32-unknown-unknown && \
           cargo bench --bench memory_pool --no-default-features --features memory-pool"
```

## Appendix C. Prototype file map

| file | what |
|---|---|
| `src/vm/memory_pool.rs` | `host_page_size`, `MemorySlot` (reserve, `mark_dirty`, `for_each_dirty_run`, `reset`, `is_zeroed`), `ResetPolicy`, `MemoryPool`, `MemoryLease`, platform `discard_range`/`remap_range`, unit tests |
| `src/vm/memory.rs` | `GlobalMemory::pooled`, the pooled `data`/`data_mut`/`grow`, `mark_dirty`, `dirty_host_pages` |
| `src/vm/store.rs` | `with_memory_pool`, `memory_pool`, `memory_dirty_host_pages`, `fresh_memory` used by `begin_instantiation` |
| `src/vm/executor.rs`, `src/vm/executor/fpu.rs`, `src/vm/executor/memory.rs` | dirty marks after stores and bulk operations |
| `tests/memory_pool.rs` | equivalence, image, capacity and replacement tests |
| `benches/memory_pool.rs` | the harness behind appendix A |

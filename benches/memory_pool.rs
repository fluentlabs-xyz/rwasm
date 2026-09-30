//! Measurements for the FLU-1501 memory pool prototype (`docs/research/`).
//!
//! ```bash
//! cargo bench --bench memory_pool --features memory-pool -- [--quick]
//! ```
//!
//! Three sections, each printed as a Markdown table:
//!
//! 1. the slot primitives alone: touching a working set and resetting it, per reset policy,
//!    against `Vec` and bare `mmap` baselines;
//! 2. the VM path: a fresh store, instantiation, one contract call and the release, with a
//!    `Vec` memory and with pooled memory;
//! 3. the cost of dirty tracking on store-heavy and store-free loops.
//!
//! Every row reports wall time per iteration, minor page faults per iteration (from
//! `getrusage`) and the process RSS after the loop.

use rwasm::{
    always_failing_syscall_handler, host_page_size, CompilationConfig, ExecutionEngine,
    ImportLinker, MemoryPool, MemoryPoolConfig, MemorySlot, ResetPolicy, RwasmModule, RwasmStore,
    Value, N_BYTES_PER_MEMORY_PAGE,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const MIB: usize = 1024 * 1024;
const SLOT_PAGES: u32 = 1024; // 64 MiB, the Fluentbase per-frame cap
const CONTRACT_PAGES: usize = 17; // 1 MiB shadow stack plus data, like every Fluentbase contract

fn minor_faults() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `usage` is a valid out-pointer.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    usage.ru_minflt as u64
}

#[cfg(target_os = "linux")]
fn rss_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let resident_pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .and_then(|field| field.parse().ok())
        .unwrap_or(0);
    resident_pages * host_page_size() as u64
}

#[cfg(target_vendor = "apple")]
fn rss_bytes() -> u64 {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a valid buffer of the size passed.
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    if written == size {
        info.pti_resident_size
    } else {
        0
    }
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
fn rss_bytes() -> u64 {
    0
}

fn thp_setting() -> String {
    std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/enabled")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "n/a".to_string())
}

fn us(d: Duration, iters: usize) -> f64 {
    d.as_secs_f64() * 1e6 / iters as f64
}

/// One write of `len` bytes at `offset`.
#[derive(Clone)]
struct Workload {
    name: &'static str,
    /// Bytes the guest could reach (the logical memory size).
    logical_len: usize,
    writes: Vec<(usize, usize)>,
}

fn workloads() -> Vec<Workload> {
    let page = host_page_size();
    let slot = SLOT_PAGES as usize * N_BYTES_PER_MEMORY_PAGE as usize;
    let contract = CONTRACT_PAGES * N_BYTES_PER_MEMORY_PAGE as usize;
    vec![
        Workload {
            name: "A sparse: 4 pages of 64 MiB",
            logical_len: slot,
            writes: vec![(0, 1), (MIB, 1), (16 * MIB, 1), (60 * MIB, 1)],
        },
        Workload {
            name: "B spread: 1 MiB in 256 KiB strides of 64 MiB",
            logical_len: slot,
            writes: (0..MIB / page).map(|i| (i * 256 * 1024, page)).collect(),
        },
        Workload {
            name: "C dense: every page of 64 MiB",
            logical_len: slot,
            writes: (0..slot / page).map(|i| (i * page, 1)).collect(),
        },
        Workload {
            name: "D contract: 64 KiB of stack + 8 KiB of data",
            logical_len: contract,
            writes: vec![(MIB - 64 * 1024, 64 * 1024), (MIB, 8 * 1024)],
        },
        Workload {
            name: "E contract: whole 1 MiB stack",
            logical_len: contract,
            writes: vec![(0, MIB + 8 * 1024)],
        },
    ]
}

fn touch(bytes: &mut [u8], writes: &[(usize, usize)]) {
    for &(offset, len) in writes {
        bytes[offset..offset + len].fill(1);
    }
}

fn policies() -> [ResetPolicy; 6] {
    [
        ResetPolicy::Discard,
        ResetPolicy::Remap,
        ResetPolicy::DiscardDirty,
        ResetPolicy::RemapDirty,
        ResetPolicy::Memset,
        MemoryPoolConfig::default_reset_policy(),
    ]
}

fn policy_name(policy: ResetPolicy) -> String {
    match policy {
        ResetPolicy::Adaptive { memset_up_to_pages } => format!("Adaptive({memset_up_to_pages})"),
        other => format!("{other:?}"),
    }
}

/// Section 1: the slot primitives.
fn section_slot_reset(quick: bool) {
    println!(
        "\n## 1. Slot reset primitives (64 MiB slot, host page {} KiB)\n",
        host_page_size() / 1024
    );
    println!("| workload | backend / policy | dirty pages | touch µs | alloc+reset µs | faults/iter | reset method | calls | RSS after MiB |");
    println!("|---|---|---:|---:|---:|---:|---|---:|---:|");
    let slot_len = SLOT_PAGES as usize * N_BYTES_PER_MEMORY_PAGE as usize;
    for workload in workloads() {
        let iters = if workload.writes.len() > 1000 {
            50
        } else {
            500
        } / if quick { 5 } else { 1 };
        let iters = iters.max(5);
        // Vec baselines: what `GlobalMemory` does today (`resize` = memset of the logical size)
        // and what `vec![0; n]` (calloc) would do.
        for (name, make) in [
            (
                "Vec resize (today)",
                (|len: usize| {
                    let mut v: Vec<u8> = Vec::new();
                    v.try_reserve_exact(len).unwrap();
                    v.resize(len, 0);
                    v
                }) as fn(usize) -> Vec<u8>,
            ),
            (
                "Vec calloc",
                (|len: usize| vec![0u8; len]) as fn(usize) -> Vec<u8>,
            ),
        ] {
            let mut alloc_time = Duration::ZERO;
            let mut touch_time = Duration::ZERO;
            let faults = minor_faults();
            for _ in 0..iters {
                let t0 = Instant::now();
                let mut v = make(workload.logical_len);
                let t1 = Instant::now();
                touch(&mut v, &workload.writes);
                let t2 = Instant::now();
                drop(v);
                let t3 = Instant::now();
                alloc_time += (t1 - t0) + (t3 - t2);
                touch_time += t2 - t1;
            }
            let faults = (minor_faults() - faults) as f64 / iters as f64;
            println!(
                "| {} | {} | - | {:.2} | {:.2} | {:.1} | alloc+drop | - | {:.1} |",
                workload.name,
                name,
                us(touch_time, iters),
                us(alloc_time, iters),
                faults,
                rss_bytes() as f64 / MIB as f64
            );
        }
        // bare mmap per instance, no pool
        {
            let mut alloc_time = Duration::ZERO;
            let mut touch_time = Duration::ZERO;
            let faults = minor_faults();
            for _ in 0..iters {
                let t0 = Instant::now();
                let mut slot = MemorySlot::reserve(workload.logical_len, false, true).unwrap();
                let t1 = Instant::now();
                touch(slot.as_mut_slice(workload.logical_len), &workload.writes);
                let t2 = Instant::now();
                drop(slot);
                let t3 = Instant::now();
                alloc_time += (t1 - t0) + (t3 - t2);
                touch_time += t2 - t1;
            }
            let faults = (minor_faults() - faults) as f64 / iters as f64;
            println!(
                "| {} | mmap+munmap per instance | - | {:.2} | {:.2} | {:.1} | mmap/munmap | - | {:.1} |",
                workload.name, us(touch_time, iters), us(alloc_time, iters), faults, rss_bytes() as f64 / MIB as f64
            );
        }
        // the pooled slot under every reset policy
        for policy in policies() {
            let mut slot = MemorySlot::reserve(slot_len, true, true).unwrap();
            let mut touch_time = Duration::ZERO;
            let mut reset_time = Duration::ZERO;
            let mut stats = Default::default();
            let mut dirty = 0;
            let faults = minor_faults();
            for _ in 0..iters {
                let t0 = Instant::now();
                slot.note_accessible(workload.logical_len);
                let bytes = slot.as_mut_slice(workload.logical_len);
                touch(bytes, &workload.writes);
                for &(offset, len) in &workload.writes {
                    slot.mark_dirty(offset, len);
                }
                dirty = slot.dirty_pages();
                let t1 = Instant::now();
                stats = slot.reset(policy, false).unwrap();
                let t2 = Instant::now();
                touch_time += t1 - t0;
                reset_time += t2 - t1;
            }
            let faults = (minor_faults() - faults) as f64 / iters as f64;
            assert!(
                slot.is_zeroed(workload.logical_len),
                "{policy:?} left dirty bytes"
            );
            println!(
                "| {} | pool {} | {} | {:.2} | {:.2} | {:.1} | {} | {} | {:.1} |",
                workload.name,
                policy_name(policy),
                dirty,
                us(touch_time, iters),
                us(reset_time, iters),
                faults,
                stats.method,
                stats.calls,
                rss_bytes() as f64 / MIB as f64
            );
        }
    }
}

/// The contract used by sections 2 and 3: `main(op, n)`.
const CONTRACT: &str = r#"(module
  (memory 17)
  (global $sp (mut i32) (i32.const 1048576))
  (data (i32.const 1048576) "\01\02\03\04\05\06\07\08initial-data-segment")
  (func $grow_to_64mib
    (drop (memory.grow (i32.sub (i32.const 1024) (memory.size)))))
  (func (export "main") (param $op i32) (param $n i32) (local $i i32) (local $acc i32) (local $b i32)
    ;; 0: A sparse writes in 64 MiB
    (if (i32.eq (local.get $op) (i32.const 0)) (then
      (call $grow_to_64mib)
      (i32.store8 (i32.const 0) (i32.const 1))
      (i32.store8 (i32.const 1048576) (i32.const 1))
      (i32.store8 (i32.const 16777216) (i32.const 1))
      (i32.store8 (i32.const 62914560) (i32.const 1))
      (return)))
    ;; 1: B 256 writes at 256 KiB strides in 64 MiB
    (if (i32.eq (local.get $op) (i32.const 1)) (then
      (call $grow_to_64mib)
      (loop $l
        (i32.store8 (i32.mul (local.get $i) (i32.const 262144)) (i32.const 1))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $l (i32.lt_u (local.get $i) (i32.const 256))))
      (return)))
    ;; 2: C fill all 64 MiB
    (if (i32.eq (local.get $op) (i32.const 2)) (then
      (call $grow_to_64mib)
      (memory.fill (i32.const 0) (i32.const 1) (i32.const 67108864))
      (return)))
    ;; 3: D a contract call: 64 KiB of stack, 8 KiB of data
    (if (i32.eq (local.get $op) (i32.const 3)) (then
      (memory.fill (i32.sub (global.get $sp) (i32.const 65536)) (i32.const 7) (i32.const 65536))
      (memory.fill (i32.const 1048576) (i32.const 9) (i32.const 8192))
      (return)))
    ;; 4: E the whole 1 MiB stack
    (if (i32.eq (local.get $op) (i32.const 4)) (then
      (memory.fill (i32.const 0) (i32.const 7) (i32.const 1056768))
      (return)))
    ;; 5: n sequential i32 stores inside 64 KiB
    (if (i32.eq (local.get $op) (i32.const 5)) (then
      (loop $l
        (i32.store (i32.and (i32.shl (local.get $i) (i32.const 2)) (i32.const 65532)) (local.get $i))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $l (i32.lt_u (local.get $i) (local.get $n))))
      (return)))
    ;; 6: n i32 stores, each on another page of the first 1 MiB
    (if (i32.eq (local.get $op) (i32.const 6)) (then
      (loop $l
        (i32.store (i32.and (i32.shl (local.get $i) (i32.const 12)) (i32.const 1048572)) (local.get $i))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $l (i32.lt_u (local.get $i) (local.get $n))))
      (return)))
    ;; 7: n iterations of an integer loop with no memory access
    (local.set $acc (i32.const 1))
    (local.set $b (i32.const 1))
    (loop $l
      (local.set $b (i32.add (local.get $acc) (local.get $b)))
      (local.set $acc (i32.sub (local.get $b) (local.get $acc)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (local.get $n))))
    (drop (local.get $acc))
  )
)"#;

fn compile_contract() -> RwasmModule {
    RwasmModule::compile(
        CompilationConfig::default().with_entrypoint_name("main".into()),
        &wat::parse_str(CONTRACT).unwrap(),
    )
    .unwrap()
    .0
}

fn new_store(pool: Option<&MemoryPool>) -> RwasmStore<()> {
    let store = RwasmStore::new(
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        Some(SLOT_PAGES),
    );
    match pool {
        Some(pool) => store.with_memory_pool(pool.clone()),
        None => store,
    }
}

/// Section 2: store creation, instantiation, one call, release.
fn section_vm(quick: bool) {
    println!("\n## 2. VM path: new store + instantiate + one call + drop\n");
    println!("| call | memory | µs/call | of which execute µs | faults/call | dirty pages | RSS after MiB |");
    println!("|---|---|---:|---:|---:|---:|---:|");
    let module = compile_contract();
    let engine = ExecutionEngine::new();
    let linker = Arc::new(ImportLinker::default());
    let calls = [
        (0, "A grow + 4 sparse writes"),
        (1, "B grow + 256 strided writes"),
        (2, "C grow + fill 64 MiB"),
        (3, "D contract: 64 KiB stack + 8 KiB data"),
        (4, "E contract: whole 1 MiB stack"),
        (8, "F instantiate only (no call)"),
    ];
    let backends: Vec<(String, Option<MemoryPool>)> =
        std::iter::once(("Vec (today)".to_string(), None))
            .chain(
                [
                    ResetPolicy::Discard,
                    ResetPolicy::DiscardDirty,
                    ResetPolicy::Memset,
                    MemoryPoolConfig::default_reset_policy(),
                ]
                .into_iter()
                .map(|policy| {
                    (
                        format!("pool {}", policy_name(policy)),
                        Some(MemoryPool::new(MemoryPoolConfig {
                            slot_pages: SLOT_PAGES,
                            reset_policy: policy,
                            ..MemoryPoolConfig::default()
                        })),
                    )
                }),
            )
            .collect();
    for (op, name) in calls {
        for (backend, pool) in &backends {
            let iters = if op == 2 { 20 } else { 400 } / if quick { 4 } else { 1 };
            let mut total = Duration::ZERO;
            let mut execute = Duration::ZERO;
            let mut dirty = None;
            let faults = minor_faults();
            for _ in 0..iters {
                let t0 = Instant::now();
                let mut store = new_store(pool.as_ref());
                let instance = linker
                    .instantiate(&mut store, engine, module.clone())
                    .unwrap();
                let t1 = Instant::now();
                if op != 8 {
                    instance
                        .execute(&mut store, &[Value::I32(op), Value::I32(0)], &mut [])
                        .unwrap();
                }
                let t2 = Instant::now();
                dirty = store.memory_dirty_host_pages();
                drop(instance);
                drop(store);
                let t3 = Instant::now();
                total += t3 - t0;
                execute += t2 - t1;
            }
            let faults = (minor_faults() - faults) as f64 / iters as f64;
            println!(
                "| {} | {} | {:.2} | {:.2} | {:.1} | {} | {:.1} |",
                name,
                backend,
                us(total, iters),
                us(execute, iters),
                faults,
                dirty.map(|d| d.to_string()).unwrap_or_else(|| "-".into()),
                rss_bytes() as f64 / MIB as f64
            );
        }
    }
}

/// Section 3: dirty tracking on the hot path.
fn section_tracking(quick: bool) {
    println!("\n## 3. Dirty tracking overhead (ns per loop iteration)\n");
    println!("| loop | Vec | pool, tracking off | pool, tracking on | tracking on vs Vec |");
    println!("|---|---:|---:|---:|---:|");
    let module = compile_contract();
    let engine = ExecutionEngine::new();
    let linker = Arc::new(ImportLinker::default());
    let n: i32 = if quick { 500_000 } else { 5_000_000 };
    let untracked = MemoryPool::new(MemoryPoolConfig {
        slot_pages: SLOT_PAGES,
        track_dirty: false,
        ..MemoryPoolConfig::default()
    });
    let tracked = MemoryPool::new(MemoryPoolConfig {
        slot_pages: SLOT_PAGES,
        track_dirty: true,
        ..MemoryPoolConfig::default()
    });
    for (op, name) in [
        (5, "sequential i32.store within 64 KiB"),
        (6, "i32.store on a new page each time"),
        (7, "integer loop, no memory access"),
    ] {
        let mut ns = Vec::new();
        for pool in [None, Some(&untracked), Some(&tracked)] {
            let mut store = new_store(pool);
            let instance = linker
                .instantiate(&mut store, engine, module.clone())
                .unwrap();
            // warm up, then take the best of five
            instance
                .execute(
                    &mut store,
                    &[Value::I32(op), Value::I32(n / 10)],
                    &mut [],
                )
                .unwrap();
            let mut best = Duration::MAX;
            for _ in 0..5 {
                let t0 = Instant::now();
                instance
                    .execute(&mut store, &[Value::I32(op), Value::I32(n)], &mut [])
                    .unwrap();
                best = best.min(t0.elapsed());
            }
            ns.push(best.as_secs_f64() * 1e9 / n as f64);
        }
        println!(
            "| {} | {:.2} | {:.2} | {:.2} | {:+.1}% |",
            name,
            ns[0],
            ns[1],
            ns[2],
            (ns[2] / ns[0] - 1.0) * 100.0
        );
    }
}

fn main() {
    let quick = std::env::args().any(|arg| arg == "--quick");
    println!(
        "# memory pool measurements\n\nos={} arch={} host_page={} thp={} quick={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        host_page_size(),
        thp_setting(),
        quick
    );
    section_slot_reset(quick);
    section_vm(quick);
    section_tracking(quick);
}

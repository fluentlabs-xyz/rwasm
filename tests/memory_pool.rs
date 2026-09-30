//! Fresh-instance equivalence of pooled linear memory (FLU-1501 research prototype).
//!
//! A store whose memory comes from a [`MemoryPool`] must behave exactly like a store with a `Vec`
//! memory, and a slot that served an arbitrary earlier execution (sparse and dense writes, growth,
//! bulk operations, segment drops, a trap, an `OutOfFuel`, an abandoned or resumed interruption)
//! must serve the next instance as if it had just been mapped: same post-instantiation image, same
//! results, same fuel, same memory, tables and globals afterwards.
#![cfg(all(feature = "memory-pool", unix))]

use rwasm::{
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, MemoryPool, MemoryPoolConfig,
    ResetPolicy, RwasmInstance, RwasmModule, RwasmStore, StoreTr, SyscallFuelParams, TrapCode,
    TypedCaller, ValType, Value,
};
use std::sync::Arc;

/// A contract-shaped module: a 1 MiB shadow stack below the data, like every Fluentbase contract,
/// a mutable stack-pointer global, tables and passive segments. `main(op, arg)` runs one of the
/// workloads the pool has to clean up after.
const CONTRACT: &str = r#"(module
  (import "env" "host" (func $host (param i32) (result i32)))
  (memory 17)
  (global $sp (mut i32) (i32.const 1048576))
  (global $g (mut i64) (i64.const 5))
  (table 4 funcref)
  (elem (i32.const 0) $f0 $f1)
  (elem $passive_elem funcref (ref.func $f1))
  (data (i32.const 1048576) "\11\22\33\44initial-data")
  (data $passive_data "PASSIVE")
  (func $f0 (result i32) i32.const 10)
  (func $f1 (result i32) i32.const 20)
  (func $probe (param $addr i32) (result i32)
    ;; the byte at addr plus 16 * the byte 8 KiB above it, to catch page-sized slips
    (i32.add (i32.load8_u (local.get $addr))
             (i32.mul (i32.const 16) (i32.load8_u (i32.add (local.get $addr) (i32.const 8192))))))
  (func (export "main") (param $op i32) (param $arg i32) (result i32)
    ;; 0: sparse writes of `arg` at four distant addresses, returns their sum
    (if (i32.eq (local.get $op) (i32.const 0)) (then
      (i32.store8 (i32.const 0) (local.get $arg))
      (i32.store8 (i32.const 65537) (local.get $arg))
      (i32.store8 (i32.const 524288) (local.get $arg))
      (i32.store8 (i32.const 1048580) (local.get $arg))
      (return (i32.add (i32.add (i32.load8_u (i32.const 0)) (i32.load8_u (i32.const 65537)))
                       (i32.add (i32.load8_u (i32.const 524288)) (i32.load8_u (i32.const 1048580)))))))
    ;; 1: grow by `arg` pages and write the last byte of the new memory, returns the old size
    (if (i32.eq (local.get $op) (i32.const 1)) (then
      (local.set $arg (memory.grow (local.get $arg)))
      (if (i32.ne (local.get $arg) (i32.const -1)) (then
        (i32.store8 (i32.sub (i32.mul (memory.size) (i32.const 65536)) (i32.const 1)) (i32.const 0x77))))
      (return (local.get $arg))))
    ;; 2: bulk operations and segment drops, returns a checksum
    (if (i32.eq (local.get $op) (i32.const 2)) (then
      (memory.fill (i32.const 131072) (i32.const 0x5a) (i32.const 65536))
      (memory.copy (i32.const 262144) (i32.const 131072) (i32.const 4096))
      (memory.init $passive_data (i32.const 8192) (i32.const 0) (i32.const 7))
      (data.drop $passive_data)
      (table.init $passive_elem (i32.const 2) (i32.const 0) (i32.const 1))
      (elem.drop $passive_elem)
      (table.set (i32.const 3) (ref.func $f0))
      (global.set $g (i64.const 77))
      (return (i32.add (i32.load8_u (i32.const 262145)) (i32.load8_u (i32.const 8193))))))
    ;; 3: write, then trap
    (if (i32.eq (local.get $op) (i32.const 3)) (then
      (i32.store (i32.const 4096) (i32.const 0x2222_2222))
      (global.set $sp (i32.sub (global.get $sp) (i32.const 64)))
      (unreachable)))
    ;; 4: write every page of the memory until the fuel runs out
    (if (i32.eq (local.get $op) (i32.const 4)) (then
      (local.set $arg (i32.const 0))
      (loop $forever
        (i32.store (i32.and (local.get $arg) (i32.const 1048572)) (local.get $arg))
        (local.set $arg (i32.add (local.get $arg) (i32.const 4093)))
        (br $forever))))
    ;; 5: write, ask the host (an interruption), write what it answered
    (if (i32.eq (local.get $op) (i32.const 5)) (then
      (i32.store (i32.const 12288) (i32.const 0x3333_3333))
      (local.set $arg (call $host (local.get $arg)))
      (i32.store (i32.const 16384) (local.get $arg))
      (return (i32.load (i32.const 16384)))))
    ;; 6: probe the bytes around `arg`
    (if (i32.eq (local.get $op) (i32.const 6)) (then
      (return (call $probe (local.get $arg)))))
    ;; 7: read the state the earlier ops changed
    (i32.add (i32.wrap_i64 (global.get $g))
      (i32.add (global.get $sp)
        (i32.add (i32.load8_u (i32.const 1048576))
          (i32.add (call_indirect (type $ret) (i32.const 0))
            (memory.size)))))
  )
  (type $ret (func (result i32)))
)"#;

/// Whether the host answers a call inline or interrupts on it; and its answer.
#[derive(Default, Clone)]
struct Host {
    interrupt: bool,
    answer: i32,
}

fn host(
    caller: &mut TypedCaller<'_, Host>,
    _: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    if caller.data().interrupt {
        return Err(TrapCode::InterruptionCalled);
    }
    // an inline answer also writes into the guest memory, as Fluentbase syscalls do
    let arg = params[0].i32().unwrap();
    caller.memory_write(20480, &arg.to_le_bytes())?;
    result[0] = Value::I32(caller.data().answer);
    Ok(())
}

fn linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "host"),
        1,
        SyscallFuelParams::None,
        &[ValType::I32],
        &[ValType::I32],
    );
    Arc::new(linker)
}

fn compile(linker: &Arc<ImportLinker>) -> RwasmModule {
    RwasmModule::compile(
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_import_linker(linker.clone()),
        &wat::parse_str(CONTRACT).unwrap(),
    )
    .unwrap()
    .0
}

const FUEL: u64 = 1_000_000;
const MAX_PAGES: u32 = 64;

fn vec_store(linker: &Arc<ImportLinker>, interrupt: bool) -> RwasmStore<Host> {
    RwasmStore::new(
        linker.clone(),
        Host {
            interrupt,
            answer: 41,
        },
        host,
        Some(FUEL),
        Some(MAX_PAGES),
    )
}

fn pooled_store(
    linker: &Arc<ImportLinker>,
    pool: &MemoryPool,
    interrupt: bool,
) -> RwasmStore<Host> {
    vec_store(linker, interrupt).with_memory_pool(pool.clone())
}

fn pool(reset_policy: ResetPolicy, track_dirty: bool) -> MemoryPool {
    MemoryPool::new(MemoryPoolConfig {
        slot_pages: MAX_PAGES,
        max_free_slots: 4,
        reset_policy,
        track_dirty,
        verify_reset: true,
        no_huge_pages: true,
    })
}

const POLICIES: [ResetPolicy; 6] = [
    ResetPolicy::Discard,
    ResetPolicy::Remap,
    ResetPolicy::DiscardDirty,
    ResetPolicy::RemapDirty,
    ResetPolicy::Memset,
    ResetPolicy::Adaptive {
        memset_up_to_pages: 4,
    },
];

/// Runs `main(op, arg)`, driving every interruption to completion with the host's answer written
/// into the guest memory first, as a Fluentbase syscall would.
fn call(
    store: &mut RwasmStore<Host>,
    instance: &RwasmInstance,
    op: i32,
    arg: i32,
) -> Result<i32, TrapCode> {
    let mut result = [Value::I32(0)];
    let mut outcome = instance.execute(store, &[Value::I32(op), Value::I32(arg)], &mut result);
    while outcome == Err(TrapCode::InterruptionCalled) {
        store.memory_write(20480, &arg.to_le_bytes())?;
        let answer = store.data().answer;
        outcome = instance.resume(store, &[Value::I32(answer)], &mut result);
    }
    outcome.map(|()| result[0].i32().unwrap())
}

/// Everything the guest and the host can observe of an instance.
#[derive(PartialEq, Eq)]
struct Observation {
    outcome: Result<i32, TrapCode>,
    fuel_consumed: u64,
    memory: Vec<u8>,
    tables: Vec<(u32, u32, Vec<u8>)>,
    globals: Vec<u32>,
}

/// Prints the memory as a size and a hash, not as a megabyte of bytes.
impl std::fmt::Debug for Observation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = self
            .memory
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            });
        f.debug_struct("Observation")
            .field("outcome", &self.outcome)
            .field("fuel_consumed", &self.fuel_consumed)
            .field(
                "memory",
                &format_args!("{} bytes, fnv {hash:016x}", self.memory.len()),
            )
            .field("tables", &self.tables)
            .field("globals", &self.globals)
            .finish()
    }
}

/// Like `assert_eq!`, but names the first differing memory byte instead of dumping the memory.
fn assert_same(actual: &Observation, expected: &Observation, context: &str) {
    if let Some(index) = (0..actual.memory.len().max(expected.memory.len()))
        .find(|&i| actual.memory.get(i) != expected.memory.get(i))
    {
        panic!(
            "{context}: memory differs at byte {index}: recycled {:?}, fresh {:?} (sizes {} vs {})",
            actual.memory.get(index),
            expected.memory.get(index),
            actual.memory.len(),
            expected.memory.len()
        );
    }
    assert_eq!(actual, expected, "{context}");
}

fn observe(
    store: &mut RwasmStore<Host>,
    instance: &RwasmInstance,
    op: i32,
    arg: i32,
) -> Observation {
    let outcome = call(store, instance, op, arg);
    Observation {
        outcome,
        fuel_consumed: store.fuel_consumed(),
        memory: store.memory_snapshot(),
        tables: store.table_snapshots_nullness_prefix(8),
        globals: (0..6).map(|word| store.global_word_bits(word)).collect(),
    }
}

/// The prior executions a recycled slot has to hide.
#[derive(Debug, Clone, Copy)]
enum Prior {
    Sparse,
    Grow,
    Bulk,
    Trap,
    OutOfFuel,
    InterruptAbandoned,
    InterruptResumed,
    DenseThenTrap,
}

const PRIORS: [Prior; 8] = [
    Prior::Sparse,
    Prior::Grow,
    Prior::Bulk,
    Prior::Trap,
    Prior::OutOfFuel,
    Prior::InterruptAbandoned,
    Prior::InterruptResumed,
    Prior::DenseThenTrap,
];

/// Runs the prior execution on a pooled store and drops it, returning the slot to the pool.
fn dirty_the_pool(
    linker: &Arc<ImportLinker>,
    module: &RwasmModule,
    pool: &MemoryPool,
    prior: Prior,
) {
    let engine = ExecutionEngine::new();
    let interrupt = matches!(prior, Prior::InterruptAbandoned | Prior::InterruptResumed);
    let mut store = pooled_store(linker, pool, interrupt);
    let instance = linker
        .instantiate(&mut store, engine, module.clone())
        .unwrap();
    match prior {
        Prior::Sparse => assert_eq!(call(&mut store, &instance, 0, 9), Ok(36)),
        Prior::Grow => {
            assert_eq!(call(&mut store, &instance, 1, 40), Ok(17));
            assert_eq!(call(&mut store, &instance, 0, 1), Ok(4));
        }
        Prior::Bulk => assert_eq!(call(&mut store, &instance, 2, 0), Ok(0x5a + b'A' as i32)),
        Prior::Trap => assert_eq!(
            call(&mut store, &instance, 3, 0),
            Err(TrapCode::UnreachableCodeReached)
        ),
        Prior::OutOfFuel => assert_eq!(call(&mut store, &instance, 4, 0), Err(TrapCode::OutOfFuel)),
        Prior::InterruptAbandoned => {
            let mut result = [Value::I32(0)];
            assert_eq!(
                instance.execute(&mut store, &[Value::I32(5), Value::I32(3)], &mut result),
                Err(TrapCode::InterruptionCalled)
            );
            // the host writes while the execution is parked, then gives up on the frame
            store.memory_write(24576, b"abandoned").unwrap();
        }
        Prior::InterruptResumed => assert_eq!(call(&mut store, &instance, 5, 3), Ok(41)),
        Prior::DenseThenTrap => {
            assert_eq!(call(&mut store, &instance, 1, 47), Ok(17));
            assert_eq!(call(&mut store, &instance, 4, 0), Err(TrapCode::OutOfFuel));
            store.reset_fuel(FUEL);
            assert_eq!(
                call(&mut store, &instance, 3, 0),
                Err(TrapCode::UnreachableCodeReached)
            );
        }
    }
    assert!(
        store
            .memory_dirty_host_pages()
            .is_some_and(|pages| pages > 0)
            || !pool.config().track_dirty
    );
    drop(instance);
    drop(store);
}

/// The probe sequence run on both the recycled and the fresh instance.
fn probe_sequence() -> Vec<(i32, i32)> {
    vec![
        (7, 0),
        (6, 0),
        (6, 4096),
        (6, 12288),
        (6, 16384),
        (6, 20480),
        (6, 24576),
        (6, 131072),
        (6, 262144),
        (6, 1048576),
        (0, 5),
        (2, 0),
        (1, 3),
        (6, 1114111 - 8192),
        (5, 8),
        (7, 0),
        (3, 0),
        (7, 0),
    ]
}

#[test]
fn recycled_slot_is_indistinguishable_from_a_fresh_instance() {
    let linker = linker();
    let module = compile(&linker);
    let engine = ExecutionEngine::new();
    for policy in POLICIES {
        for track_dirty in [true, false] {
            for prior in PRIORS {
                let pool = pool(policy, track_dirty);
                dirty_the_pool(&linker, &module, &pool, prior);
                let context = format!("{policy:?} track_dirty={track_dirty} after {prior:?}");
                assert_eq!(
                    pool.stats()
                        .reset_failures
                        .load(std::sync::atomic::Ordering::Relaxed),
                    0,
                    "{context}"
                );
                assert_eq!(pool.free_slots(), 1, "{context}");

                let mut recycled = pooled_store(&linker, &pool, false);
                let mut fresh = vec_store(&linker, false);
                // the recycled store took the slot back: nothing new was mapped
                assert_eq!(
                    pool.stats()
                        .slots_reserved
                        .load(std::sync::atomic::Ordering::Relaxed),
                    1,
                    "{context}"
                );
                let recycled_instance = linker
                    .instantiate(&mut recycled, engine, module.clone())
                    .unwrap();
                let fresh_instance = linker
                    .instantiate(&mut fresh, engine, module.clone())
                    .unwrap();
                assert_eq!(
                    recycled.memory_snapshot(),
                    fresh.memory_snapshot(),
                    "{context}: image"
                );
                assert_eq!(
                    recycled.fuel_consumed(),
                    fresh.fuel_consumed(),
                    "{context}: prologue fuel"
                );
                for (op, arg) in probe_sequence() {
                    let expected = observe(&mut fresh, &fresh_instance, op, arg);
                    let actual = observe(&mut recycled, &recycled_instance, op, arg);
                    assert_same(&actual, &expected, &format!("{context}: main({op}, {arg})"));
                }
            }
        }
    }
}

#[test]
fn slot_serves_the_post_instantiation_image_without_growing_memory_by_hand() {
    let linker = linker();
    let module = compile(&linker);
    let pool = pool(ResetPolicy::Discard, true);
    let mut store = pooled_store(&linker, &pool, false);
    let instance = linker
        .instantiate(&mut store, ExecutionEngine::new(), module)
        .unwrap();
    assert_eq!(store.memory_size_bytes(), 17 * 65536);
    let image = store.memory_snapshot();
    assert_eq!(&image[1048576..1048576 + 4], &[0x11, 0x22, 0x33, 0x44]);
    assert!(image[..1048576].iter().all(|byte| *byte == 0));
    // only the data segment's page(s) and nothing else were written by the prologue
    let host_page = rwasm::host_page_size();
    let data_pages = 16_usize.div_ceil(host_page);
    assert_eq!(store.memory_dirty_host_pages(), Some(data_pages));
    // three of the four sparse writes land on new pages; the fourth shares the data segment's
    assert_eq!(call(&mut store, &instance, 0, 1), Ok(4));
    assert_eq!(store.memory_dirty_host_pages(), Some(data_pages + 3));
}

#[test]
fn a_slot_too_small_for_the_module_fails_instantiation_like_a_small_store() {
    let linker = linker();
    let module = compile(&linker);
    let pool = MemoryPool::new(MemoryPoolConfig {
        slot_pages: 4,
        verify_reset: true,
        ..MemoryPoolConfig::default()
    });
    let mut pooled = pooled_store(&linker, &pool, false);
    let mut small = RwasmStore::new(linker.clone(), Host::default(), host, Some(FUEL), Some(4));
    let engine = ExecutionEngine::new();
    let expected = linker.instantiate(&mut small, engine, module.clone()).err();
    assert_eq!(expected, Some(TrapCode::MemoryOutOfBounds));
    assert_eq!(
        linker.instantiate(&mut pooled, engine, module).err(),
        expected
    );
    // the failed instantiation's slot went back through a reset, and the rolled-back store
    // holds no slot
    assert_eq!(
        pool.stats()
            .leases
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(pool.free_slots(), 1);
    assert_eq!(
        pool.stats()
            .reset_failures
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    drop(pooled);
    assert_eq!(pool.free_slots(), 1);
}

#[test]
fn replacing_the_instance_returns_the_previous_slot() {
    let linker = linker();
    let module = compile(&linker);
    let pool = pool(
        ResetPolicy::Adaptive {
            memset_up_to_pages: 4,
        },
        true,
    );
    let engine = ExecutionEngine::new();
    let mut store = pooled_store(&linker, &pool, false);
    let first = linker
        .instantiate(&mut store, engine, module.clone())
        .unwrap();
    assert_eq!(call(&mut store, &first, 0, 2), Ok(8));
    assert_eq!(pool.free_slots(), 0);
    // the replacement leases a second slot and returns the first one, reset
    let second = linker
        .instantiate(&mut store, engine, module.clone())
        .unwrap();
    assert_eq!(
        pool.stats()
            .slots_reserved
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    assert_eq!(pool.free_slots(), 1);
    assert_eq!(
        first.execute(
            &mut store,
            &[Value::I32(7), Value::I32(0)],
            &mut [Value::I32(0)]
        ),
        Err(TrapCode::IllegalOpcode)
    );
    let mut fresh = vec_store(&linker, false);
    let fresh_instance = linker.instantiate(&mut fresh, engine, module).unwrap();
    // the store's fuel counter is cumulative across instances; compare from the same point
    store.reset_fuel(FUEL);
    fresh.reset_fuel(FUEL);
    for (op, arg) in probe_sequence() {
        let expected = observe(&mut fresh, &fresh_instance, op, arg);
        let actual = observe(&mut store, &second, op, arg);
        assert_same(&actual, &expected, &format!("main({op}, {arg})"));
    }
}

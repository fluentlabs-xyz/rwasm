//! Reproductions for the 2026-09-14 round-7 audit.
//!
//! `R7-1`: after a memory access that Cranelift can prove out of bounds at compile time — the
//! access's immediate `offset` plus its size exceeds the memory's declared maximum, or the memory
//! declares a maximum of zero pages — the Wasmtime strategy charges the region only up to and
//! including that access, while rwasm charges the whole region on entry (its documented model,
//! which Wasmtime otherwise follows: a *dynamic* out-of-bounds access, a division trap or an
//! `unreachable` leave the same counter on both). Both strategies trap `MemoryOutOfBounds`, but
//! they disagree on the remaining fuel by the cost of everything after the access in the region.
//! Written against the correct behaviour, so the tests fail until fixed.
#![cfg(feature = "wasmtime")]

use rwasm::{
    for_each_strategy, CompilationConfig, ImportLinker, StoreTr, StrategyError, TrapCode, Value,
};
use std::sync::Arc;

/// Runs `main` on both strategies with a 1000 fuel budget: `(trap, remaining fuel)` per strategy.
fn both(wat: &str) -> Vec<(Option<TrapCode>, Option<u64>)> {
    let wasm = wat::parse_str(wat).expect("the test module parses");
    for_each_strategy(
        |strategy| -> Result<_, StrategyError> {
            let mut executor = strategy.create_executor(
                Arc::new(ImportLinker::default()),
                (),
                rwasm::always_failing_syscall_handler,
                Some(1_000),
                None,
            )?;
            let trap = executor.execute("main", &[], &mut []).err();
            Ok((trap, executor.remaining_fuel()))
        },
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true),
        &wasm,
    )
    .expect("both strategies compile the module")
}

fn assert_aligned(label: &str, wat: &str) {
    let outcomes = both(wat);
    assert_eq!(
        outcomes[0].0,
        Some(TrapCode::MemoryOutOfBounds),
        "{label}: rwasm traps"
    );
    assert_eq!(
        outcomes[0], outcomes[1],
        "{label}: (trap, remaining fuel) must agree: rwasm={:?} wasmtime={:?}",
        outcomes[0], outcomes[1]
    );
}

/// Control: a *dynamically* out-of-bounds load (offset within the maximum, address past the
/// current size) charges the whole region on both strategies.
#[test]
fn dynamic_out_of_bounds_load_charges_the_whole_region_on_both() {
    assert_aligned(
        "dynamic",
        r#"(module
          (memory (export "memory") 1 2)
          (func (export "main")
            (drop (i32.load offset=131068 (i32.const 0)))
            (drop (i32.const 1)) (drop (i32.const 1))
            unreachable))"#,
    );
}

/// A load whose immediate offset lies beyond the declared maximum: statically out of bounds.
#[test]
fn statically_out_of_bounds_load_charges_the_whole_region_on_both() {
    assert_aligned(
        "static offset",
        r#"(module
          (memory (export "memory") 1 2)
          (func (export "main")
            (drop (i32.load offset=131072 (i32.const 0)))
            (drop (i32.const 1)) (drop (i32.const 1))
            unreachable))"#,
    );
}

/// The degenerate form: a memory that can never hold a page makes every access static.
#[test]
fn access_to_a_zero_maximum_memory_charges_the_whole_region_on_both() {
    assert_aligned(
        "max 0",
        r#"(module
          (memory (export "memory") 0 0)
          (func (export "main")
            (drop (i32.load (i32.const 0)))
            (drop (i32.const 1)) (drop (i32.const 1))
            unreachable))"#,
    );
}

/// Stores behave the same as loads.
#[test]
fn statically_out_of_bounds_store_charges_the_whole_region_on_both() {
    assert_aligned(
        "static store",
        r#"(module
          (memory (export "memory") 0 1)
          (func (export "main")
            (i32.store offset=65536 (i32.const 0) (i32.const 0))
            (drop (i32.const 1)) (drop (i32.const 1))
            unreachable))"#,
    );
}

/// The gap scales with the region: everything after the access is uncharged on Wasmtime.
#[test]
fn undercharge_grows_with_the_region() {
    let mut tail = String::new();
    for _ in 0..500 {
        tail.push_str("(drop (i32.const 1)) ");
    }
    let wat = format!(
        r#"(module
          (memory (export "memory") 1 1)
          (func (export "main")
            (drop (i32.load offset=65536 (i32.const 0)))
            {tail}
            unreachable))"#
    );
    let outcomes = both(&wat);
    assert_eq!(outcomes[0].0, Some(TrapCode::MemoryOutOfBounds));
    let _ = Value::I32(0);
    assert_eq!(
        outcomes[0], outcomes[1],
        "500 charged operators after a static trap: rwasm={:?} wasmtime={:?}",
        outcomes[0], outcomes[1]
    );
}

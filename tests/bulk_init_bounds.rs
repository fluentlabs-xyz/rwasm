//! Differential coverage for the bulk-segment bound checks injected into `memory.init` and
//! `table.init`.
//!
//! The injected guard must compare the original source offset and length against the segment's
//! length with unsigned, non-wrapping arithmetic, and must leave `src == len == 0` untouched.
//! Both backends have to agree on every case below: a wrapped source index
//! used to read another segment's bytes on rwasm while wasmtime trapped, and a zero-length init
//! on a dropped segment used to trap on rwasm while wasmtime accepted it.

#![cfg(feature = "wasmtime")]

use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ImportLinker, RwasmModule, StoreTr,
    StrategyDefinition, TrapCode, Value, N_MAX_DATA_SEGMENTS, N_MAX_ELEM_SEGMENTS,
};
use std::sync::Arc;

fn config() -> CompilationConfig {
    CompilationConfig::default_strategy_compatible().with_entrypoint_name("main".into())
}

/// Runs `main` through one strategy, reporting the trap instead of failing the harness.
fn run(
    definition: StrategyDefinition,
    params: &[Value],
) -> (Result<Option<i32>, TrapCode>, Option<u64>) {
    let mut executor = definition
        .create_executor(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        )
        .expect("the module must instantiate");
    let mut result = [Value::I32(i32::MIN)];
    let outcome = match executor.execute("main", params, &mut result) {
        Ok(()) => Ok(Some(result[0].i32().unwrap_or(i32::MIN))),
        Err(TrapCode::ExecutionHalted) => Ok(None),
        Err(trap_code) => Err(trap_code),
    };
    (outcome, executor.remaining_fuel())
}

/// Runs `main` on both strategies and asserts they agree with each other and with `expected`.
fn assert_strategies_agree(wasm: &[u8], params: &[Value], expected: Result<i32, TrapCode>) {
    let expected = match expected {
        Ok(value) => Ok(Some(value)),
        Err(trap_code) => Err(trap_code),
    };
    let rwasm = StrategyDefinition::new_as_rwasm(config(), wasm).expect("rwasm must compile");
    let wasmtime =
        StrategyDefinition::new_as_wasmtime(config(), wasm, None).expect("wasmtime must compile");

    let (rwasm_outcome, rwasm_fuel) = run(rwasm, params);
    let (wasmtime_outcome, wasmtime_fuel) = run(wasmtime, params);
    assert_eq!(
        rwasm_outcome, wasmtime_outcome,
        "backends disagree for params {params:?}: rwasm={rwasm_outcome:?} wasmtime={wasmtime_outcome:?}"
    );
    assert_eq!(
        rwasm_outcome, expected,
        "unexpected outcome for params {params:?}"
    );
    assert_eq!(rwasm_fuel, wasmtime_fuel, "fuel differs for {params:?}");
}

const MEMORY_INIT: &str = r#"
(module
  (memory (export "memory") 1)
  (data (i32.const 0) "\11")
  (data "\aa\bb\cc\dd")
  (func (export "main") (param i32 i32 i32) (result i32)
    (memory.init 1 (local.get 0) (local.get 1) (local.get 2))
    (i32.load8_u (i32.const 0))))
"#;

const TABLE_INIT: &str = r#"
(module
  (type $t (func (result i32)))
  (table 8 funcref)
  (func $f0 (result i32) (i32.const 100))
  (func $f1 (result i32) (i32.const 200))
  (func $f2 (result i32) (i32.const 300))
  (elem (i32.const 0) $f0)
  (elem func $f1 $f2)
  (func (export "main") (param i32 i32 i32) (result i32)
    (table.init 1 (local.get 0) (local.get 1) (local.get 2))
    (call_indirect (type $t) (i32.const 0))))
"#;

#[test]
fn memory_init_in_range_copies_the_passive_segment() {
    let wasm = wat::parse_str(MEMORY_INIT).unwrap();
    // src=1 points at the second byte of the passive segment (`\bb`), not at the active segment
    // that shares the flattened blob.
    assert_strategies_agree(
        &wasm,
        &[Value::I32(0), Value::I32(1), Value::I32(1)],
        Ok(0xbb),
    );
}

#[test]
fn memory_init_past_the_segment_traps_on_both() {
    let wasm = wat::parse_str(MEMORY_INIT).unwrap();
    assert_strategies_agree(
        &wasm,
        &[Value::I32(0), Value::I32(9), Value::I32(1)],
        Err(TrapCode::MemoryOutOfBounds),
    );
}

/// `src = -1` used to wrap the injected `n + s` check into range and read the *active* segment's
/// byte (`0x11`) on rwasm, while wasmtime trapped.
#[test]
fn memory_init_with_wrapping_source_index_traps_on_both() {
    let wasm = wat::parse_str(MEMORY_INIT).unwrap();
    for src in [-1i32, -2, -3, i32::MIN, i32::MAX] {
        assert_strategies_agree(
            &wasm,
            &[Value::I32(0), Value::I32(src), Value::I32(1)],
            Err(TrapCode::MemoryOutOfBounds),
        );
    }
}

/// A zero-length init at a valid source offset must succeed on both backends.
#[test]
fn memory_init_zero_length_is_accepted_by_both() {
    let wasm = wat::parse_str(MEMORY_INIT).unwrap();
    for (src, len) in [(0i32, 0i32), (4, 0), (0, 4), (4, 0)] {
        assert_strategies_agree(
            &wasm,
            &[Value::I32(0), Value::I32(src), Value::I32(len)],
            Ok(if len == 0 { 0x11 } else { 0xaa }),
        );
    }
}

const MEMORY_INIT_DROPPED: &str = r#"
(module
  (memory (export "memory") 1)
  (data "\11\22")
  (data "\aa\bb\cc\dd")
  (func (export "main") (param i32 i32 i32) (result i32)
    (data.drop 1)
    (memory.init 1 (local.get 0) (local.get 1) (local.get 2))
    (i32.const 7)))
"#;

/// After `data.drop` the segment has length 0, so only `s == 0 && n == 0` survives. That
/// zero-copy case keeps its source offset, which makes the runtime's empty-window check agree
/// with wasmtime without querying whether the segment is live.
#[test]
fn memory_init_on_dropped_segment_follows_the_zero_length_rule() {
    let wasm = wat::parse_str(MEMORY_INIT_DROPPED).unwrap();
    assert_strategies_agree(&wasm, &[Value::I32(0), Value::I32(0), Value::I32(0)], Ok(7));
    for (src, len) in [(1i32, 0i32), (0, 1), (1, 1), (-1, 0)] {
        assert_strategies_agree(
            &wasm,
            &[Value::I32(0), Value::I32(src), Value::I32(len)],
            Err(TrapCode::MemoryOutOfBounds),
        );
    }
}

#[test]
fn table_init_in_range_dispatches_the_passive_segment() {
    let wasm = wat::parse_str(TABLE_INIT).unwrap();
    // src=0 of the passive segment holds `$f1`.
    assert_strategies_agree(
        &wasm,
        &[Value::I32(0), Value::I32(0), Value::I32(1)],
        Ok(200),
    );
}

#[test]
fn table_init_past_the_segment_traps_on_both() {
    let wasm = wat::parse_str(TABLE_INIT).unwrap();
    assert_strategies_agree(
        &wasm,
        &[Value::I32(0), Value::I32(2), Value::I32(1)],
        Err(TrapCode::TableOutOfBounds),
    );
}

/// Same wrap as the data case: `src = -1` used to install the active segment's `$f0`.
#[test]
fn table_init_with_wrapping_source_index_traps_on_both() {
    let wasm = wat::parse_str(TABLE_INIT).unwrap();
    for src in [-1i32, -2, i32::MIN, i32::MAX] {
        assert_strategies_agree(
            &wasm,
            &[Value::I32(0), Value::I32(src), Value::I32(1)],
            Err(TrapCode::TableOutOfBounds),
        );
    }
}

const TABLE_INIT_DROPPED: &str = r#"
(module
  (type $t (func (result i32)))
  (table 8 funcref)
  (func $f0 (result i32) (i32.const 100))
  (func $f1 (result i32) (i32.const 200))
  (elem func $f0)
  (elem func $f1)
  (func (export "main") (param i32 i32 i32) (result i32)
    (elem.drop 1)
    (table.init 1 (local.get 0) (local.get 1) (local.get 2))
    (i32.const 9)))
"#;

#[test]
fn table_init_on_dropped_segment_follows_the_zero_length_rule() {
    let wasm = wat::parse_str(TABLE_INIT_DROPPED).unwrap();
    assert_strategies_agree(&wasm, &[Value::I32(0), Value::I32(0), Value::I32(0)], Ok(9));
    for (src, len) in [(1i32, 0i32), (0, 1), (1, 1), (-1, 0)] {
        assert_strategies_agree(
            &wasm,
            &[Value::I32(0), Value::I32(src), Value::I32(len)],
            Err(TrapCode::TableOutOfBounds),
        );
    }
}

/// A dropped segment whose blob offset is not zero used to be rejected even for the valid
/// zero-length init, because the prologue added the blob offset unconditionally. `MEMORY_INIT_DROPPED`
/// has two segments, so the second one's blob offset is not zero.
#[test]
fn dropped_segment_with_nonzero_blob_offset_keeps_its_source_offset() {
    let wasm = wat::parse_str(MEMORY_INIT_DROPPED).unwrap();
    assert_strategies_agree(&wasm, &[Value::I32(0), Value::I32(0), Value::I32(0)], Ok(7));
}

/// Empty copies must still check the destination, and skipping the rewrite for `len == 0` alone
/// must not accept a nonzero source after a drop. Exercise both live and dropped segments with a
/// nonzero offset in the flattened blob, including unsigned source/length boundaries.
#[test]
fn bulk_init_source_and_destination_boundary_matrix() {
    for (template, drop_instruction, destination_size, segment_len, value, trap) in [
        (
            MEMORY_INIT_DROPPED,
            "(data.drop 1)",
            65536,
            4,
            7,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            TABLE_INIT_DROPPED,
            "(elem.drop 1)",
            8,
            1,
            9,
            TrapCode::TableOutOfBounds,
        ),
    ] {
        for dropped in [false, true] {
            let wasm = wat::parse_str(if dropped {
                template.to_owned()
            } else {
                template.replace(drop_instruction, "")
            })
            .unwrap();
            let (module, _) = RwasmModule::compile(config(), &wasm).unwrap();
            // All these integer-only fixtures must stay within the pre-audit opcode inventory.
            assert!(module.code_section.iter().all(|op| op.code() < 90));
            for dst in [0, destination_size, destination_size + 1, -1] {
                for src in [0, 1, segment_len, segment_len + 1, -1, i32::MAX, i32::MIN] {
                    for len in [0, 1, segment_len, -1] {
                        let source_size = if dropped { 0 } else { segment_len } as u64;
                        let length = len as u32 as u64;
                        let valid = src as u32 as u64 + length <= source_size
                            && dst as u32 as u64 + length <= destination_size as u64;
                        assert_strategies_agree(
                            &wasm,
                            &[Value::I32(dst), Value::I32(src), Value::I32(len)],
                            if valid { Ok(value) } else { Err(trap) },
                        );
                    }
                }
            }
        }
    }
}

/// The empty-copy branch must land before any dynamic fuel charge and the init instruction.
/// Reset fuel after instantiation to isolate the copy's charge from initial memory allocation.
#[test]
fn bulk_init_offset_branch_preserves_dynamic_fuel() {
    for (template, drop_instruction) in [
        (MEMORY_INIT_DROPPED, "(data.drop 1)"),
        (TABLE_INIT_DROPPED, "(elem.drop 1)"),
    ] {
        let wasm = wat::parse_str(template.replace(drop_instruction, "")).unwrap();
        for len in [0, 1] {
            let mut remaining = Vec::new();
            for charge_bulk in [false, true] {
                let definition = StrategyDefinition::new_as_rwasm(
                    config().with_consume_fuel_for_bulk_ops(charge_bulk),
                    &wasm,
                )
                .unwrap();
                let mut executor = definition.default_executor().unwrap();
                executor.reset_fuel(1000);
                executor
                    .execute(
                        "main",
                        &[Value::I32(0), Value::I32(0), Value::I32(len)],
                        &mut [Value::I32(0)],
                    )
                    .unwrap();
                remaining.push(executor.remaining_fuel().unwrap());
            }
            // An empty copy charges zero dynamic fuel; one byte/element rounds up to one unit.
            assert_eq!(remaining[0] - remaining[1], len as u64);
        }
    }
}

/// The compiler reserves segment index zero and shifts every Wasm segment index by one. The
/// runtime limit must allow the translated index of the last legal segment for both kinds.
#[test]
fn highest_valid_segment_can_be_dropped_and_initialized_empty() {
    let memory = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1) {}
            (func (export "main") (result i32)
                data.drop {last}
                data.drop {last}
                (memory.init {last} (i32.const 0) (i32.const 0) (i32.const 0))
                i32.const 42))"#,
        "(data \"\")".repeat(N_MAX_DATA_SEGMENTS),
        last = N_MAX_DATA_SEGMENTS - 1,
    ))
    .unwrap();
    assert_strategies_agree(&memory, &[], Ok(42));

    let table = wat::parse_str(format!(
        r#"(module (table 1 funcref) {}
            (func (export "main") (result i32)
                elem.drop {last}
                elem.drop {last}
                (table.init {last} (i32.const 0) (i32.const 0) (i32.const 0))
                i32.const 42))"#,
        "(elem func)".repeat(N_MAX_ELEM_SEGMENTS),
        last = N_MAX_ELEM_SEGMENTS - 1,
    ))
    .unwrap();
    assert_strategies_agree(&table, &[], Ok(42));
}

//! Reproductions for the 2026-09-13 round-9 audit (revision v0.6.0, `84cb1740e`).
//!
//! `R9-1`: the host result/parameter buffer is validated by its *total slot count* against the
//! operand stack (`src/vm/executor.rs::run_raw`), not by the value count and types the entrypoint
//! declares. A buffer whose slots line up but whose value partition differs is accepted by rwasm
//! and its values are reinterpreted from the raw slots, while the Wasmtime backend rejects the same
//! call with `IllegalOpcode` (`src/wasmtime/instance.rs` compares `result.len()` with the declared
//! result count). Written against the correct behaviour: both strategies must agree, so the test
//! fails until the rwasm side validates the declared value signature.
#![cfg(feature = "wasmtime")]

use rwasm::{
    CompilationConfig, ImportLinker, StrategyDefinition, TrapCode, TypedCaller, Value,
};
use std::sync::Arc;

fn handler(_: &mut TypedCaller<'_, ()>, _: u32, _: &[Value], _: &mut [Value]) -> Result<(), TrapCode> {
    Ok(())
}

fn run(def: Result<StrategyDefinition, rwasm::CompilationError>, linker: &Arc<ImportLinker>, params: &[Value], results: &mut [Value]) -> String {
    match def {
        Err(e) => format!("compile:{e:?}"),
        Ok(d) => match d.create_executor(linker.clone(), (), handler, Some(1_000_000), None) {
            Err(trap) => format!("instantiate:{trap:?}"),
            Ok(mut ex) => {
                let before = results.to_vec();
                match ex.execute("main", params, results) {
                    Ok(()) => format!("Ok {:?}", results),
                    Err(trap) => format!("trap:{trap:?} buf={before:?}"),
                }
            }
        },
    }
}

/// (label, module, params, result buffer)
type Row = (&'static str, &'static str, Vec<Value>, Vec<Value>);

fn rows() -> Vec<Row> {
    vec![
        // correct partitions (controls)
        (
            "2 x i32 results, [i32, i32]",
            r#"(module (func (export "main") (result i32 i32) (i32.const 0x1122) (i32.const 0x3344)))"#,
            vec![],
            vec![Value::I32(0), Value::I32(0)],
        ),
        (
            "i64 result, [i64]",
            r#"(module (func (export "main") (result i64) (i64.const 0x1122334455667788)))"#,
            vec![],
            vec![Value::I64(0)],
        ),
        // slot count matches, value partition differs
        (
            "2 x i32 results, [i64] (1 value, 2 slots)",
            r#"(module (func (export "main") (result i32 i32) (i32.const 0x1122) (i32.const 0x3344)))"#,
            vec![],
            vec![Value::I64(0)],
        ),
        (
            "i64 result, [i32, i32] (2 values, 2 slots)",
            r#"(module (func (export "main") (result i64) (i64.const 0x1122334455667788)))"#,
            vec![],
            vec![Value::I32(0), Value::I32(0)],
        ),
        (
            "i32,i64,f64 results, [i32 x5] (5 values, 5 slots)",
            r#"(module (func (export "main") (result i32 i64 f64) (i32.const 1) (i64.const 2) (f64.const 3)))"#,
            vec![],
            vec![Value::I32(0); 5],
        ),
        // parameter side
        (
            "(i32, i32) params, [i64] param (1 value, 2 slots)",
            r#"(module (func (export "main") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#,
            vec![Value::I64(0x0000_0003_0000_0002)],
            vec![Value::I32(0)],
        ),
        (
            "(i64) param, [i32, i32] params (2 values, 2 slots)",
            r#"(module (func (export "main") (param i64) (result i32) (i32.wrap_i64 (local.get 0))))"#,
            vec![Value::I32(0x5678), Value::I32(0x1234)],
            vec![Value::I32(0)],
        ),
    ]
}

#[test]
fn host_buffer_partition_mismatches_are_reported_on_both_strategies() {
    // Collect every diverging row instead of failing on the first, so one run shows the whole
    // surface of the bug.
    let mut mismatches = Vec::new();
    for (label, wat, params, results) in rows() {
        let wasm = wat::parse_str(wat).unwrap();
        let linker = Arc::new(ImportLinker::default());
        let config = CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(linker.clone());
        let mut rwasm_buf = results.clone();
        let rwasm = run(
            StrategyDefinition::new_as_rwasm(config.clone(), &wasm),
            &linker,
            &params,
            &mut rwasm_buf,
        );
        let mut wasmtime_buf = results.clone();
        let wasmtime = run(
            StrategyDefinition::new_as_wasmtime(config, &wasm, None),
            &linker,
            &params,
            &mut wasmtime_buf,
        );
        if rwasm != wasmtime {
            mismatches.push(format!(
                "{label}:\n  rwasm    = {rwasm}\n  wasmtime = {wasmtime}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} rows diverge: a host buffer whose slot count matches but whose value partition \
         does not must be reported like the Wasmtime backend (IllegalOpcode), not answered with \
         reinterpreted values\n{}",
        mismatches.len(),
        rows().len(),
        mismatches.join("\n")
    );
}

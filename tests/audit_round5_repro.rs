//! Reproductions for the 2026-09-13 round-5 audit.
//!
//! Both findings live in the import trampoline, the one piece of generated code the differential
//! fuzzer never exercises (`max_imports = 0`). They are written against the correct behaviour and
//! fail until fixed:
//!
//! * `R5-1`: the syscall fuel of an import (`SyscallFuelParams`) is charged by rwasm inside the
//!   import trampoline, so every way of reaching the import pays it. The Wasmtime strategy charges
//!   it at Cranelift `call`/`return_call` sites only, so `call_indirect`, `return_call_indirect`,
//!   an import exported as the entrypoint and an import used as `start` all run the builtin for
//!   free there.
//! * `R5-2`: the fuel prologue `compile_block_params` emits into the trampoline pushes up to two
//!   (`LinearFuel`) or four (`QuadraticFuel`) temporaries that are never accounted in the
//!   translator's stack height, so the trampoline's `StackCheck` is `0`. When the value stack is
//!   within that many slots of its capacity at the call, rwasm traps `StackOverflow` on a module
//!   Wasmtime executes.
#![cfg(feature = "wasmtime")]

use rwasm::{
    CompilationConfig, ImportLinker, ImportName, StoreTr, StrategyDefinition, StrategyExecutor,
    SyscallFuelParams, TrapCode, TypedCaller, ValType, Value,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
use std::sync::Arc;

const CONST_FUEL: u64 = 1000;

fn accept(
    _: &mut TypedCaller<'_, ()>,
    _: u32,
    _: &[Value],
    _: &mut [Value],
) -> Result<(), TrapCode> {
    Ok(())
}

fn linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "flat"),
        0x11,
        SyscallFuelParams::Const(CONST_FUEL),
        &[],
        &[],
    );
    linker.insert_function(
        ImportName::new("env", "lin"),
        0x12,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            param_index: 1,
            word_cost: 3,
            base_fuel: 7,
        }),
        &[ValType::I32],
        &[],
    );
    linker.insert_function(
        ImportName::new("env", "quad"),
        0x13,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 3,
            divisor: 512,
            fuel_denom_rate: 1,
        }),
        &[ValType::I32],
        &[],
    );
    Arc::new(linker)
}

fn config(allow_start: bool) -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_allow_start_section(allow_start)
        .with_builtins_consume_fuel(true)
        .with_import_linker(linker())
}

/// Returns `(rwasm, wasmtime)` executors for `wat`.
fn executors(wat: &str, fuel: u64, allow_start: bool) -> [StrategyExecutor<()>; 2] {
    let wasm = wat::parse_str(wat).expect("the test module parses");
    let config = config(allow_start);
    let rwasm = StrategyDefinition::new_as_rwasm(config.clone(), &wasm)
        .expect("rwasm compiles the module")
        .create_executor(linker(), (), accept, Some(fuel), None)
        .expect("rwasm instantiates the module");
    let wasmtime = StrategyDefinition::new_as_wasmtime(config, &wasm, None)
        .expect("wasmtime compiles the module")
        .create_executor(linker(), (), accept, Some(fuel), None)
        .expect("wasmtime instantiates the module");
    [rwasm, wasmtime]
}

fn run(exec: &mut StrategyExecutor<()>, params: &[Value]) -> (Result<(), TrapCode>, Option<u64>) {
    let outcome = exec.execute("main", params, &mut []);
    (outcome, exec.remaining_fuel())
}

// ---------------------------------------------------------------------------------------------
// R5-1
// ---------------------------------------------------------------------------------------------

/// Control: a direct `call` to the import charges `CONST_FUEL` on both strategies.
#[test]
fn direct_syscall_fuel_is_charged_on_both_strategies() {
    let wat = r#"(module
      (import "env" "flat" (func $flat))
      (memory (export "memory") 1)
      (func (export "main") call $flat))"#;
    let [mut rwasm, mut wasmtime] = executors(wat, 100_000, false);
    let rwasm = run(&mut rwasm, &[]);
    let wasmtime = run(&mut wasmtime, &[]);
    assert_eq!(rwasm, wasmtime);
    assert!(rwasm.1.unwrap() <= 100_000 - CONST_FUEL, "{rwasm:?}");
}

/// The same import reached through a table entry or a tail call. rwasm keeps charging
/// `CONST_FUEL` (it lives in the trampoline), Wasmtime charges nothing.
#[test]
fn indirect_syscall_fuel_is_charged_on_both_strategies() {
    let paths = [
        ("call_indirect", "(call_indirect (type $t) (i32.const 0))"),
        (
            "return_call_indirect",
            "(return_call_indirect (type $t) (i32.const 0))",
        ),
        (
            "ref.func + table.set + call_indirect",
            "(table.set 0 (i32.const 1) (ref.func $flat)) (call_indirect (type $t) (i32.const 1))",
        ),
    ];
    let mut divergent = Vec::new();
    for (label, body) in paths {
        let wat = format!(
            r#"(module
              (type $t (func))
              (import "env" "flat" (func $flat))
              (memory (export "memory") 1)
              (table 2 funcref)
              (elem (i32.const 0) $flat)
              (func (export "main") {body}))"#
        );
        let [mut rwasm, mut wasmtime] = executors(&wat, 100_000, false);
        let rwasm = run(&mut rwasm, &[]);
        let wasmtime = run(&mut wasmtime, &[]);
        if rwasm != wasmtime {
            divergent.push((label, rwasm, wasmtime));
        }
    }
    assert!(
        divergent.is_empty(),
        "syscall fuel differs by dispatch path (label, rwasm, wasmtime): {divergent:#?}"
    );
}

/// An import exported as the entrypoint, and an import used as the start function: neither has a
/// Cranelift call site, so Wasmtime never charges the syscall fuel rwasm charges.
#[test]
fn entrypoint_and_start_imports_charge_syscall_fuel_on_both_strategies() {
    let cases = [
        (
            "export-of-import",
            r#"(module
              (import "env" "flat" (func $flat))
              (memory (export "memory") 1)
              (export "main" (func $flat)))"#,
            false,
        ),
        (
            "start-is-import",
            r#"(module
              (import "env" "flat" (func $flat))
              (memory (export "memory") 1)
              (start $flat)
              (func (export "main")))"#,
            true,
        ),
    ];
    let mut divergent = Vec::new();
    for (label, wat, allow_start) in cases {
        let [mut rwasm, mut wasmtime] = executors(wat, 100_000, allow_start);
        let rwasm = (rwasm.remaining_fuel(), run(&mut rwasm, &[]));
        let wasmtime = (wasmtime.remaining_fuel(), run(&mut wasmtime, &[]));
        if rwasm != wasmtime {
            divergent.push((label, rwasm, wasmtime));
        }
    }
    assert!(
        divergent.is_empty(),
        "(label, (fuel after instantiation, (outcome, fuel after call))): {divergent:#?}"
    );
}

/// The consequence: a loop of 1 MiB `LinearFuel` builtin calls through a table needs ~9.4M fuel
/// (rwasm traps `OutOfFuel` on a 1M budget), while Wasmtime completes all 100 calls for ~2000.
#[test]
fn indirect_builtin_calls_cannot_bypass_fuel_on_wasmtime() {
    let wat = r#"(module
      (type $t (func (param i32)))
      (import "env" "lin" (func $lin (param i32)))
      (memory (export "memory") 1)
      (table 1 funcref)
      (elem (i32.const 0) $lin)
      (func (export "main") (param $bytes i32) (param $iters i32)
        (block
          (loop
            (br_if 1 (i32.eqz (local.get $iters)))
            (call_indirect (type $t) (local.get $bytes) (i32.const 0))
            (local.set $iters (i32.sub (local.get $iters) (i32.const 1)))
            (br 0)))))"#;
    let params = [Value::I32(1_000_000), Value::I32(100)];
    let [mut rwasm, mut wasmtime] = executors(wat, 1_000_000, false);
    let rwasm = run(&mut rwasm, &params);
    let wasmtime = run(&mut wasmtime, &params);
    assert_eq!(
        rwasm.0,
        Err(TrapCode::OutOfFuel),
        "rwasm charges the builtin: {rwasm:?}"
    );
    assert_eq!(
        wasmtime, rwasm,
        "wasmtime must not run 100 MiB of metered builtin work on a 1M budget"
    );
}

// ---------------------------------------------------------------------------------------------
// R5-2
// ---------------------------------------------------------------------------------------------

/// A function whose stack peak is exactly the initial value-stack capacity (32 slots: one param,
/// 30 locals, one argument) calling a `LinearFuel` import. The trampoline's `StackCheck(0)`
/// reserves nothing for the two temporaries of the fuel prologue, so the first `LocalGet` lands
/// on `ptr == end` and rwasm traps `StackOverflow`; Wasmtime returns the argument.
#[test]
fn linear_fuel_trampoline_reserves_its_temporaries() {
    assert_trampoline_runs_at_capacity("lin", 30);
}

/// Same with `QuadraticFuel`, whose prologue peaks at four temporaries.
#[test]
fn quadratic_fuel_trampoline_reserves_its_temporaries() {
    assert_trampoline_runs_at_capacity("quad", 27);
}

fn assert_trampoline_runs_at_capacity(import: &str, locals: usize) {
    let wat = format!(
        r#"(module
          (import "env" "{import}" (func $builtin (param i32)))
          (memory (export "memory") 1)
          (func (export "main") (param i32) (result i32) (local {locals})
            (call $builtin (local.get 0))
            (local.get 0)))"#,
        locals = vec!["i32"; locals].join(" ")
    );
    let [mut rwasm, mut wasmtime] = executors(&wat, 1_000_000, false);
    let mut outcomes = Vec::new();
    for exec in [&mut rwasm, &mut wasmtime] {
        let mut result = [Value::I32(0)];
        let outcome = exec.execute("main", &[Value::I32(64)], &mut result);
        outcomes.push((outcome, result[0].clone()));
    }
    assert_eq!(
        outcomes[1],
        (Ok(()), Value::I32(64)),
        "wasmtime runs the module: {outcomes:?}"
    );
    assert_eq!(
        outcomes[0], outcomes[1],
        "rwasm must not trap on a stack peak the compiler accepted (rwasm, wasmtime): {outcomes:?}"
    );
}

//! Regressions for the round-6 syscall-fuel findings.
//!
//! Metered lengths must be `i32` parameters, and valid calls at the compiler's stack limit must
//! execute on both strategies. Pin rejection types, results, host arguments, and fuel explicitly:
//! agreement alone could hide the same failure or missing charge on both backends.
#![cfg(feature = "wasmtime")]

use rwasm::{
    CompilationConfig, CompilationError, ImportLinker, ImportName, StoreTr, StrategyDefinition,
    SyscallFuelParams, TrapCode, TypedCaller, ValType, Value, N_MAX_STACK_SIZE,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
use std::sync::Arc;

const INITIAL_FUEL: u64 = 100_000_000;

#[derive(Debug, Default, PartialEq)]
struct HostCalls {
    count: usize,
    params: Vec<Value>,
}

fn handler(
    caller: &mut TypedCaller<'_, HostCalls>,
    _: u32,
    params: &[Value],
    _: &mut [Value],
) -> Result<(), TrapCode> {
    let calls = caller.data_mut();
    calls.count += 1;
    calls.params = params.to_vec();
    Ok(())
}

/// Both policies meter 9 bytes (one word); keep these charges independent of the implementation.
fn policies(param_index: u32) -> [(&'static str, SyscallFuelParams, u64); 2] {
    [
        (
            "linear",
            SyscallFuelParams::LinearFuel(LinearFuelParams {
                base_fuel: 3,
                param_index,
                word_cost: 5,
            }),
            8, // 3 + 5 * 1
        ),
        (
            "quadratic",
            SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
                local_depth: param_index,
                word_cost: 3,
                divisor: 2,
                fuel_denom_rate: 4,
            }),
            12, // (3 * 1 + 1 * 1 / 2) * 4, with integer division
        ),
    ]
}

fn linker(policy: SyscallFuelParams, params: &'static [ValType]) -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(ImportName::new("env", "imp"), 0x71, policy, params, &[]);
    Arc::new(linker)
}

fn definitions(
    linker: &Arc<ImportLinker>,
    wasm: &[u8],
) -> [(&'static str, Result<StrategyDefinition, CompilationError>); 2] {
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_builtins_consume_fuel(true)
        .with_import_linker(linker.clone());
    [
        (
            "rwasm",
            StrategyDefinition::new_as_rwasm(config.clone(), wasm),
        ),
        (
            "wasmtime",
            StrategyDefinition::new_as_wasmtime(config, wasm, None),
        ),
    ]
}

fn assert_runs(
    linker: &Arc<ImportLinker>,
    wasm: &[u8],
    params: &[Value],
    host_params: &[Value],
    expected_fuel: u64,
    label: &str,
) {
    for (strategy, definition) in definitions(linker, wasm) {
        let definition = definition.unwrap_or_else(|err| panic!("{label}/{strategy}: {err:?}"));
        let mut executor = definition
            .create_executor(
                linker.clone(),
                HostCalls::default(),
                handler,
                Some(INITIAL_FUEL),
                None,
            )
            .unwrap_or_else(|trap| panic!("{label}/{strategy}: instantiate: {trap:?}"));
        let mut result = [Value::I64(-1)];
        assert_eq!(
            executor.execute("main", params, &mut result),
            Ok(()),
            "{label}/{strategy}"
        );
        assert_eq!(result, [Value::I64(0)], "{label}/{strategy}");
        assert_eq!(
            executor.data().count,
            1,
            "{label}/{strategy}: host call count"
        );
        assert_eq!(
            executor.data().params,
            host_params,
            "{label}/{strategy}: host arguments"
        );
        assert_eq!(
            executor.remaining_fuel(),
            Some(INITIAL_FUEL - expected_fuel),
            "{label}/{strategy}: fuel"
        );
    }
}

/// Use parameters rather than float constants so these fixtures also work with FPU disabled.
fn parameter_wasm(param_text: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
          (import "env" "imp" (func $imp (param {param_text})))
          (func (export "main") (param {param_text}) (result i64)
            local.get 0 local.get 1 call $imp i64.const 0))"#
    ))
    .unwrap()
}

/// Parameter signatures and the position of their non-i32 parameter, counted from the end.
const PARAMETER_CASES: [(&str, &[ValType], u32); 6] = [
    ("i32 i64", &[ValType::I32, ValType::I64], 1),
    ("i64 i32", &[ValType::I64, ValType::I32], 2),
    ("i32 f64", &[ValType::I32, ValType::F64], 1),
    ("f64 i32", &[ValType::F64, ValType::I32], 2),
    ("i32 f32", &[ValType::I32, ValType::F32], 1),
    ("f32 i32", &[ValType::F32, ValType::I32], 2),
];

/// R6-F1: a non-i32 metered parameter is a configuration error on both strategies. The old rwasm
/// trampoline accepted wide values and read one 32-bit word, while Wasmtime rejected them.
#[test]
fn non_i32_metered_syscall_parameters_are_rejected() {
    for (param_text, params, index) in PARAMETER_CASES {
        let wasm = parameter_wasm(param_text);
        for (policy_name, policy, _) in policies(index) {
            let linker = linker(policy, params);
            for (strategy, definition) in definitions(&linker, &wasm) {
                let err = definition.err().unwrap_or_else(|| {
                    panic!("{param_text}/{policy_name}/{strategy}: accepted a non-i32 metered parameter")
                });
                assert!(
                    matches!(err, CompilationError::InvalidSyscallFuelParam),
                    "{param_text}/{policy_name}/{strategy}: unexpected rejection: {err:?}"
                );
            }
        }
    }
}

/// Rejecting a non-i32 metered length must not reject a different, unmetered wide parameter or
/// change which parameter is charged when that wide value occupies two rwasm stack slots.
#[test]
fn i32_metered_parameters_with_non_i32_neighbors_run_and_charge_correctly() {
    for (param_text, params, non_i32_index) in PARAMETER_CASES {
        let value = match params[2 - non_i32_index as usize] {
            // Either 32-bit half would charge for two words, unlike the one-word i32 length.
            ValType::I64 => Value::I64(0x40_0000_0040),
            ValType::F64 => Value::F64(4.0.into()),
            ValType::F32 => Value::F32(4.0.into()),
            _ => unreachable!(),
        };
        let args = if non_i32_index == 1 {
            [Value::I32(9), value]
        } else {
            [value, Value::I32(9)]
        };
        let wasm = parameter_wasm(param_text);
        for (policy_name, policy, charge) in policies(3 - non_i32_index) {
            // 1 entry + 2 local.get + 10 call + 1 i64.const, plus the syscall policy.
            assert_runs(
                &linker(policy, params),
                &wasm,
                &args,
                &args,
                14 + charge,
                &format!("{param_text}/{policy_name}"),
            );
        }
    }
}

fn stack_wasm(locals: usize) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
          (import "env" "imp" (func $imp (param i32)))
          (func (export "main") (result i64) {locals}
            i32.const 9 call $imp i64.const 0))"#,
        locals = "(local i32)".repeat(locals)
    ))
    .unwrap()
}

/// R6-F2: the i64 result makes the Wasm frame peak `locals + 2`. A linear trampoline needs two
/// additional slots during the call; a quadratic one needs four. Every formerly failing frame
/// up to the compiler's limit must run, return the right value and charge the expected fuel.
#[test]
fn stack_window_boundary_for_metered_imports_runs_and_charges_correctly() {
    for locals in N_MAX_STACK_SIZE - 4..=N_MAX_STACK_SIZE - 2 {
        let wasm = stack_wasm(locals);
        for (policy_name, policy, charge) in policies(1) {
            // 1 entry + 1 i32.const + 10 call + 1 i64.const, plus the syscall policy.
            assert_runs(
                &linker(policy, &[ValType::I32]),
                &wasm,
                &[],
                &[Value::I32(9)],
                13 + charge,
                &format!("{policy_name}/{locals} locals"),
            );
        }
    }
}

/// Control: this frame fit even before the runtime reserved trampoline headroom.
#[test]
fn stack_window_below_the_boundary_runs_and_charges_correctly() {
    let wasm = stack_wasm(N_MAX_STACK_SIZE - 5);
    for (policy_name, policy, charge) in policies(1) {
        assert_runs(
            &linker(policy, &[ValType::I32]),
            &wasm,
            &[],
            &[Value::I32(9)],
            13 + charge,
            policy_name,
        );
    }
}

/// Trampoline headroom must not enlarge the accepted Wasm frame: one slot above the limit is
/// still rejected by both strategies, including the height and limit reported by the compiler.
#[test]
fn stack_window_above_the_boundary_is_rejected() {
    let wasm = stack_wasm(N_MAX_STACK_SIZE - 1);
    for (policy_name, policy, _) in policies(1) {
        for (strategy, definition) in definitions(&linker(policy, &[ValType::I32]), &wasm) {
            let err = definition
                .err()
                .unwrap_or_else(|| panic!("{policy_name}/{strategy}: oversized frame accepted"));
            assert!(
                matches!(err, CompilationError::StackHeightExceeded { height, limit }
                if height == N_MAX_STACK_SIZE as u32 + 1 && limit == N_MAX_STACK_SIZE as u32),
                "{policy_name}/{strategy}: unexpected rejection: {err:?}"
            );
        }
    }
}

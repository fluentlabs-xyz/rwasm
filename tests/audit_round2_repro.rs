//! Regression tests for the valid-Wasm findings of the 2026-09-13 audit.
//! Both backends must preserve host argument widths, enforce the complete stack-frame limit,
//! return zeroed results after a halt, and reject a halting start function.
#![cfg(feature = "wasmtime")]

use rwasm::{
    CompilationConfig, CompilationError, ImportLinker, ImportName, StrategyDefinition,
    StrategyExecutor, SyscallFuelParams, SyscallHandler, TrapCode, TypedCaller, Value,
};
use std::sync::Arc;
use wasmparser::ValType;

fn wat_type(ty: &ValType) -> &'static str {
    match ty {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        _ => panic!("non-numeric test type"),
    }
}

fn config(linker: &Arc<ImportLinker>) -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(linker.clone())
}

/// Compiles `wasm` for both strategies and instantiates each with `linker`/`handler`.
fn both_executors(
    wasm: &[u8],
    linker: Arc<ImportLinker>,
    handler: SyscallHandler<()>,
) -> (StrategyExecutor<()>, StrategyExecutor<()>) {
    let instantiate = |definition: StrategyDefinition| {
        definition
            .create_executor(linker.clone(), (), handler, Some(1_000_000), None)
            .expect("the module must instantiate")
    };
    let rwasm = instantiate(
        StrategyDefinition::new_as_rwasm(config(&linker), wasm).expect("rwasm compiles the module"),
    );
    let wasmtime = instantiate(
        StrategyDefinition::new_as_wasmtime(config(&linker), wasm, None)
            .expect("wasmtime compiles the module"),
    );
    (rwasm, wasmtime)
}

/// `R2-1`, signature `(i64, i32) -> i64`.
///
/// Wasm pushes arguments left to right, so the *last* argument is on top of the operand stack and
/// each parameter has to be popped with its own width. `RwasmExecutor::invoke_syscall` pops the
/// last argument first (correct) but applies `params[i]`'s width to it (wrong), so a signature
/// whose width pattern is not palindromic delivers permuted, corrupted values: here the handler
/// receives `[I32, I64]` instead of `[I64, I32]`, and typed accessors such as
/// `params[0].i64().unwrap()` panic inside the host.
#[test]
fn host_syscall_parameters_keep_their_declared_widths() {
    let wasm = wat::parse_str(
        r#"(module
             (import "env" "s" (func $s (param i64 i32) (result i64)))
             (func (export "main") (result i64)
               i64.const 0x1_0000_0002
               i32.const 7
               call $s))"#,
    )
    .unwrap();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "s"),
        1,
        SyscallFuelParams::default(),
        &[ValType::I64, ValType::I32],
        &[ValType::I64],
    );
    let linker = Arc::new(linker);

    fn handler(
        _caller: &mut TypedCaller<'_, ()>,
        _sys_func_idx: u32,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match (params[0].i64(), params[1].i32()) {
            (Some(wide), Some(narrow)) => result[0] = Value::I64(wide ^ ((narrow as i64) << 32)),
            // The parameters arrived with the wrong types: report a marker the assertion can name.
            _ => result[0] = Value::I64(-1),
        }
        Ok(())
    }

    let expected = 0x1_0000_0002_i64 ^ (7_i64 << 32);
    let (mut rwasm, mut wasmtime) = both_executors(&wasm, linker, handler);

    let mut rwasm_result = [Value::I64(0)];
    let mut wasmtime_result = [Value::I64(0)];
    wasmtime
        .execute("main", &[], &mut wasmtime_result)
        .expect("wasmtime executes");
    rwasm
        .execute("main", &[], &mut rwasm_result)
        .expect("rwasm executes");

    assert_eq!(
        wasmtime_result[0].i64(),
        Some(expected),
        "wasmtime delivers the values in the declared order"
    );
    assert_eq!(
        rwasm_result[0].i64(),
        Some(expected),
        "R2-1: rwasm delivered {:?} to the handler instead of [{expected:#x}]",
        rwasm_result[0]
    );
}

/// `R2-1`, signature `(i32, i64) -> i64`: the same defect from the other side, so a fix that only
/// reverses the pop order without matching widths still fails here.
#[test]
fn host_syscall_parameters_keep_their_declared_widths_with_reversed_signature() {
    let wasm = wat::parse_str(
        r#"(module
             (import "env" "s" (func $s (param i32 i64) (result i64)))
             (func (export "main") (result i64)
               i32.const 7
               i64.const 0x1_0000_0002
               call $s))"#,
    )
    .unwrap();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "s"),
        1,
        SyscallFuelParams::default(),
        &[ValType::I32, ValType::I64],
        &[ValType::I64],
    );
    let linker = Arc::new(linker);

    fn handler(
        _caller: &mut TypedCaller<'_, ()>,
        _sys_func_idx: u32,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match (params[0].i32(), params[1].i64()) {
            (Some(narrow), Some(wide)) => result[0] = Value::I64((narrow as i64) ^ wide),
            _ => result[0] = Value::I64(-1),
        }
        Ok(())
    }

    let expected = 7_i64 ^ 0x1_0000_0002_i64;
    let (mut rwasm, mut wasmtime) = both_executors(&wasm, linker, handler);

    let mut rwasm_result = [Value::I64(0)];
    let mut wasmtime_result = [Value::I64(0)];
    wasmtime
        .execute("main", &[], &mut wasmtime_result)
        .expect("wasmtime executes");
    rwasm
        .execute("main", &[], &mut rwasm_result)
        .expect("rwasm executes");

    assert_eq!(wasmtime_result[0].i64(), Some(expected));
    assert_eq!(
        rwasm_result[0].i64(),
        Some(expected),
        "R2-1: rwasm delivered {:?} to the handler instead of [{expected:#x}]",
        rwasm_result[0]
    );
}

/// R2-2: parameters, locals and operands share the same 8192-slot window.
#[test]
fn parameter_slots_count_towards_the_value_stack_window() {
    const PARAMS: usize = 100;
    for (ty, width) in [(ValType::I32, 1), (ValType::I64, 2), (ValType::F64, 2)] {
        for height in [8191, 8192, 8193] {
            let locals = height - PARAMS * width - 1;
            let wasm = wat::parse_str(format!(
                r#"(module (func (export "main") {} (result i32) {} (i32.const 42)))"#,
                format!("(param {})", wat_type(&ty)).repeat(PARAMS),
                "(local i32)".repeat(locals)
            ))
            .unwrap();
            let linker = Arc::new(ImportLinker::default());
            let definitions = [
                StrategyDefinition::new_as_rwasm(config(&linker), &wasm),
                StrategyDefinition::new_as_wasmtime(config(&linker), &wasm, None),
            ];
            for definition in definitions {
                if height > 8192 {
                    assert!(matches!(
                        definition,
                        Err(CompilationError::StackHeightExceeded {
                            height: 8193,
                            limit: 8192
                        })
                    ));
                } else {
                    let mut executor = definition.unwrap().default_executor().unwrap();
                    let mut result = [Value::I32(-1)];
                    executor
                        .execute("main", &vec![Value::default(ty); PARAMS], &mut result)
                        .unwrap();
                    assert_eq!(result, [Value::I32(42)]);
                }
            }
        }
    }
}

/// `R2-3`: after a host handler returns `ExecutionHalted` both engines report success, but the
/// caller's result buffer keeps its previous contents on rwasm while the Wasmtime backend writes
/// zeros. The engines have to agree on this observable output.
#[test]
fn halted_call_reports_the_same_result_buffer_on_both_backends() {
    let wasm = wat::parse_str(
        r#"(module
             (import "env" "exit" (func $exit (result i32)))
             (func (export "main") (result i32) call $exit))"#,
    )
    .unwrap();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "exit"),
        1,
        SyscallFuelParams::default(),
        &[],
        &[ValType::I32],
    );
    let linker = Arc::new(linker);

    fn halting_handler(
        _caller: &mut TypedCaller<'_, ()>,
        _sys_func_idx: u32,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I32(4242);
        Err(TrapCode::ExecutionHalted)
    }

    let (mut rwasm, mut wasmtime) = both_executors(&wasm, linker, halting_handler);

    // A template the caller reused from a previous execution: "untouched" and "zeroed" differ.
    let mut rwasm_result = [Value::I32(-1)];
    let mut wasmtime_result = [Value::I32(-1)];
    let rwasm_outcome = rwasm.execute("main", &[], &mut rwasm_result);
    let wasmtime_outcome = wasmtime.execute("main", &[], &mut wasmtime_result);

    assert_eq!(rwasm_outcome, Ok(()), "a halt is a successful termination");
    assert_eq!(wasmtime_outcome, Ok(()));
    assert_eq!(wasmtime_result, [Value::I32(0)]);
    assert_eq!(
        rwasm_result[0].i32(),
        wasmtime_result[0].i32(),
        "R2-3: rwasm left {:?} in the result buffer where wasmtime wrote {:?}",
        rwasm_result[0],
        wasmtime_result[0]
    );
}

/// `R2-4`: a start function that halts on a host syscall is accepted by the rwasm entrypoint path
/// (`ExecutionHalted` is mapped to `Ok(())` there) and rejects instantiation on the Wasmtime
/// backend, so the same module is deployable on one engine and not on the other.
#[test]
fn start_function_that_halts_agrees_on_both_backends() {
    let wasm = wat::parse_str(
        r#"(module
             (import "env" "exit" (func $exit))
             (memory (export "memory") 1)
             (global $state (mut i32) (i32.const 0))
             (start $init)
             (func $init
               i32.const 7
               global.set $state
               call $exit
               i32.const 9
               global.set $state)
             (func (export "main") (result i32) global.get $state))"#,
    )
    .unwrap();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "exit"),
        1,
        SyscallFuelParams::default(),
        &[],
        &[],
    );
    let linker = Arc::new(linker);
    let cfg = config(&linker).with_allow_start_section(true);

    fn halting_handler(
        _caller: &mut TypedCaller<'_, ()>,
        _sys_func_idx: u32,
        _params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        Err(TrapCode::ExecutionHalted)
    }

    let instantiate = |definition: StrategyDefinition| {
        definition
            .create_executor(linker.clone(), (), halting_handler, Some(1_000_000), None)
            .map(|_| ())
    };
    let rwasm =
        instantiate(StrategyDefinition::new_as_rwasm(cfg.clone(), &wasm).expect("compiles"));
    let wasmtime =
        instantiate(StrategyDefinition::new_as_wasmtime(cfg, &wasm, None).expect("compiles"));

    assert_eq!(rwasm, Err(TrapCode::ExecutionHalted));
    assert_eq!(wasmtime, Err(TrapCode::ExecutionHalted));
}

/// Mixed numeric signatures and multiple results exercise both ordering and slot widths.
#[test]
fn mixed_numeric_syscalls_preserve_values_and_multiple_results() {
    let signatures = [
        vec![Value::I64(0x1234_5678_9abc_def0), Value::I32(-7)],
        vec![Value::I32(-7), Value::I64(0x1234_5678_9abc_def0)],
        vec![Value::F64(123.5.into()), Value::I32(-7)],
        vec![
            Value::I32(-7),
            Value::F32((-5.25).into()),
            Value::I64(-123456789012),
            Value::F64(123.5.into()),
        ],
    ];
    for args in signatures {
        let params: &'static [ValType] = Box::leak(
            args.iter()
                .map(Value::ty)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let results: &'static [ValType] = Box::leak(
            params
                .iter()
                .rev()
                .copied()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let param_types = params.iter().map(wat_type).collect::<Vec<_>>().join(" ");
        let result_types = results.iter().map(wat_type).collect::<Vec<_>>().join(" ");
        let loads = (0..args.len())
            .map(|i| format!("local.get {i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let wasm = wat::parse_str(format!(
            r#"(module
            (import "env" "s" (func $s (param {param_types}) (result {result_types})))
            (func (export "main") (param {param_types}) (result {result_types}) {loads} call $s))"#
        ))
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "s"),
            1,
            SyscallFuelParams::default(),
            params,
            results,
        );
        fn handler(
            _: &mut TypedCaller<'_, ()>,
            _: u32,
            params: &[Value],
            result: &mut [Value],
        ) -> Result<(), TrapCode> {
            for (out, arg) in result.iter_mut().zip(params.iter().rev()) {
                *out = arg.clone();
            }
            Ok(())
        }
        let (rwasm, wasmtime) = both_executors(&wasm, Arc::new(linker), handler);
        let expected = args.iter().rev().cloned().collect::<Vec<_>>();
        for mut executor in [rwasm, wasmtime] {
            let mut result = results
                .iter()
                .map(|ty| Value::default(*ty))
                .collect::<Vec<_>>();
            executor.execute("main", &args, &mut result).unwrap();
            assert_eq!(result, expected);
        }
    }
}

/// A syscall replaces its arguments even when the operand stack fills the whole window.
#[test]
fn syscall_at_the_stack_limit_reuses_parameter_slots() {
    let wasm = wat::parse_str(format!(
        r#"(module
        (import "env" "s" (func $s (param i64 i32) (result i64)))
        (func (export "main") (result i64) {}
            i64.const 0x123456789abcdef0 i32.const 7 call $s))"#,
        "(local i32)".repeat(8189)
    ))
    .unwrap();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "s"),
        1,
        SyscallFuelParams::default(),
        &[ValType::I64, ValType::I32],
        &[ValType::I64],
    );
    fn handler(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        assert_eq!(params, [Value::I64(0x123456789abcdef0), Value::I32(7)]);
        result[0] = params[0].clone();
        Ok(())
    }
    let (rwasm, wasmtime) = both_executors(&wasm, Arc::new(linker), handler);
    for mut executor in [rwasm, wasmtime] {
        let mut result = [Value::I64(0)];
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result, [Value::I64(0x123456789abcdef0)]);
    }
}

#[test]
fn halted_call_zeros_every_numeric_result_type() {
    let wasm = wat::parse_str(
        r#"(module
        (import "env" "exit" (func $exit))
        (func (export "main") (result i32 i64 f32 f64) call $exit unreachable))"#,
    )
    .unwrap();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "exit"),
        1,
        SyscallFuelParams::default(),
        &[],
        &[],
    );
    fn handler(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Err(TrapCode::ExecutionHalted)
    }
    let (rwasm, wasmtime) = both_executors(&wasm, Arc::new(linker), handler);
    for mut executor in [rwasm, wasmtime] {
        let mut result = [
            Value::I32(-1),
            Value::I64(-2),
            Value::F32(3.5.into()),
            Value::F64(4.5.into()),
        ];
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(
            result,
            [
                Value::I32(0),
                Value::I64(0),
                Value::F32(0.0.into()),
                Value::F64(0.0.into())
            ]
        );
    }
}

#[test]
fn syscall_reserves_all_wide_result_slots() {
    use rwasm::{instruction_set, ExecutionEngine, RwasmModuleBuilder, RwasmStore};
    // No StackCheck: exercise the host ABI's own reservation past the default 32-slot capacity.
    let module = RwasmModuleBuilder::new(instruction_set! { Call(1) Return }).build();
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "s"),
        1,
        SyscallFuelParams::default(),
        &[],
        &[ValType::I64; 33],
    );
    fn handler(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        for (i, out) in result.iter_mut().enumerate() {
            *out = Value::I64(0x1234567800000000 + i as i64);
        }
        Ok(())
    }
    let mut store = RwasmStore::new(Arc::new(linker), (), handler, None, None);
    let mut result = vec![Value::I64(0); 33];
    ExecutionEngine::new()
        .execute(&mut store, &module, &[], &mut result)
        .unwrap();
    assert_eq!(
        result,
        (0..33)
            .map(|i| Value::I64(0x1234567800000000 + i))
            .collect::<Vec<_>>()
    );
}

#[test]
fn resumed_initialization_preserves_the_halt_error() {
    use rwasm::{instruction_set, ExecutionEngine, RwasmModuleBuilder, RwasmStore};
    let module = RwasmModuleBuilder::new(instruction_set! { Call(1) Call(2) Return }).build();
    let mut linker = ImportLinker::default();
    for (name, index) in [("pause", 1), ("exit", 2)] {
        linker.insert_function(
            ImportName::new("env", name),
            index,
            SyscallFuelParams::default(),
            &[],
            &[],
        );
    }
    fn handler(
        _: &mut TypedCaller<'_, ()>,
        index: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Err(if index == 1 {
            TrapCode::InterruptionCalled
        } else {
            TrapCode::ExecutionHalted
        })
    }
    let linker = Arc::new(linker);
    let engine = ExecutionEngine::new();
    for initializing in [false, true] {
        let mut store = RwasmStore::new(linker.clone(), (), handler, None, None);
        let outcome = if initializing {
            engine.entrypoint(&mut store, &module)
        } else {
            engine.execute(&mut store, &module, &[], &mut [])
        };
        assert_eq!(outcome, Err(TrapCode::InterruptionCalled));
        assert_eq!(
            engine.resume(&mut store, &[], &mut []),
            if initializing {
                Err(TrapCode::ExecutionHalted)
            } else {
                Ok(())
            }
        );
        assert_eq!(
            engine.resume(&mut store, &[], &mut []),
            Err(TrapCode::IllegalOpcode)
        );
    }
}

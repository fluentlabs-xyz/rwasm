use crate::{
    wasmtime::{compile_wasmtime_module, WasmtimeExecutor},
    CompilationConfig, ImportLinker, ImportName, StoreTr, TrapCode, TypedCaller, Value,
    N_BYTES_PER_MEMORY_PAGE,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams, SyscallFuelParams};
use std::sync::Arc;
use wasmtime::Module;

const DIVISOR: u64 = 10;
const WORD_COST: u64 = 0;

fn get_test_wasmtime_module() -> (Module, Arc<ImportLinker>) {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $default_call (import "call" "linear") (param i32))
              (func $quadratic_call (import "call" "quadratic") (param i32))
              (func (export "main")
                (i32.const 300)
                (call $default_call)
              )
              (func (export "main_with_quadratic")
                (i32.const 300)
                (call $quadratic_call)
              )
              (func (export "main_with_overflow")
                (i32.const 134_217_729)
                (call $default_call)
              )
              (func (export "main_quadratic_with_overflow")
                (i32.const 1_310_721)
                (call $quadratic_call)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("call", "quadratic"),
        0xdd,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: WORD_COST as u32,
            divisor: DIVISOR as u32,
            fuel_denom_rate: 1,
        }),
        &[wasmparser::ValType::I32],
        &[],
    );

    import_linker.insert_function(
        ImportName::new("call", "linear"),
        0xee,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index: 1,
            word_cost: 5,
        }),
        &[wasmparser::ValType::I32],
        &[],
    );

    let import_linker = Arc::new(import_linker);
    // run with wasmtime
    let compilation_config = CompilationConfig::default()
        .with_consume_fuel(true)
        .with_builtins_consume_fuel(true)
        .with_import_linker(import_linker.clone());

    (
        compile_wasmtime_module(compilation_config, wasm_binary).unwrap(),
        import_linker,
    )
}

#[test]
fn test_call_with_charging_quadratic_wasmtime() {
    let (module, import_linker) = get_test_wasmtime_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker.clone(),
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
        None,
    );

    wasmtime_worker
        .execute("main_with_quadratic", &[], &mut [])
        .unwrap();
    let words = 300_u64.div_ceil(32);
    assert_eq!(
        wasmtime_worker.store.get_fuel().unwrap(),
        100_000 - (1 + 1 + 10 + WORD_COST * words + words * words / DIVISOR)
    );
}

#[test]
fn test_call_with_charging_linear_wasmtime() {
    let (module, import_linker) = get_test_wasmtime_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker.clone(),
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
        None,
    );

    wasmtime_worker.execute("main", &[], &mut []).unwrap();
    assert_eq!(
        wasmtime_worker.store.get_fuel().unwrap(),
        100_000 - (1 + 1 + 10 + 10 * 5 + 7)
    );
}

#[test]
fn test_call_with_charging_param_overflow_wasmtime() {
    let (module, import_linker) = get_test_wasmtime_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker.clone(),
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
        None,
    );

    let err = wasmtime_worker
        .execute("main_with_overflow", &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::IntegerOverflow);
    let err = wasmtime_worker
        .execute("main_quadratic_with_overflow", &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::IntegerOverflow);
}

#[test]
fn test_wasmtime_executor_missing_entrypoint_returns_trap() {
    let (module, import_linker) = get_test_wasmtime_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
        None,
    );

    let err = wasmtime_worker
        .execute("missing_export", &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::UnknownExternalFunction);
}

fn get_test_memory_module() -> (Module, Arc<ImportLinker>) {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $read (import "host" "read") (param i32 i32))
              (memory (export "memory") 1)
              (data (i32.const 0) "\01\02\03\04")
              (func (export "read_ok")
                (i32.const 0)
                (i32.const 4)
                (call $read)
              )
              (func (export "read_oob")
                (i32.const 65536)
                (i32.const 1)
                (call $read)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("host", "read"),
        0xab,
        SyscallFuelParams::default(),
        &[wasmparser::ValType::I32, wasmparser::ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);
    let compilation_config = CompilationConfig::default().with_import_linker(import_linker.clone());

    (
        compile_wasmtime_module(compilation_config, wasm_binary).unwrap(),
        import_linker,
    )
}

fn get_test_module_without_memory() -> (Module, Arc<ImportLinker>) {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $read (import "host" "read") (param i32 i32))
              (func (export "read_missing_memory")
                (i32.const 0)
                (i32.const 1)
                (call $read)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("host", "read"),
        0xab,
        SyscallFuelParams::default(),
        &[wasmparser::ValType::I32, wasmparser::ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);
    let compilation_config = CompilationConfig::default().with_import_linker(import_linker.clone());

    (
        compile_wasmtime_module(compilation_config, wasm_binary).unwrap(),
        import_linker,
    )
}

fn read_memory_syscall(
    caller: &mut TypedCaller<'_, Vec<u8>>,
    _sys_func_idx: u32,
    params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    let offset = match params[0] {
        Value::I32(value) => value as usize,
        _ => unreachable!("unexpected offset type"),
    };
    let length = match params[1] {
        Value::I32(value) => value as usize,
        _ => unreachable!("unexpected length type"),
    };
    let bytes = caller.memory_read_into_vec(offset, length)?;
    caller.data_mut().extend(bytes);
    Ok(())
}

#[test]
fn test_wasmtime_caller_missing_memory_returns_trap() {
    let (module, import_linker) = get_test_module_without_memory();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    );

    assert_eq!(
        wasmtime_worker
            .execute("read_missing_memory", &[], &mut [])
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
}

#[test]
fn test_wasmtime_snapshot_missing_memory_returns_trap() {
    let (module, import_linker) = get_test_module_without_memory();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    );

    assert_eq!(
        wasmtime_worker.snapshot_memory().unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
}

#[test]
fn test_wasmtime_executor_memory_read_into_vec_checks_bounds_before_allocating() {
    let (module, import_linker) = get_test_memory_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    );

    assert_eq!(
        wasmtime_worker.memory_read_into_vec(0, 4).unwrap(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        wasmtime_worker
            .memory_read_into_vec(N_BYTES_PER_MEMORY_PAGE as usize, 1)
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
    assert_eq!(
        wasmtime_worker
            .memory_read_into_vec(usize::MAX, 1)
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
}

#[test]
fn test_wasmtime_caller_memory_read_into_vec_checks_bounds_before_allocating() {
    let (module, import_linker) = get_test_memory_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    );

    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.data(), &[1, 2, 3, 4]);
    assert_eq!(
        wasmtime_worker
            .execute("read_oob", &[], &mut [])
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
}

fn get_test_module_without_engine_fuel() -> (Module, Arc<ImportLinker>) {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $probe (import "host" "probe"))
              (memory (export "memory") 1)
              (func (export "main"))
              (func (export "probe") (call $probe))
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("host", "probe"),
        0xef,
        SyscallFuelParams::default(),
        &[],
        &[],
    );
    let import_linker = Arc::new(import_linker);
    let compilation_config = CompilationConfig::default()
        .with_consume_fuel(false)
        .with_import_linker(import_linker.clone());
    (
        compile_wasmtime_module(compilation_config, wasm_binary).unwrap(),
        import_linker,
    )
}

#[test]
fn test_wasmtime_fuel_accessors_use_engine_metering_when_enabled() {
    let (module, import_linker) = get_test_wasmtime_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
        None,
    );

    assert_eq!(wasmtime_worker.remaining_fuel(), Some(100_000));
    wasmtime_worker.try_consume_fuel(10).unwrap();
    assert_eq!(wasmtime_worker.store.get_fuel().unwrap(), 99_990);
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(99_990));
    assert_eq!(
        wasmtime_worker.try_consume_fuel(100_000).unwrap_err(),
        TrapCode::OutOfFuel
    );
    wasmtime_worker.reset_fuel(5);
    assert_eq!(wasmtime_worker.store.get_fuel().unwrap(), 5);
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(5));
}

#[test]
fn test_wasmtime_fuel_accessors_use_soft_counter_when_engine_metering_is_off() {
    let (module, import_linker) = get_test_module_without_engine_fuel();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(1_000),
        None,
    );

    assert!(wasmtime_worker.store.get_fuel().is_err());
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(1_000));
    wasmtime_worker.try_consume_fuel(600).unwrap();
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(400));
    assert_eq!(
        wasmtime_worker.try_consume_fuel(401).unwrap_err(),
        TrapCode::OutOfFuel
    );
    wasmtime_worker.reset_fuel(7);
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(7));
    wasmtime_worker.execute("main", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(7));
}

#[test]
fn test_wasmtime_executor_exports_follow_the_instance() {
    let (memory_module, import_linker) = get_test_memory_module();
    let module_without_memory = Module::new(
        memory_module.engine(),
        wat::parse_str(
            r#"
            (module
              (func $read (import "host" "read") (param i32 i32))
              (func (export "read_missing_memory")
                (i32.const 0)
                (i32.const 1)
                (call $read)
              )
            )
            "#,
        )
        .unwrap(),
    )
    .unwrap();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        memory_module.clone(),
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    );
    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.data(), &[1, 2, 3, 4]);

    // Re-instantiating through the executor swaps both the function table and the memory.
    wasmtime_worker.instantiate(&module_without_memory).unwrap();
    assert_eq!(
        wasmtime_worker
            .execute("read_missing_memory", &[], &mut [])
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
    assert_eq!(
        wasmtime_worker
            .execute("read_ok", &[], &mut [])
            .unwrap_err(),
        TrapCode::UnknownExternalFunction
    );
    assert_eq!(
        wasmtime_worker.memory_read_into_vec(0, 1).unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );

    // Replacing the public `instance` field directly must resolve the new exports as well.
    let instance_pre = wasmtime_worker
        .linker
        .instantiate_pre(&memory_module)
        .unwrap();
    wasmtime_worker.instance = instance_pre
        .instantiate(&mut wasmtime_worker.store)
        .unwrap();
    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.data(), &[1, 2, 3, 4, 1, 2, 3, 4]);
    assert_eq!(
        wasmtime_worker.memory_read_into_vec(0, 4).unwrap(),
        vec![1, 2, 3, 4]
    );
}

fn get_test_numeric_marshalling_module() -> (Module, Arc<ImportLinker>) {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $mix (import "host" "mix") (param i32 i64 f32 f64) (result i64))
              (func (export "main") (result i64)
                (i32.const -7)
                (i64.const 0x1_0000_0000)
                (f32.const 1.5)
                (f64.const -2.25)
                (call $mix)
              )
              (func (export "add") (param i32 i32) (result i32)
                (i32.add (local.get 0) (local.get 1))
              )
              (func (export "pass_f64") (param f64) (result f64)
                (local.get 0)
              )
              (func (export "widen") (param i32) (result i64)
                (i64.extend_i32_s (local.get 0))
              )
              (func (export "mix_params") (param i32 i64) (result i64)
                (call $mix (local.get 0) (local.get 1) (f32.const 0) (f64.const 0))
              )
              (func (export "pass_f32") (param f32) (result f32)
                (local.get 0)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("host", "mix"),
        0xcd,
        SyscallFuelParams::default(),
        &[
            wasmparser::ValType::I32,
            wasmparser::ValType::I64,
            wasmparser::ValType::F32,
            wasmparser::ValType::F64,
        ],
        &[wasmparser::ValType::I64],
    );
    let import_linker = Arc::new(import_linker);
    let compilation_config = CompilationConfig::default().with_import_linker(import_linker.clone());
    (
        compile_wasmtime_module(compilation_config, wasm_binary).unwrap(),
        import_linker,
    )
}

/// Folds every parameter into one `i64` so the test can check each value arrived intact.
fn mix_syscall(
    _caller: &mut TypedCaller<'_, ()>,
    sys_func_idx: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    assert_eq!(sys_func_idx, 0xcd);
    let [Value::I32(a), Value::I64(b), Value::F32(c), Value::F64(d)] = params else {
        panic!("unexpected parameter types: {params:?}");
    };
    assert_eq!(c.to_bits(), 1.5f32.to_bits());
    assert_eq!(d.to_bits(), (-2.25f64).to_bits());
    result[0] = Value::I64(*a as i64 + *b + c.to_bits() as i64 + d.to_bits() as i64);
    Ok(())
}

#[test]
fn test_wasmtime_numeric_imports_round_trip_through_raw_slots() {
    let (module, import_linker) = get_test_numeric_marshalling_module();
    let mut wasmtime_worker =
        WasmtimeExecutor::new(module, import_linker, (), mix_syscall, Some(100_000), None);

    let mut result = [Value::I64(0)];
    wasmtime_worker.execute("main", &[], &mut result).unwrap();
    assert_eq!(
        result[0],
        Value::I64(-7 + 0x1_0000_0000 + 1.5f32.to_bits() as i64 + (-2.25f64).to_bits() as i64)
    );
}

#[test]
fn test_wasmtime_numeric_exports_marshal_params_and_results() {
    let (module, import_linker) = get_test_numeric_marshalling_module();
    let mut wasmtime_worker =
        WasmtimeExecutor::new(module, import_linker, (), mix_syscall, Some(100_000), None);

    let mut result = [Value::I32(0)];
    wasmtime_worker
        .execute("add", &[Value::I32(40), Value::I32(2)], &mut result)
        .unwrap();
    assert_eq!(result[0], Value::I32(42));

    // Float values pass through untouched (float arithmetic itself is disabled by the engine).
    let mut result = [Value::F64(crate::F64::from_bits(0))];
    wasmtime_worker
        .execute(
            "pass_f64",
            &[Value::F64(crate::F64::from_bits((-2.25f64).to_bits()))],
            &mut result,
        )
        .unwrap();
    assert_eq!(
        result[0],
        Value::F64(crate::F64::from_bits((-2.25f64).to_bits()))
    );

    let mut result = [Value::I64(0)];
    wasmtime_worker
        .execute("widen", &[Value::I32(-1)], &mut result)
        .unwrap();
    assert_eq!(result[0], Value::I64(-1));

    let mut result = [Value::F32(crate::F32::from_bits(0))];
    wasmtime_worker
        .execute(
            "pass_f32",
            &[Value::F32(crate::F32::from_bits(1.5f32.to_bits()))],
            &mut result,
        )
        .unwrap();
    assert_eq!(
        result[0],
        Value::F32(crate::F32::from_bits(1.5f32.to_bits()))
    );

    // Mismatched arity or types are rejected before the call, as the checked path did.
    let mut result = [Value::I32(0)];
    assert_eq!(
        wasmtime_worker
            .execute("add", &[Value::I32(1)], &mut result)
            .unwrap_err(),
        TrapCode::IllegalOpcode
    );
    assert_eq!(
        wasmtime_worker
            .execute("add", &[Value::I64(1), Value::I32(1)], &mut result)
            .unwrap_err(),
        TrapCode::IllegalOpcode
    );
}

#[test]
fn test_wasmtime_raw_import_rejects_mistyped_syscall_results() {
    let (module, import_linker) = get_test_numeric_marshalling_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, result| -> Result<(), TrapCode> {
            result[0] = Value::I32(1);
            Ok(())
        },
        Some(100_000),
        None,
    );

    let mut result = [Value::I64(0)];
    assert_eq!(
        wasmtime_worker
            .execute("main", &[], &mut result)
            .unwrap_err(),
        TrapCode::BadSignature
    );
}

#[test]
fn test_wasmtime_raw_import_halt_is_a_controlled_exit() {
    let (module, import_linker) = get_test_numeric_marshalling_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            Err(TrapCode::ExecutionHalted)
        },
        Some(100_000),
        None,
    );

    let mut result = [Value::I64(0)];
    wasmtime_worker.execute("main", &[], &mut result).unwrap();
    assert_eq!(result[0], Value::I64(0));

    // The halted export never wrote its result, so the caller must not see the parameter bits
    // that are still in the shared slots.
    let mut result = [Value::I64(0)];
    wasmtime_worker
        .execute("mix_params", &[Value::I32(-1), Value::I64(5)], &mut result)
        .unwrap();
    assert_eq!(result[0], Value::I64(0));
}

#[test]
fn test_wasmtime_caller_fuel_accessors_use_engine_metering_when_enabled() {
    let (module, import_linker) = get_test_wasmtime_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            let before = caller.remaining_fuel().unwrap();
            caller.try_consume_fuel(1_000)?;
            assert_eq!(caller.remaining_fuel(), Some(before - 1_000));
            caller.reset_fuel(500);
            assert_eq!(caller.remaining_fuel(), Some(500));
            // Overspending the engine counter is reported, not saturated.
            assert_eq!(
                caller.try_consume_fuel(501).unwrap_err(),
                TrapCode::OutOfFuel
            );
            Ok(())
        },
        Some(100_000),
        None,
    );

    wasmtime_worker.execute("main", &[], &mut []).unwrap();
    // Only the instructions after the import call are charged against the reset budget.
    let remaining = wasmtime_worker.store.get_fuel().unwrap();
    assert!(
        (450..=500).contains(&remaining),
        "remaining fuel {remaining}"
    );
}

#[test]
fn test_wasmtime_caller_fuel_accessors_use_soft_counter_when_engine_metering_is_off() {
    let (module, import_linker) = get_test_module_without_engine_fuel();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        0u32,
        |caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            match *caller.data() {
                0 => {
                    assert_eq!(caller.remaining_fuel(), Some(1_000));
                    caller.try_consume_fuel(600)?;
                    assert_eq!(caller.remaining_fuel(), Some(400));
                    caller.reset_fuel(7);
                    assert_eq!(caller.remaining_fuel(), Some(7));
                    *caller.data_mut() = 1;
                    Ok(())
                }
                _ => caller.try_consume_fuel(8),
            }
        },
        Some(1_000),
        None,
    );

    wasmtime_worker.execute("probe", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.remaining_fuel(), Some(7));
    // The second probe overspends the soft counter, which surfaces as an out-of-fuel trap.
    assert_eq!(
        wasmtime_worker.execute("probe", &[], &mut []).unwrap_err(),
        TrapCode::OutOfFuel
    );
}

#[test]
fn test_wasmtime_executor_reports_instantiation_errors() {
    let (module, import_linker) = get_test_memory_module();
    let unlinked_module = Module::new(
        module.engine(),
        wat::parse_str(
            r#"
            (module
              (func $missing (import "missing" "import"))
              (func (export "main") (call $missing))
            )
            "#,
        )
        .unwrap(),
    )
    .unwrap();

    assert!(WasmtimeExecutor::try_new(
        unlinked_module.clone(),
        import_linker.clone(),
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    )
    .is_err());

    // A failed re-instantiation leaves the executor on its previous instance.
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    );
    assert!(wasmtime_worker.instantiate(&unlinked_module).is_err());
    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.data(), &[1, 2, 3, 4]);
}

#[test]
fn test_wasmtime_caller_writes_guest_memory_through_the_cached_handle() {
    let (module, import_linker) = get_test_memory_module();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::<u8>::new(),
        |caller, _sys_func_idx, params, _result| -> Result<(), TrapCode> {
            let offset = params[0].i32().unwrap() as usize;
            caller.memory_write(offset, &[9, 8, 7, 6])?;
            assert_eq!(
                caller.memory_write(N_BYTES_PER_MEMORY_PAGE as usize, &[1]),
                Err(TrapCode::MemoryOutOfBounds)
            );
            Ok(())
        },
        Some(100_000),
        None,
    );

    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(
        wasmtime_worker.memory_read_into_vec(0, 4).unwrap(),
        vec![9, 8, 7, 6]
    );
}

#[test]
fn test_wasmtime_raw_imports_return_float_results() {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $f32r (import "host" "f32r") (result f32))
              (func $f64r (import "host" "f64r") (result f64))
              (func (export "main") (result f64)
                (drop (call $f32r))
                (call $f64r)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("host", "f32r"),
        0x32,
        SyscallFuelParams::default(),
        &[],
        &[wasmparser::ValType::F32],
    );
    import_linker.insert_function(
        ImportName::new("host", "f64r"),
        0x64,
        SyscallFuelParams::default(),
        &[],
        &[wasmparser::ValType::F64],
    );
    let import_linker = Arc::new(import_linker);
    let compilation_config = CompilationConfig::default().with_import_linker(import_linker.clone());
    let module = compile_wasmtime_module(compilation_config, wasm_binary).unwrap();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        (),
        |_caller, sys_func_idx, _params, result| -> Result<(), TrapCode> {
            result[0] = match sys_func_idx {
                0x32 => Value::F32(crate::F32::from_bits(1.5f32.to_bits())),
                0x64 => Value::F64(crate::F64::from_bits((-2.25f64).to_bits())),
                _ => unreachable!(),
            };
            Ok(())
        },
        Some(100_000),
        None,
    );

    let mut result = [Value::F64(crate::F64::from_bits(0))];
    wasmtime_worker.execute("main", &[], &mut result).unwrap();
    assert_eq!(
        result[0],
        Value::F64(crate::F64::from_bits((-2.25f64).to_bits()))
    );
}

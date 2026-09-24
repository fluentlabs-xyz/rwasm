use crate::{
    wasmtime::{
        compile_wasmtime_module, compile_wasmtime_module_on, deserialize_wasmtime_module,
        WasmtimeExecutor, WasmtimeModule,
    },
    CompilationConfig, CompilationError, ImportLinker, ImportName, StoreTr, TrapCode, TypedCaller,
    Value, N_BYTES_PER_MEMORY_PAGE,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams, SyscallFuelParams};
use std::sync::Arc;
use wasmtime::Module;

const DIVISOR: u64 = 10;
const WORD_COST: u64 = 0;

fn get_test_wasmtime_module() -> (WasmtimeModule, Arc<ImportLinker>) {
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
    )
    .unwrap();

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
    )
    .unwrap();

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
    )
    .unwrap();

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
    )
    .unwrap();

    let err = wasmtime_worker
        .execute("missing_export", &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::UnknownExternalFunction);
}

/// A module loaded back from `wasmtime::Module::serialize` output carries the same syscall fuel
/// schedule as a freshly compiled one, so its imports are charged identically; `into_module`
/// hands the bare Wasmtime module back to callers that only need the compiled code.
#[test]
fn test_deserialized_module_keeps_its_syscall_fuel_schedule() {
    let (module, import_linker) = get_test_wasmtime_module();
    let compilation_config = CompilationConfig::default()
        .with_consume_fuel(true)
        .with_builtins_consume_fuel(true)
        .with_import_linker(import_linker.clone());
    let serialized = module.serialize().unwrap();
    // SAFETY: the bytes were produced by `serialize` on this very build a moment ago.
    let restored = unsafe { deserialize_wasmtime_module(compilation_config, &serialized) }.unwrap();
    assert_eq!(restored.syscall_fuel(), module.syscall_fuel());
    assert_eq!(restored.syscall_fuel().len(), 2);

    let mut wasmtime_worker = WasmtimeExecutor::new(
        restored,
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
        None,
    )
    .unwrap();
    wasmtime_worker.execute("main", &[], &mut []).unwrap();
    assert_eq!(
        wasmtime_worker.store.get_fuel().unwrap(),
        100_000 - (1 + 1 + 10 + 10 * 5 + 7)
    );

    let bare = module.into_module();
    assert!(bare.exports().any(|export| export.name() == "main"));
}

fn get_test_memory_module() -> (WasmtimeModule, Arc<ImportLinker>) {
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

fn get_test_module_without_memory() -> (WasmtimeModule, Arc<ImportLinker>) {
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
    )
    .unwrap();

    assert_eq!(
        wasmtime_worker
            .execute("read_missing_memory", &[], &mut [])
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
}

/// A module without memory snapshots as the zero-page memory the rwasm VM gives it.
#[test]
fn test_wasmtime_snapshot_missing_memory_is_empty() {
    let (module, import_linker) = get_test_module_without_memory();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker,
        Vec::new(),
        read_memory_syscall,
        Some(100_000),
        None,
    )
    .unwrap();

    assert_eq!(wasmtime_worker.snapshot_memory(), Ok(Vec::new()));
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
    )
    .unwrap();

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
    )
    .unwrap();

    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.data(), &[1, 2, 3, 4]);
    assert_eq!(
        wasmtime_worker
            .execute("read_oob", &[], &mut [])
            .unwrap_err(),
        TrapCode::MemoryOutOfBounds
    );
}

fn get_test_module_without_engine_fuel() -> (WasmtimeModule, Arc<ImportLinker>) {
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
    )
    .unwrap();

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
    )
    .unwrap();

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
    let module_without_memory = compile_wasmtime_module_on(
        memory_module.engine(),
        CompilationConfig::default().with_import_linker(import_linker.clone()),
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
    )
    .unwrap();
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

    // Instantiating the first module again brings its exports and memory back.
    let replaced = wasmtime_worker.instance();
    wasmtime_worker.instantiate(&memory_module).unwrap();
    assert_ne!(wasmtime_worker.instance(), replaced);
    wasmtime_worker.execute("read_ok", &[], &mut []).unwrap();
    assert_eq!(wasmtime_worker.data(), &[1, 2, 3, 4, 1, 2, 3, 4]);
    assert_eq!(
        wasmtime_worker.memory_read_into_vec(0, 4).unwrap(),
        vec![1, 2, 3, 4]
    );
}

fn get_test_numeric_marshalling_module() -> (WasmtimeModule, Arc<ImportLinker>) {
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
        WasmtimeExecutor::new(module, import_linker, (), mix_syscall, Some(100_000), None).unwrap();

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
        WasmtimeExecutor::new(module, import_linker, (), mix_syscall, Some(100_000), None).unwrap();

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
    )
    .unwrap();

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
    )
    .unwrap();

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
    )
    .unwrap();

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
    )
    .unwrap();

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
    // compiled against a linker that knows the import, instantiated with the one that does not
    let mut linking_linker = ImportLinker::default();
    linking_linker.insert_function(
        ImportName::new("missing", "import"),
        0xac,
        SyscallFuelParams::default(),
        &[],
        &[],
    );
    let unlinked_module = compile_wasmtime_module_on(
        module.engine(),
        CompilationConfig::default().with_import_linker(Arc::new(linking_linker)),
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
    )
    .unwrap();
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
    )
    .unwrap();

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
    )
    .unwrap();

    let mut result = [Value::F64(crate::F64::from_bits(0))];
    wasmtime_worker.execute("main", &[], &mut result).unwrap();
    assert_eq!(
        result[0],
        Value::F64(crate::F64::from_bits((-2.25f64).to_bits()))
    );
}

/// `compile_wasmtime_module` applies no entrypoint policy, so an export with a reference in its
/// signature reaches the executor. The reference marshalling used to exist in the `e2e` build
/// only, and the call hit `unreachable!` in every other build; it now reports the null reference
/// the rwasm VM reports for the same export, and passes a reference parameter through by
/// nullness (`funcref`) or by index (`externref`).
#[test]
fn reference_typed_export_reports_a_null_reference() {
    use crate::{always_failing_syscall_handler, ExternRef, FuncRef};
    let wasm = wat::parse_str(
        r#"(module
            (func (export "main") (result funcref) ref.null func)
            (func (export "numeric") (result i32) i32.const 7)
            (func (export "funcref_id") (param funcref) (result funcref) local.get 0)
            (func (export "externref_id") (param externref) (result externref) local.get 0)
            (func (export "non_null") (result funcref) ref.func 0))"#,
    )
    .unwrap();
    let module = compile_wasmtime_module(CompilationConfig::default(), wasm).unwrap();
    let mut executor = WasmtimeExecutor::new(
        module,
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        None,
    )
    .unwrap();
    let mut result = [Value::I32(0)];
    executor.execute("main", &[], &mut result).unwrap();
    assert_eq!(result, [Value::FuncRef(FuncRef::null())]);
    executor.execute("numeric", &[], &mut result).unwrap();
    assert_eq!(result, [Value::I32(7)]);
    executor
        .execute(
            "funcref_id",
            &[Value::FuncRef(FuncRef::null())],
            &mut result,
        )
        .unwrap();
    assert_eq!(result, [Value::FuncRef(FuncRef::null())]);
    for index in [0, 5] {
        executor
            .execute(
                "externref_id",
                &[Value::ExternRef(ExternRef::new(index))],
                &mut result,
            )
            .unwrap();
        assert_eq!(result, [Value::ExternRef(ExternRef::new(index))]);
    }
    // a result buffer of another length than the signature is the same mismatch as on the raw path
    assert_eq!(
        executor.execute("main", &[], &mut []),
        Err(TrapCode::IllegalOpcode)
    );
    // a non-null function reference has no counterpart on this backend: a parameter is refused
    // before the call, a result after it, and the buffer keeps what it held
    let mut result = [Value::I32(-1)];
    assert_eq!(
        executor.execute(
            "funcref_id",
            &[Value::FuncRef(FuncRef::new(3))],
            &mut result
        ),
        Err(TrapCode::IllegalOpcode)
    );
    assert_eq!(
        executor.execute("non_null", &[], &mut result),
        Err(TrapCode::IllegalOpcode)
    );
    assert_eq!(result, [Value::I32(-1)]);
}

/// The default value has no layout for a global type the rwasm compiler never admits. A module
/// compiled outside the rwasm front end, on an engine with SIMD, can import a `v128` global;
/// linking reports it instead of instantiating the module with a made-up value.
#[test]
fn imported_global_of_an_unsupported_type_is_a_linking_error() {
    use crate::always_failing_syscall_handler;
    let mut config = wasmtime::Config::new();
    config.wasm_simd(true);
    let engine = wasmtime::Engine::new(&config).unwrap();
    let wasm =
        wat::parse_str(r#"(module (import "env" "v" (global v128)) (func (export "main")))"#)
            .unwrap();
    let module = WasmtimeModule::new(
        Module::new(&engine, &wasm).unwrap(),
        &CompilationConfig::default().with_default_imported_global_value(7),
    );
    let err = WasmtimeExecutor::try_new(
        module,
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        None,
    )
    .err()
    .expect("linking must fail");
    assert!(
        err.to_string().contains("unsupported type `v128`"),
        "unexpected error: {err}"
    );
}

/// The store limiter follows the compile-time page cap of the module it runs, and a replacement
/// instantiated through `instantiate` brings its own cap.
#[test]
fn memory_grow_follows_the_compile_time_page_cap_of_the_live_module() {
    use crate::always_failing_syscall_handler;
    let wasm = wat::parse_str(
        r#"(module (memory (export "memory") 1)
            (func (export "main") (param i32) (result i32) (memory.grow (local.get 0))))"#,
    )
    .unwrap();
    let compile = |max_allowed_memory_pages: u32| {
        compile_wasmtime_module(
            CompilationConfig::default().with_max_allowed_memory_pages(max_allowed_memory_pages),
            &wasm,
        )
        .unwrap()
    };
    let grow = |executor: &mut WasmtimeExecutor<()>, delta: i32| {
        let mut result = [Value::I32(0)];
        executor
            .execute("main", &[Value::I32(delta)], &mut result)
            .unwrap();
        result[0].i32().unwrap()
    };
    let mut executor = WasmtimeExecutor::new(
        compile(2),
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        None,
    )
    .unwrap();
    assert_eq!(grow(&mut executor, 2), -1);
    assert_eq!(grow(&mut executor, 1), 1);
    assert_eq!(grow(&mut executor, 1), -1);

    // the replacement may grow to its own cap, the run-time cap still applies on top
    let replacement = compile_wasmtime_module_on(
        executor.store.engine(),
        CompilationConfig::default().with_max_allowed_memory_pages(4),
        &wasm,
    )
    .unwrap();
    executor.instantiate(&replacement).unwrap();
    assert_eq!(grow(&mut executor, 4), -1);
    assert_eq!(grow(&mut executor, 3), 1);
    assert_eq!(grow(&mut executor, 1), -1);
    let mut executor = WasmtimeExecutor::new(
        compile(4),
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        Some(3),
    )
    .unwrap();
    assert_eq!(grow(&mut executor, 3), -1);
    assert_eq!(grow(&mut executor, 2), 1);
}

/// A module whose instantiation fails must be reported as the trap the rwasm strategy raises for
/// it, never as a panic: the input reaching `WasmtimeExecutor::new` is not pre-validated against
/// the import linker or the store limits.
mod instantiation_failures {
    use super::*;
    use crate::always_failing_syscall_handler;

    fn compile(wat: &str, config: CompilationConfig) -> WasmtimeModule {
        compile_wasmtime_module(config, wat::parse_str(wat).unwrap()).unwrap()
    }

    fn instantiate(
        module: WasmtimeModule,
        max_allowed_memory_pages: Option<u32>,
    ) -> Result<(), TrapCode> {
        WasmtimeExecutor::new(
            module,
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            max_allowed_memory_pages,
        )
        .map(|_| ())
    }

    #[test]
    fn trapping_start_function_is_its_trap() {
        let module = compile(
            r#"(module (func $start unreachable) (start $start) (func (export "main")))"#,
            CompilationConfig::default().with_allow_start_section(true),
        );
        assert_eq!(
            instantiate(module, None),
            Err(TrapCode::UnreachableCodeReached)
        );
    }

    /// The module is compiled against a linker that knows the import (the rwasm front end
    /// rejects an unresolved import at compile time) and instantiated with one that does not.
    #[test]
    fn unresolved_import_is_unknown_external_function() {
        let mut import_linker = ImportLinker::default();
        import_linker.insert_function(
            ImportName::new("host", "missing"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[],
        );
        let module = compile(
            r#"(module (func (import "host" "missing")) (func (export "main")))"#,
            CompilationConfig::default().with_import_linker(Arc::new(import_linker)),
        );
        assert_eq!(
            instantiate(module, None),
            Err(TrapCode::UnknownExternalFunction)
        );
    }

    #[test]
    fn initial_memory_above_the_store_limit_is_memory_out_of_bounds() {
        let module = compile(
            r#"(module (memory 2) (func (export "main")))"#,
            CompilationConfig::default(),
        );
        assert_eq!(
            instantiate(module.clone(), Some(1)),
            Err(TrapCode::MemoryOutOfBounds)
        );
        // the same module instantiates once the store permits its initial memory
        assert_eq!(instantiate(module, Some(2)), Ok(()));
    }

    /// A refused grow inside the start function returns `-1` to the guest; a failure the function
    /// raises afterwards for its own reason must not be relabelled as the denial.
    #[test]
    fn handled_grow_denial_does_not_relabel_a_later_trap() {
        let module = compile(
            r#"(module
                (memory 1)
                (func $start
                    (drop (memory.grow (i32.const 16)))
                    unreachable)
                (start $start)
                (func (export "main")))"#,
            CompilationConfig::default().with_allow_start_section(true),
        );
        assert_eq!(
            instantiate(module, Some(1)),
            Err(TrapCode::UnreachableCodeReached)
        );
    }

    #[test]
    fn handled_grow_denial_does_not_relabel_a_host_error() {
        let wasm = wat::parse_str(
            r#"(module
                (import "host" "fail" (func $fail))
                (memory 1)
                (func $start
                    (drop (memory.grow (i32.const 16)))
                    call $fail)
                (start $start)
                (func (export "main")))"#,
        )
        .unwrap();
        let mut import_linker = ImportLinker::default();
        import_linker.insert_function(
            ImportName::new("host", "fail"),
            0x01,
            SyscallFuelParams::default(),
            &[],
            &[],
        );
        let import_linker = Arc::new(import_linker);
        let module = compile_wasmtime_module(
            CompilationConfig::default()
                .with_allow_start_section(true)
                .with_import_linker(import_linker.clone()),
            wasm,
        )
        .unwrap();
        let err = WasmtimeExecutor::new(
            module,
            import_linker,
            (),
            |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
                Err(TrapCode::OutOfFuel)
            },
            None,
            Some(1),
        )
        .err()
        .expect("the start function fails");
        assert_eq!(err, TrapCode::OutOfFuel);
    }

    #[test]
    fn try_new_keeps_the_trap_code_as_error_context() {
        let module = compile(
            r#"(module (memory 2) (func (export "main")))"#,
            CompilationConfig::default(),
        );
        let err = WasmtimeExecutor::try_new(
            module,
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            Some(1),
        )
        .err()
        .expect("instantiation must fail");
        assert_eq!(
            err.downcast_ref::<TrapCode>(),
            Some(&TrapCode::MemoryOutOfBounds)
        );
    }

    /// A replacement that fails to instantiate leaves the previous module's store limits in
    /// place: the executor installs the replacement's compile-time cap before instantiating and
    /// has to put the previous limits back.
    #[test]
    fn failed_replacement_keeps_the_previous_store_limits() {
        let mut executor = WasmtimeExecutor::new(
            compile(
                r#"(module (memory (export "memory") 1)
                    (func (export "main") (param i32) (result i32)
                        (memory.grow (local.get 0))))"#,
                CompilationConfig::default().with_max_allowed_memory_pages(2),
            ),
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            Some(3),
        )
        .unwrap();
        // five initial pages exceed the run-time cap of three; the replacement's own cap of
        // eight would let the live module grow to three pages if it stayed in place
        let replacement = compile_wasmtime_module_on(
            executor.store.engine(),
            CompilationConfig::default().with_max_allowed_memory_pages(8),
            wat::parse_str(r#"(module (memory (export "memory") 5) (func (export "main")))"#)
                .unwrap(),
        )
        .unwrap();
        let err = executor
            .instantiate(&replacement)
            .expect_err("instantiation must fail");
        assert_eq!(
            err.downcast_ref::<TrapCode>(),
            Some(&TrapCode::MemoryOutOfBounds)
        );
        let mut grow = |delta: i32| {
            let mut result = [Value::I32(0)];
            executor
                .execute("main", &[Value::I32(delta)], &mut result)
                .unwrap();
            result[0].i32().unwrap()
        };
        assert_eq!(grow(1), 1);
        assert_eq!(grow(1), -1, "the replacement's cap stayed in place");
    }
}

/// The Wasmtime store applies rwasm's table cap, so a module that reaches instantiation with an
/// oversized table (bypassing `compile_wasmtime_module`, e.g. through deserialization) fails with
/// the trap the rwasm prologue would raise.
#[test]
fn test_initial_table_above_the_cap_is_table_out_of_bounds() {
    use crate::{always_failing_syscall_handler, N_MAX_TABLE_SIZE};
    let engine = compile_wasmtime_module(
        CompilationConfig::default(),
        wat::parse_str("(module)").unwrap(),
    )
    .unwrap()
    .engine()
    .clone();
    let wasm = wat::parse_str(format!(
        r#"(module (table {} funcref) (func (export "main")))"#,
        N_MAX_TABLE_SIZE + 1
    ))
    .unwrap();
    // the engine still needs the frame height of the one (empty) function
    let wasm = super::with_frame_heights_section(&wasm, &[0]);
    let module = Module::new(&engine, &wasm).unwrap();
    let err = WasmtimeExecutor::new(
        module.into(),
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        None,
    )
    .err()
    .expect("instantiation must fail");
    assert_eq!(err, TrapCode::TableOutOfBounds);
}

/// The module cache keys on the compilation config as well as the caller's key: a module carries
/// the engine it was compiled with, so a cross-config hit would run on the first caller's fuel
/// schedule.
#[test]
fn test_module_cache_distinguishes_configs_under_one_key() {
    use crate::wasmtime::compile_wasmtime_module_cached;
    let wasm = wat::parse_str(r#"(module (func (export "main")))"#).unwrap();
    let key = [0x5a; 32];
    let metered = CompilationConfig::default().with_consume_fuel(true);
    let unmetered = CompilationConfig::default().with_consume_fuel(false);
    let metered_module = compile_wasmtime_module_cached(metered.clone(), &wasm, key).unwrap();
    let unmetered_module = compile_wasmtime_module_cached(unmetered, &wasm, key).unwrap();
    // a store meters fuel only if the module's engine was configured to
    let fuel_enabled =
        |module: &WasmtimeModule| wasmtime::Store::new(module.engine(), ()).get_fuel().is_ok();
    assert!(fuel_enabled(&metered_module));
    assert!(!fuel_enabled(&unmetered_module));
    // the same key with the same config is a cache hit
    let again = compile_wasmtime_module_cached(metered, &wasm, key).unwrap();
    assert!(wasmtime::Engine::same(
        metered_module.engine(),
        again.engine()
    ));
}

/// Syscall fuel is charged by the host trampoline, so every way of reaching an import pays it:
/// `call_indirect` and `return_call_indirect` through a table entry, an import exported as the
/// entrypoint and an import used as `start`. The engine used to charge it at Cranelift `call`
/// sites only, which left all of these free (audit round 5, R5-1).
#[test]
fn test_syscall_fuel_is_charged_on_every_dispatch_path() {
    const BASE: u64 = 1000;
    fn linker() -> Arc<ImportLinker> {
        let mut import_linker = ImportLinker::default();
        import_linker.insert_function(
            ImportName::new("env", "flat"),
            1,
            SyscallFuelParams::Const(BASE),
            &[],
            &[],
        );
        Arc::new(import_linker)
    }
    fn accept(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Ok(())
    }
    let cases = [
        ("call", "(func (export \"main\") call $flat)", false),
        (
            "call_indirect",
            "(func (export \"main\") (call_indirect (type $t) (i32.const 0)))",
            false,
        ),
        (
            "return_call_indirect",
            "(func (export \"main\") (return_call_indirect (type $t) (i32.const 0)))",
            false,
        ),
        (
            "ref.func + table.set",
            "(func (export \"main\") (table.set 0 (i32.const 1) (ref.func $flat)) \
             (call_indirect (type $t) (i32.const 1)))",
            false,
        ),
        ("export-of-import", "(export \"main\" (func $flat))", false),
        ("start", "(start $flat) (func (export \"main\"))", true),
    ];
    for (label, body, allow_start) in cases {
        let wasm = wat::parse_str(format!(
            r#"(module
              (type $t (func))
              (import "env" "flat" (func $flat))
              (table 2 funcref)
              (elem (i32.const 0) $flat)
              {body})"#
        ))
        .unwrap();
        let config = CompilationConfig::default()
            .with_consume_fuel(true)
            .with_builtins_consume_fuel(true)
            .with_allow_start_section(allow_start)
            .with_import_linker(linker());
        let module = compile_wasmtime_module(config, &wasm).unwrap();
        let mut executor =
            WasmtimeExecutor::new(module, linker(), (), accept, Some(100_000), None).unwrap();
        executor.execute("main", &[], &mut []).unwrap();
        let consumed = 100_000 - executor.remaining_fuel().unwrap();
        assert!(
            consumed >= BASE,
            "{label}: the syscall fuel must be charged, consumed only {consumed}"
        );
    }
}

/// The schedule belongs to the module: a bare `wasmtime::Module` charges no syscall fuel, and
/// re-instantiating a module compiled under another config swaps the schedule with it.
#[test]
fn test_syscall_fuel_schedule_follows_the_instantiated_module() {
    fn accept(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Ok(())
    }
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("env", "flat"),
        1,
        SyscallFuelParams::Const(1000),
        &[],
        &[],
    );
    let import_linker = Arc::new(import_linker);
    let wasm = wat::parse_str(
        r#"(module
          (import "env" "flat" (func $flat))
          (func (export "main") call $flat))"#,
    )
    .unwrap();
    let metered = compile_wasmtime_module(
        CompilationConfig::default()
            .with_consume_fuel(true)
            .with_builtins_consume_fuel(true)
            .with_import_linker(import_linker.clone()),
        &wasm,
    )
    .unwrap();
    // a store only instantiates modules of its own engine, so the variants share `metered`'s
    let unmetered = compile_wasmtime_module_on(
        metered.engine(),
        CompilationConfig::default()
            .with_consume_fuel(true)
            .with_builtins_consume_fuel(false)
            .with_import_linker(import_linker.clone()),
        &wasm,
    )
    .unwrap();
    let bare = WasmtimeModule::from(
        compile_wasmtime_module_on(
            metered.engine(),
            CompilationConfig::default()
                .with_consume_fuel(true)
                .with_builtins_consume_fuel(true)
                .with_import_linker(import_linker.clone()),
            &wasm,
        )
        .unwrap()
        .into_module(),
    );

    let consumed_by = |executor: &mut WasmtimeExecutor<()>| {
        executor.reset_fuel(100_000);
        executor.execute("main", &[], &mut []).unwrap();
        100_000 - executor.remaining_fuel().unwrap()
    };
    let mut executor = WasmtimeExecutor::new(
        metered.clone(),
        import_linker.clone(),
        (),
        accept,
        Some(100_000),
        None,
    )
    .unwrap();
    let with_schedule = consumed_by(&mut executor);
    assert!(with_schedule >= 1000, "metered module: {with_schedule}");

    executor.instantiate(&unmetered).unwrap();
    let without_schedule = consumed_by(&mut executor);
    assert_eq!(with_schedule - without_schedule, 1000, "unmetered module");

    executor.instantiate(&bare).unwrap();
    assert_eq!(consumed_by(&mut executor), without_schedule, "bare module");

    executor.instantiate(&metered).unwrap();
    assert_eq!(consumed_by(&mut executor), with_schedule, "metered again");

    // The replacement's schedule must be active before its imported start function runs.
    let start_wasm = wat::parse_str(
        r#"(module (import "env" "flat" (func $flat))
          (start $flat) (func (export "main") call $flat))"#,
    )
    .unwrap();
    for (previous, enabled, expected) in [(&unmetered, true, 1000), (&metered, false, 0)] {
        executor.instantiate(previous).unwrap();
        let replacement = compile_wasmtime_module_on(
            metered.engine(),
            CompilationConfig::default()
                .with_consume_fuel(true)
                .with_builtins_consume_fuel(enabled)
                .with_import_linker(import_linker.clone()),
            &start_wasm,
        )
        .unwrap();
        executor.reset_fuel(100_000);
        executor.instantiate(&replacement).unwrap();
        assert_eq!(100_000 - executor.remaining_fuel().unwrap(), expected);
    }

    // A failed start must restore the prior instance's schedule without refunding its charge.
    executor.instantiate(&unmetered).unwrap();
    let trapping_start = compile_wasmtime_module_on(
        metered.engine(),
        CompilationConfig::default()
            .with_consume_fuel(true)
            .with_builtins_consume_fuel(true)
            .with_import_linker(import_linker),
        wat::parse_str(
            r#"(module (import "env" "flat" (func $flat))
              (func $start call $flat unreachable) (start $start) (func (export "main")))"#,
        )
        .unwrap(),
    )
    .unwrap();
    executor.reset_fuel(100_000);
    assert!(executor.instantiate(&trapping_start).is_err());
    assert!(100_000 - executor.remaining_fuel().unwrap() >= 1000);
    assert_eq!(consumed_by(&mut executor), without_schedule);
}

/// A schedule whose metered parameter does not name an `i32` parameter of the import is refused
/// at compile time, as the rwasm compiler refuses the same linker entry, and again when the
/// executor is built for a module that reached it under such a schedule (a deserialized module,
/// or a module paired with the schedule after compilation).
#[test]
fn test_misaddressed_syscall_fuel_parameter_is_rejected_at_instantiation() {
    fn linker(schedule: SyscallFuelParams) -> Arc<ImportLinker> {
        let mut import_linker = ImportLinker::default();
        import_linker.insert_function(
            ImportName::new("env", "lin"),
            1,
            schedule,
            &[wasmparser::ValType::I32],
            &[],
        );
        Arc::new(import_linker)
    }
    fn config(import_linker: &Arc<ImportLinker>) -> CompilationConfig {
        CompilationConfig::default()
            .with_consume_fuel(true)
            .with_builtins_consume_fuel(true)
            .with_import_linker(import_linker.clone())
    }
    let misaddressed = linker(SyscallFuelParams::LinearFuel(LinearFuelParams {
        base_fuel: 0,
        param_index: 2,
        word_cost: 1,
    }));
    let wasm = wat::parse_str(
        r#"(module
          (import "env" "lin" (func $lin (param i32)))
          (func (export "main") (i32.const 0) (call $lin)))"#,
    )
    .unwrap();
    assert!(matches!(
        compile_wasmtime_module(config(&misaddressed), &wasm),
        Err(CompilationError::InvalidSyscallFuelParam)
    ));
    let compiled = compile_wasmtime_module(config(&linker(SyscallFuelParams::Const(1))), &wasm)
        .unwrap()
        .into_module();
    let module = WasmtimeModule::new(compiled, &config(&misaddressed));
    let err = WasmtimeExecutor::new(
        module,
        misaddressed,
        (),
        crate::always_failing_syscall_handler,
        Some(100_000),
        None,
    )
    .err()
    .expect("the executor must not be built");
    assert_eq!(err, TrapCode::BadSignature);
}

/// The engine's `max_wasm_stack` must hold every frame the rwasm compiler accepts. The compiler
/// bounds a frame by `N_MAX_STACK_SIZE` 32-bit slots and the engine used to be sized as that many
/// bytes times four, but Cranelift spills each live value into an 8-byte slot, so a single
/// function whose live operands filled more than about half of the rwasm window was executed by
/// the rwasm VM and trapped `StackOverflow` at entry on this backend (audit 2026-09-18).
mod native_frame_size {
    use super::*;
    use crate::{always_failing_syscall_handler, ExecutionEngine, RwasmModule, RwasmStore};

    /// `n` i32 locals loaded from memory (so nothing folds), all live until the final sum.
    fn live_frame(n: usize) -> Vec<u8> {
        let mut body = String::new();
        for i in 0..n {
            body.push_str(&format!(
                "(local.set {i} (i32.load (i32.const {})))\n",
                (i * 4) % N_BYTES_PER_MEMORY_PAGE as usize
            ));
        }
        body.push_str("(i32.const 0)\n");
        for i in 0..n {
            body.push_str(&format!("(local.get {i}) (i32.add)\n"));
        }
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 0) "\01\00\00\00")
                (func (export "main") (result i32) (local {locals})
                  {body}))"#,
            locals = "i32 ".repeat(n)
        ))
        .unwrap()
    }

    /// Compiles `wasm` for the Wasmtime backend and runs `main` there.
    fn run(wasm: &[u8]) -> Result<Value, TrapCode> {
        let config = CompilationConfig::default().with_entrypoint_name("main".into());
        let module = compile_wasmtime_module(config, wasm).unwrap();
        let mut executor = WasmtimeExecutor::new(
            module,
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            None,
        )?;
        let mut result = [Value::I32(0)];
        executor.execute("main", &[], &mut result)?;
        Ok(result[0].clone())
    }

    /// `f(depth)` keeps `live` i32 locals (loaded from memory, so not folded) alive across its
    /// recursive call, so a chain of `depth + 1` frames spills `live` values per frame. Returns
    /// the number of frames that saw a non-zero first local, i.e. `depth + 1` when `live > 0`.
    fn call_chain(live: usize, depth: u32) -> Vec<u8> {
        let mut sets = String::new();
        let mut uses = String::new();
        for i in 0..live {
            sets.push_str(&format!(
                "(local.set {} (i32.load (i32.const {})))\n",
                i + 1,
                (i * 4) % N_BYTES_PER_MEMORY_PAGE as usize
            ));
            uses.push_str(&format!("(local.get {}) (i32.add)\n", i + 1));
        }
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 0) "\01\00\00\00")
                (func $f (param i32) (result i32) (local {locals})
                  {sets}
                  (if (result i32) (i32.eqz (local.get 0))
                    (then (i32.const 0))
                    (else (call $f (i32.sub (local.get 0) (i32.const 1)))))
                  {uses})
                (func (export "main") (result i32) (call $f (i32.const {depth}))))"#,
            locals = "i32 ".repeat(live)
        ))
        .unwrap()
    }

    /// Runs `main` on the rwasm VM; the chains below must be accepted there before they mean
    /// anything for the Wasmtime backend.
    fn run_on_rwasm(wasm: &[u8]) -> Result<Value, TrapCode> {
        let config = CompilationConfig::default().with_entrypoint_name("main".into());
        let module = RwasmModule::compile(config, wasm)
            .expect("the module is within the compiler's limits")
            .0;
        let linker = Arc::new(ImportLinker::default());
        let mut store = RwasmStore::<()>::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            None,
            None,
        );
        let instance = linker.instantiate(&mut store, ExecutionEngine::new(), module)?;
        let mut result = [Value::I32(0)];
        instance.execute(&mut store, &[], &mut result)?;
        Ok(result[0].clone())
    }

    /// The deepest call chains the rwasm VM accepts (up to `N_MAX_RECURSION_DEPTH` frames, or
    /// the value-stack window, whichever is reached first) must fit the native stack too: the
    /// per-frame allowance in `WASMTIME_MAX_WASM_STACK` is what pays for their fixed frame cost.
    #[test]
    fn an_accepted_call_chain_fits_the_native_stack() {
        let mut failures = Vec::new();
        // (live i32 locals per frame, recursion depth): the deepest chain, one with a few live
        // values per frame, and wider frames that fill the rwasm window at a lower depth
        for (live, depth) in [(0usize, 1023u32), (3, 1023), (7, 700), (15, 400), (60, 120)] {
            let wasm = call_chain(live, depth);
            let expected = Value::I32(if live > 0 { depth as i32 + 1 } else { 0 });
            assert_eq!(
                run_on_rwasm(&wasm),
                Ok(expected.clone()),
                "{live} live locals x {depth} frames: the rwasm VM accepts the chain"
            );
            let outcome = run(&wasm);
            if outcome != Ok(expected) {
                failures.push(format!("{live} live locals x {depth} frames: {outcome:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "a call chain the rwasm VM runs must run on the Wasmtime backend:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn an_accepted_frame_fits_the_native_stack() {
        let mut failures = Vec::new();
        for n in [4000usize, 6000, 8000] {
            let wasm = live_frame(n);
            // the rwasm compiler accepts the frame and its VM runs it
            let config = CompilationConfig::default().with_entrypoint_name("main".into());
            let module = RwasmModule::compile(config, &wasm)
                .expect("the frame is within the compiler's limit")
                .0;
            let linker = Arc::new(ImportLinker::default());
            let mut store = RwasmStore::<()>::new(
                linker.clone(),
                (),
                always_failing_syscall_handler,
                None,
                None,
            );
            let instance = linker
                .instantiate(&mut store, ExecutionEngine::new(), module)
                .unwrap();
            let mut result = [Value::I32(0)];
            instance.execute(&mut store, &[], &mut result).unwrap();
            assert_eq!(result[0], Value::I32(1), "{n} locals: rwasm VM");

            let outcome = run(&wasm);
            if outcome != Ok(Value::I32(1)) {
                failures.push(format!("{n} live i32 locals: {outcome:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "a frame the compiler accepts must run on the Wasmtime backend:\n{}",
            failures.join("\n")
        );
    }
}

/// The frame heights the Wasmtime backend checks come from the rwasm translator, attached to the
/// binary as the `rwasm.frames` custom section: one `u32` per function, imports first.
#[test]
fn frame_heights_section_records_every_function() {
    use crate::{ModuleParser, N_SYSCALL_FUEL_PROLOGUE_SLOTS};
    use wasmparser::{Parser, Payload};
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("env", "quad"),
        1,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 1,
            divisor: 1,
            fuel_denom_rate: 1,
        }),
        &[wasmparser::ValType::I32],
        &[],
    );
    let wasm = wat::parse_str(
        r#"(module
          (import "env" "quad" (func $quad (param i32)))
          (func (export "main") (local i32 i64) (i32.const 1) (i64.const 2) (i64.const 3)
            (drop (i64.add)) (call $quad))
          (func $leaf))"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_import_linker(Arc::new(import_linker))
        .with_builtins_consume_fuel(true);
    let mut parser = ModuleParser::new(config.clone());
    parser.parse(&wasm).unwrap();
    // the trampoline's temporaries; `main`: three local slots plus one i32 and two i64 operands;
    // `leaf`: nothing
    let heights = parser.frame_heights();
    assert_eq!(heights, [N_SYSCALL_FUEL_PROLOGUE_SLOTS as u32, 3 + 5, 0]);

    let binary = super::with_frame_heights_section(&wasm, &heights);
    let mut found = None;
    for payload in Parser::new(0).parse_all(&binary) {
        if let Payload::CustomSection(section) = payload.unwrap() {
            if section.name() == wasmtime::RWASM_FRAMES_SECTION {
                found = Some(section.data().to_vec());
            }
        }
    }
    let expected: Vec<u8> = heights.iter().flat_map(|h| h.to_le_bytes()).collect();
    assert_eq!(found.as_deref(), Some(expected.as_slice()));
    // the section changes nothing else: the module still compiles and runs
    let module = compile_wasmtime_module(config, &wasm).unwrap();
    assert_eq!(module.module().imports().count(), 1);
}

/// The frames the engine assumes behind an `i64` operator are the snippets the rwasm compiler
/// emits: the same frame count and the same `StackCheck`.
#[test]
fn snippet_frames_match_the_snippet_definitions() {
    use crate::compiler::snippets::Snippet;
    use wasmparser::Operator;
    let table = [
        (Operator::I64Eq, Snippet::I64Eq),
        (Operator::I64Ne, Snippet::I64Ne),
        (Operator::I64LtS, Snippet::I64LtS),
        (Operator::I64LtU, Snippet::I64LtU),
        (Operator::I64GtS, Snippet::I64GtS),
        (Operator::I64GtU, Snippet::I64GtU),
        (Operator::I64LeS, Snippet::I64LeS),
        (Operator::I64LeU, Snippet::I64LeU),
        (Operator::I64GeS, Snippet::I64GeS),
        (Operator::I64GeU, Snippet::I64GeU),
        (Operator::I64Add, Snippet::I64Add),
        (Operator::I64Sub, Snippet::I64Sub),
        (Operator::I64Mul, Snippet::I64Mul),
        (Operator::I64DivS, Snippet::I64DivS),
        (Operator::I64DivU, Snippet::I64DivU),
        (Operator::I64RemS, Snippet::I64RemS),
        (Operator::I64RemU, Snippet::I64RemU),
        (Operator::I64Shl, Snippet::I64Shl),
        (Operator::I64ShrS, Snippet::I64ShrS),
        (Operator::I64ShrU, Snippet::I64ShrU),
        (Operator::I64Rotl, Snippet::I64RotL),
        (Operator::I64Rotr, Snippet::I64RotR),
    ];
    let mut mismatches = Vec::new();
    for (op, snippet) in &table {
        let frames = wasmtime::rwasm_snippet_frames(op);
        let expected = wasmtime::RwasmSnippetFrames {
            frames: 1 + snippet.dependencies().len() as u32,
            max_stack_height: snippet.max_stack_height(),
        };
        if frames != Some(expected) {
            mismatches.push(format!("{op:?}: {frames:?}, rwasm emits {expected:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    // every snippet an operator can reach is in the table; the core is only called by them
    let covered: Vec<Snippet> = table.iter().map(|(_, snippet)| *snippet).collect();
    for snippet in Snippet::ALL {
        assert!(
            covered.contains(&snippet) || snippet == Snippet::UDivMod64,
            "{snippet:?} is not covered"
        );
    }
    // operators the compiler inlines push no frame
    for op in [
        Operator::I64And,
        Operator::I64Clz,
        Operator::I32Add,
        Operator::I64Eqz,
    ] {
        assert_eq!(wasmtime::rwasm_snippet_frames(&op), None, "{op:?}");
    }
}

/// The Wasmtime compile path runs the rwasm translator for the frame heights, so a frame the
/// rwasm compiler rejects is rejected here as well, with the same error.
#[test]
fn oversized_frame_is_rejected_on_the_wasmtime_path() {
    let wasm = wat::parse_str(format!(
        r#"(module (func (export "main") (result i32) {} (i32.const 42)))"#,
        "(local i32)".repeat(crate::N_MAX_STACK_SIZE)
    ))
    .unwrap();
    let outcome = compile_wasmtime_module(CompilationConfig::default(), &wasm).map(|_| ());
    assert!(
        matches!(
            outcome,
            Err(CompilationError::StackHeightExceeded {
                height: 8193,
                limit: 8192
            })
        ),
        "{outcome:?}"
    );
}

mod imported_globals {
    //! The globals a module imports with `default_imported_global_value` are module state: they
    //! link through a copy of the executor's linker, so they neither replace a host function of
    //! the same name for the modules instantiated afterwards nor survive into a replacement whose
    //! config defines no default.

    use super::*;
    use crate::{always_failing_syscall_handler, ValType};

    fn host_linker() -> Arc<ImportLinker> {
        let mut import_linker = ImportLinker::default();
        import_linker.insert_function(
            ImportName::new("env", "f"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[ValType::I32],
        );
        Arc::new(import_linker)
    }

    fn answer_42(
        _caller: &mut TypedCaller<'_, ()>,
        _sys_func_idx: u32,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I32(42);
        Ok(())
    }

    fn main_result(executor: &mut WasmtimeExecutor<()>) -> Result<i32, TrapCode> {
        let mut result = [Value::I32(0)];
        executor.execute("main", &[], &mut result)?;
        Ok(result[0].i32().unwrap())
    }

    /// A global import carrying the name of a host function links, the global winning for that
    /// module as on the rwasm strategy, and a module instantiated afterwards still reaches the
    /// host function under that name.
    #[test]
    fn a_global_named_like_a_host_function_does_not_replace_it() {
        let import_linker = host_linker();
        let with_global = compile_wasmtime_module(
            CompilationConfig::default()
                .with_import_linker(import_linker.clone())
                .with_default_imported_global_value(7),
            wat::parse_str(
                r#"(module (import "env" "f" (global i32))
                    (func (export "main") (result i32) global.get 0))"#,
            )
            .unwrap(),
        )
        .unwrap();
        let mut executor = WasmtimeExecutor::new(
            with_global,
            import_linker.clone(),
            (),
            answer_42,
            None,
            None,
        )
        .unwrap();
        assert_eq!(main_result(&mut executor), Ok(7));

        let calls_function = compile_wasmtime_module_on(
            executor.store.engine(),
            CompilationConfig::default().with_import_linker(import_linker),
            wat::parse_str(
                r#"(module (import "env" "f" (func (result i32)))
                    (func (export "main") (result i32) call 0))"#,
            )
            .unwrap(),
        )
        .unwrap();
        executor.instantiate(&calls_function).unwrap();
        assert_eq!(main_result(&mut executor), Ok(42));
    }

    /// A linker definition of another type than the import is `UnknownExternalFunction`, as the
    /// linker's own resolution reported it: the module was compiled against a host function
    /// `env.f: () -> i32` and is instantiated with a linker whose `env.f` takes an `i32`.
    #[test]
    fn a_definition_of_another_type_is_unknown_external_function() {
        let module = compile_wasmtime_module(
            CompilationConfig::default().with_import_linker(host_linker()),
            wat::parse_str(
                r#"(module (import "env" "f" (func (result i32)))
                    (func (export "main") (result i32) call 0))"#,
            )
            .unwrap(),
        )
        .unwrap();
        let mut other_linker = ImportLinker::default();
        other_linker.insert_function(
            ImportName::new("env", "f"),
            1,
            SyscallFuelParams::default(),
            &[ValType::I32],
            &[],
        );
        let err = WasmtimeExecutor::new(
            module,
            Arc::new(other_linker),
            (),
            always_failing_syscall_handler,
            None,
            None,
        )
        .err()
        .expect("the import must not resolve");
        assert_eq!(err, TrapCode::UnknownExternalFunction);
    }

    /// Imports that get no global of their own resolve against the executor's linker whatever
    /// their kind: a global has to match in type and mutability, a memory is left to Wasmtime's
    /// check. Modules of the rwasm language never import these (the compiler rejects them), so a
    /// bare module on a plain engine exercises the path.
    #[test]
    fn linker_globals_and_memories_resolve_by_kind() {
        use wasmtime::{Global, GlobalType, Memory, MemoryType, Mutability, Val};
        let engine = wasmtime::Engine::new(&wasmtime::Config::new()).unwrap();
        let module = |wat: &str| {
            WasmtimeModule::from(Module::new(&engine, wat::parse_str(wat).unwrap()).unwrap())
        };
        let mut executor = WasmtimeExecutor::new(
            module(r#"(module (func (export "main")))"#),
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            None,
        )
        .unwrap();
        let global = Global::new(
            &mut executor.store,
            GlobalType::new(wasmtime::ValType::I32, Mutability::Const),
            Val::I32(5),
        )
        .unwrap();
        executor
            .linker
            .define(&mut executor.store, "env", "g", global)
            .unwrap();
        let memory = Memory::new(&mut executor.store, MemoryType::new(1, None)).unwrap();
        executor
            .linker
            .define(&mut executor.store, "env", "m", memory)
            .unwrap();

        executor
            .instantiate(&module(
                r#"(module (import "env" "g" (global i32)) (import "env" "m" (memory 1))
                    (func (export "main") (result i32) global.get 0))"#,
            ))
            .unwrap();
        assert_eq!(main_result(&mut executor), Ok(5));

        // a global of another type or mutability does not resolve, nor does a function import
        // under the name of a global
        for wat in [
            r#"(module (import "env" "g" (global i64)) (func (export "main")))"#,
            r#"(module (import "env" "g" (global (mut i32))) (func (export "main")))"#,
            r#"(module (import "env" "g" (func)) (func (export "main")))"#,
        ] {
            let err = executor
                .instantiate(&module(wat))
                .expect_err("the global must not resolve");
            assert_eq!(
                err.downcast_ref::<TrapCode>(),
                Some(&TrapCode::UnknownExternalFunction),
                "{wat}"
            );
        }
        // the previous instance is still the live one
        assert_eq!(main_result(&mut executor), Ok(5));
    }

    /// A replacement compiled without a default for imported globals does not inherit the global
    /// the previous module defined under the same name: its import stays unresolved.
    #[test]
    fn a_replacement_without_a_default_does_not_inherit_a_global() {
        let wasm = wat::parse_str(
            r#"(module (import "env" "g" (global i32))
                (func (export "main") (result i32) global.get 0))"#,
        )
        .unwrap();
        let with_default = compile_wasmtime_module(
            CompilationConfig::default().with_default_imported_global_value(7),
            &wasm,
        )
        .unwrap();
        let mut executor = WasmtimeExecutor::new(
            with_default,
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            None,
        )
        .unwrap();
        assert_eq!(main_result(&mut executor), Ok(7));

        // the same code under a config without a default, on the executor's engine
        let without_default = WasmtimeModule::new(
            compile_wasmtime_module_on(
                executor.store.engine(),
                CompilationConfig::default().with_default_imported_global_value(7),
                &wasm,
            )
            .unwrap()
            .into_module(),
            &CompilationConfig::default(),
        );
        let err = executor
            .instantiate(&without_default)
            .expect_err("the global import must stay unresolved");
        assert_eq!(
            err.downcast_ref::<TrapCode>(),
            Some(&TrapCode::UnknownExternalFunction)
        );
        // the previous instance is still the live one
        assert_eq!(main_result(&mut executor), Ok(7));
    }
}

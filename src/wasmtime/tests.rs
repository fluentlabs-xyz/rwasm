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

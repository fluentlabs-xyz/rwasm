use crate::{
    wasmtime::{compile_wasmtime_module, WasmtimeExecutor},
    CompilationConfig, ImportLinker, ImportName, TrapCode,
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
    let words = (300 + 31) / 32;
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

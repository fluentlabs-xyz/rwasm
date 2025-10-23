use fluent_rwasm::{
    always_failing_syscall_handler, intrinsic::Intrinsic, CompilationConfig, ExecutionEngine,
    FuelConfig, ImportLinker, ImportName, Opcode, RwasmModule, RwasmStore,
};
use std::sync::Arc;
use wasmparser::ValType;

#[test]
fn test_intrinsic_replace() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (import "env" "consume_fuel" (func $consume_fuel (param i32)))

  (func (export "call_gas")
    i32.const 35
    call $consume_fuel
  )
)
"#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_intrinsic(
        ImportName::new("env", "consume_fuel"),
        71,
        Intrinsic::Replace(vec![Opcode::ConsumeFuelStack]),
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);

    let config = CompilationConfig::default()
        .with_entrypoint_name("call_gas".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker)
        .with_consume_fuel(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::new(
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        FuelConfig::default().with_fuel_limit(100),
    );
    let engine = ExecutionEngine::default();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    // 35 by consume_fuel and 4 by other opcodes
    assert_eq!(store.fuel_consumed(), 35 + 4);
}

#[test]
fn test_intrinsic_remove() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (import "env" "consume_fuel" (func $consume_fuel (param i32)))

  (func (export "call_gas")
    i32.const 33
    call $consume_fuel
  )
)
"#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_intrinsic(
        ImportName::new("env", "consume_fuel"),
        71,
        Intrinsic::Remove,
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);

    let config = CompilationConfig::default()
        .with_entrypoint_name("call_gas".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker)
        .with_consume_fuel(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::new(
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        FuelConfig::default(),
    );
    let engine = ExecutionEngine::default();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
}

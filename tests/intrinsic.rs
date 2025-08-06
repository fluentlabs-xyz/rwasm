use rwasm::intrinsic::Intrinsic;
use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, ExecutorConfig,
    ImportLinker, ImportName, Opcode, RwasmModule, RwasmStore,
};
use std::rc::Rc;
use wasmparser::ValType;

#[test]
fn test_intrinsic_replace() {
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
        Intrinsic::Replace(vec![Opcode::ConsumeFuelStack]),
        &[ValType::I32],
        &[],
    );
    let import_linker = Rc::new(import_linker);

    let config = CompilationConfig::default()
        .with_entrypoint_name("call_gas".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker)
        .with_consume_fuel(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::new(
        ExecutorConfig::default()
            .fuel_enabled(true)
            .fuel_limit(1000),
        Rc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
    );
    let mut engine = ExecutionEngine::new();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    assert_eq!(store.fuel_consumed(), 33);
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
    let import_linker = Rc::new(import_linker);

    let config = CompilationConfig::default()
        .with_entrypoint_name("call_gas".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker)
        .with_consume_fuel(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::new(
        ExecutorConfig::default()
            .fuel_enabled(true)
            .fuel_limit(1000),
        Rc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
    );
    let mut engine = ExecutionEngine::new();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
}

#![cfg(feature = "wasmtime")]

use rwasm::{
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmStore, StoreTr,
    StrategyDefinition, SyscallFuelParams, TrapCode, TypedCaller, ValType, Value,
};
use std::sync::Arc;

#[test]
fn pending_execution_survives_rejected_replacement_and_reset_cancels_it() {
    fn interrupt(
        caller: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        caller.memory_write(0, &[42])?;
        Err(TrapCode::InterruptionCalled)
    }
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "pause"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    let linker = Arc::new(linker);
    let wasm = wat::parse_str(
        r#"(module
        (import "env" "pause" (func $pause))
        (memory 1)
        (func (export "main") (result i32) call $pause i32.const 0 i32.load8_u))"#,
    )
    .unwrap();
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_import_linker(linker.clone());
    let module = RwasmModule::compile(config, &wasm).unwrap().0;
    let engine = ExecutionEngine::new();
    for keep_flags in [false, true] {
        let mut store = RwasmStore::new(linker.clone(), (), interrupt, Some(1000), None);
        let instance = linker
            .instantiate(&mut store, engine, module.clone())
            .unwrap();
        let mut result = [Value::I32(0)];
        assert_eq!(
            instance.execute(&mut store, &[], &mut result),
            Err(TrapCode::InterruptionCalled)
        );
        assert_eq!(
            instance.execute(&mut store, &[], &mut result),
            Err(TrapCode::IllegalOpcode)
        );
        assert_eq!(
            engine.entrypoint(&mut store, &module),
            Err(TrapCode::IllegalOpcode)
        );
        assert_eq!(
            linker.instantiate(&mut store, engine, module.clone()).err(),
            Some(TrapCode::IllegalOpcode)
        );
        instance.resume(&mut store, &[], &mut result).unwrap();
        assert_eq!(
            result,
            [Value::I32(42)],
            "failed replacement must preserve the parked memory"
        );
        assert_eq!(
            instance.execute(&mut store, &[], &mut result),
            Err(TrapCode::InterruptionCalled)
        );
        store.reset(keep_flags);
        assert_eq!(
            instance.resume(&mut store, &[], &mut result),
            Err(TrapCode::IllegalOpcode)
        );
        assert!(linker
            .instantiate(&mut store, engine, module.clone())
            .is_ok());
    }
}

#[test]
fn untouched_mixed_numeric_syscall_results_are_typed_zeroes() {
    fn leave_results(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Ok(())
    }
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "zeroes"),
        1,
        SyscallFuelParams::None,
        &[],
        &[ValType::I32, ValType::I64, ValType::F32, ValType::F64],
    );
    let linker = Arc::new(linker);
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_import_linker(linker.clone());
    let wasm = wat::parse_str(
        r#"(module
        (import "env" "zeroes" (func $zeroes (result i32 i64 f32 f64)))
        (func (export "main") (result i32 i64 f32 f64) call $zeroes))"#,
    )
    .unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config.clone(), &wasm).unwrap(),
        StrategyDefinition::new_as_wasmtime(config, &wasm, None).unwrap(),
    ] {
        let mut executor = definition
            .create_executor(linker.clone(), (), leave_results, Some(1000), None)
            .unwrap();
        let expected = [
            Value::default(ValType::I32),
            Value::default(ValType::I64),
            Value::default(ValType::F32),
            Value::default(ValType::F64),
        ];
        let mut result = expected.clone();
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result, expected);
    }
}

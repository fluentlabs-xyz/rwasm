use rwasm::{
    for_each_strategy, CompilationConfig, ImportLinker, ImportName, StoreTr, StrategyDefinition,
    TrapCode, TypedCaller, Value,
};
use std::sync::Arc;
use wasmparser::ValType;

fn linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("host", "call"),
        0,
        Default::default(),
        &[],
        &[],
    );
    Arc::new(linker)
}

fn definitions(wat: &str, linker: &Arc<ImportLinker>) -> Vec<StrategyDefinition> {
    for_each_strategy(
        Ok,
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(linker.clone()),
        &wat::parse_str(wat).unwrap(),
    )
    .unwrap()
}

fn count_calls(
    caller: &mut TypedCaller<'_, usize>,
    _: u32,
    _: &[Value],
    _: &mut [Value],
) -> Result<(), TrapCode> {
    *caller.data_mut() += 1;
    Ok(())
}

#[test]
fn invalid_host_signatures_are_rejected_before_execution() {
    let linker = linker();
    let cases = [
        ("i32 i32", vec![Value::I64(0)], "i32", vec![Value::I32(7)]),
        ("i64", vec![Value::I32(0); 2], "i32", vec![Value::I32(7)]),
        (
            "i32",
            vec![Value::F32(0.0.into())],
            "i32",
            vec![Value::I32(7)],
        ),
        (
            "i32 i64",
            vec![Value::I64(0), Value::I32(0)],
            "i32",
            vec![Value::I32(7)],
        ),
        ("", vec![], "i32 i32", vec![Value::I64(7)]),
        ("", vec![], "i64", vec![const { Value::I32(7) }; 2]),
        ("", vec![], "i32", vec![]),
    ];
    for (param_types, params, result_types, results) in cases {
        // Unreachable permits every declared result signature without fabricating results.
        // Signature errors must take precedence over both the host callback and this trap.
        let wat = format!(
            r#"(module (import "host" "call" (func $call))
                (func (export "main") (param {param_types}) (result {result_types})
                    call $call unreachable))"#
        );
        for definition in definitions(&wat, &linker) {
            let mut executor = definition
                .create_executor(linker.clone(), 0, count_calls, Some(10_000), None)
                .unwrap();
            let fuel = executor.remaining_fuel();
            let mut output = results.clone();
            assert_eq!(
                executor.execute("main", &params, &mut output),
                Err(TrapCode::IllegalOpcode),
                "params={param_types}, results={result_types}"
            );
            assert_eq!(output, results);
            assert_eq!(*executor.data(), 0);
            assert_eq!(executor.remaining_fuel(), fuel);
        }
    }
}

#[test]
fn result_placeholders_are_overwritten_with_the_declared_types() {
    let linker = linker();
    let wat = r#"(module
        (func (export "main") (param i32 i64 f32 f64) (result i32 i64 f32 f64)
            local.get 0 local.get 1 local.get 2 local.get 3))"#;
    let params = [
        Value::I32(0x1122),
        Value::I64(0x1122334455667788),
        Value::F32(1.25.into()),
        Value::F64(3.5.into()),
    ];
    for definition in definitions(wat, &linker) {
        let mut executor = definition.default_executor().unwrap();
        let mut results = [const { Value::I32(-1) }; 4];
        executor.execute("main", &params, &mut results).unwrap();
        assert_eq!(results, params);
        // Correctly typed buffers still work when the same instance is called again.
        executor.execute("main", &params, &mut results).unwrap();
        assert_eq!(results, params);
    }
}

#[test]
fn imported_entrypoint_uses_its_declared_signature() {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("host", "call"),
        0,
        Default::default(),
        &[ValType::I64],
        &[ValType::I64],
    );
    let linker = Arc::new(linker);
    let wat = r#"(module (import "host" "call" (func $call (param i64) (result i64)))
        (export "main" (func $call)))"#;
    for definition in definitions(wat, &linker) {
        let mut executor = definition
            .create_executor(
                linker.clone(),
                (),
                |_, _, params, results| {
                    results.clone_from_slice(params);
                    Ok(())
                },
                None,
                None,
            )
            .unwrap();
        let mut results = [Value::I32(-1)];
        assert_eq!(
            executor.execute("main", &[Value::I32(1), Value::I32(2)], &mut results),
            Err(TrapCode::IllegalOpcode)
        );
        executor
            .execute("main", &[Value::I64(42)], &mut results)
            .unwrap();
        assert_eq!(results, [Value::I64(42)]);
    }
}

#[test]
fn halt_zeroes_declared_results_and_traps_preserve_placeholders() {
    let linker = linker();
    let wat = r#"(module (import "host" "call" (func $call))
        (func (export "main") (result i64 f64) call $call unreachable))"#;
    for definition in definitions(wat, &linker) {
        for trap in [TrapCode::ExecutionHalted, TrapCode::IntegerOverflow] {
            let mut executor = definition
                .create_executor(
                    linker.clone(),
                    trap,
                    |caller, _, _, _| Err(*caller.data()),
                    None,
                    None,
                )
                .unwrap();
            let mut results = [const { Value::I32(7) }; 2];
            let outcome = executor.execute("main", &[], &mut results);
            if trap == TrapCode::ExecutionHalted {
                assert_eq!(outcome, Ok(()));
                assert_eq!(results, [Value::I64(0), Value::F64(0.0.into())]);
            } else {
                assert_eq!(outcome, Err(trap));
                assert_eq!(results, [const { Value::I32(7) }; 2]);
            }
        }
    }
}

#[test]
fn invalid_resume_output_preserves_the_parked_execution() {
    let linker = linker();
    let wat = r#"(module (import "host" "call" (func $call))
        (func (export "main") (result i64) call $call i64.const 42))"#;
    let definition = definitions(wat, &linker).remove(0);
    let mut executor = definition
        .create_executor(
            linker,
            (),
            |_, _, _, _| Err(TrapCode::InterruptionCalled),
            Some(10_000),
            None,
        )
        .unwrap();
    let mut results = [Value::I32(7)];
    assert_eq!(
        executor.execute("main", &[], &mut results),
        Err(TrapCode::InterruptionCalled)
    );
    assert_eq!(results, [Value::I32(7)]);
    let fuel = executor.remaining_fuel();
    assert_eq!(
        executor.resume(&[], &mut [const { Value::I32(7) }; 2]),
        Err(TrapCode::IllegalOpcode)
    );
    assert_eq!(executor.remaining_fuel(), fuel);
    executor.resume(&[], &mut results).unwrap();
    assert_eq!(results, [Value::I64(42)]);
}

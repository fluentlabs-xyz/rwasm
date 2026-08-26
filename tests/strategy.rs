use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ImportLinker, ImportName,
    QuadraticFuelParams, RwasmModule, RwasmStore, StoreTr, StrategyDefinition, StrategyExecutor,
    SyscallFuelParams, TrapCode, TypedCaller, TypedStore, Value, N_BYTES_PER_MEMORY_PAGE,
};
use std::sync::Arc;
use wasmparser::ValType;

const STRATEGY_WAT: &str = r#"
    (module
        (memory (export "memory") 1)
        (table 2 funcref)
        (global (mut i32) (i32.const 9))
        (func $main (export "main") (result i32)
            i32.const 42)
        (elem (i32.const 0) $main)
    )
"#;

/// Returns a configuration accepted by both execution strategies.
fn strategy_config() -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
}

/// Checks the common store contract through a strategy-neutral type.
fn exercise_store<T: StoreTr<u32>>(store: &mut T) {
    assert_eq!(store.data(), &7);
    *store.data_mut() = 8;
    assert_eq!(store.data(), &8);

    store.memory_write(4, &[1, 2, 3, 4]).unwrap();
    let mut buffer = [0_u8; 4];
    store.memory_read(4, &mut buffer).unwrap();
    assert_eq!(buffer, [1, 2, 3, 4]);
    assert_eq!(store.memory_read_into_vec(5, 2).unwrap(), [2, 3]);

    store.reset_fuel(100);
    assert_eq!(store.remaining_fuel(), Some(100));
    store.try_consume_fuel(9).unwrap();
    assert_eq!(store.remaining_fuel(), Some(91));
    assert_eq!(store.try_consume_fuel(92), Err(TrapCode::OutOfFuel));
    store.reset_fuel(50);
    assert_eq!(store.remaining_fuel(), Some(50));
}

/// Checks execution and memory snapshots through a strategy executor.
fn exercise_executor(mut executor: StrategyExecutor<u32>) {
    exercise_store(&mut executor);
    let mut result = [Value::I32(0)];
    executor.execute("main", &[], &mut result).unwrap();
    assert_eq!(result, [Value::I32(42)]);
    let snapshot = executor.snapshot_memory().unwrap();
    assert_eq!(&snapshot[4..8], &[1, 2, 3, 4]);
}

/// Checks Wasmtime caller delegation from inside an imported host call.
fn exercise_wasmtime_caller(
    caller: &mut TypedCaller<u32>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    assert_eq!(caller.as_wasmtime_ref().data(), &7);
    *caller.as_wasmtime_mut().data_mut() = 8;
    caller.memory_write(4, &[1, 2, 3, 4])?;
    let mut buffer = [0_u8; 4];
    caller.memory_read(4, &mut buffer)?;
    assert_eq!(buffer, [1, 2, 3, 4]);
    assert_eq!(caller.memory_read_into_vec(5, 2)?, [2, 3]);
    caller.reset_fuel(100);
    assert_eq!(caller.remaining_fuel(), Some(100));
    caller.try_consume_fuel(9)?;
    assert_eq!(caller.remaining_fuel(), Some(91));
    caller.reset_fuel(50);
    assert_eq!(caller.remaining_fuel(), Some(50));
    Ok(())
}

/// Covers strategy constructors and executor delegation for both engines.
#[test]
fn strategy_definitions_and_executors_delegate_store_operations() {
    let wasm = wat::parse_str(STRATEGY_WAT).unwrap();
    let import_linker = Arc::new(ImportLinker::default());

    let rwasm = StrategyDefinition::new_as_rwasm(strategy_config(), &wasm).unwrap();
    exercise_executor(
        rwasm
            .create_executor(
                import_linker.clone(),
                7,
                always_failing_syscall_handler,
                Some(100),
                Some(1),
            )
            .unwrap(),
    );

    let wasmtime = StrategyDefinition::new_as_wasmtime(strategy_config(), &wasm, None).unwrap();
    exercise_executor(
        wasmtime
            .create_executor(
                import_linker.clone(),
                7,
                always_failing_syscall_handler,
                Some(100),
                Some(1),
            )
            .unwrap(),
    );

    let cached =
        StrategyDefinition::new_as_wasmtime(strategy_config(), &wasm, Some([7; 32])).unwrap();
    assert!(matches!(cached, StrategyDefinition::Wasmtime { .. }));
    let default = StrategyDefinition::new(strategy_config(), &wasm, None).unwrap();
    assert!(matches!(default, StrategyDefinition::Wasmtime { .. }));

    let executor = StrategyExecutor::compile_and_instantiate(
        strategy_config(),
        &wasm,
        None,
        import_linker,
        7,
        always_failing_syscall_handler,
        Some(100),
    )
    .unwrap();
    assert!(matches!(executor, StrategyExecutor::Wasmtime { .. }));
}

/// Covers rWasm store snapshots, reset behavior, and typed delegation.
#[test]
fn typed_store_delegates_to_rwasm_store() {
    let wasm = wat::parse_str(STRATEGY_WAT).unwrap();
    let (module, _) = RwasmModule::compile(strategy_config(), &wasm).unwrap();
    let import_linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(
        import_linker.clone(),
        7,
        always_failing_syscall_handler,
        Some(100),
        Some(1),
    );
    let instance = import_linker
        .instantiate(&mut store, rwasm::ExecutionEngine::new(), module)
        .unwrap();

    assert_eq!(store.memory_size_bytes(), N_BYTES_PER_MEMORY_PAGE as usize);
    assert_eq!(store.memory_snapshot_prefix(4), [0; 4]);
    assert_eq!(
        store.memory_snapshot().len(),
        N_BYTES_PER_MEMORY_PAGE as usize
    );
    assert_eq!(store.table_snapshots_nullness_prefix(1), [(0, 2, vec![1])]);
    assert!(store.has_global_word(0));
    assert_eq!(store.global_word_bits(0), 9);
    store.try_consume_fuel(10).unwrap();
    assert_eq!(store.fuel_consumed(), 10);
    store.reset(false);
    assert_eq!(store.fuel_consumed(), 0);

    let mut result = [Value::I32(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    assert_eq!(result, [Value::I32(42)]);

    let mut store = TypedStore::Rwasm(store);
    exercise_store(&mut store);
}

/// Covers Wasmtime typed-store and typed-caller delegation.
#[test]
fn typed_store_and_caller_delegate_to_wasmtime() {
    let wasm = wat::parse_str(STRATEGY_WAT).unwrap();
    let definition = StrategyDefinition::new_as_wasmtime(strategy_config(), &wasm, None).unwrap();
    let executor = definition
        .create_executor(
            Arc::new(ImportLinker::default()),
            7,
            always_failing_syscall_handler,
            Some(100),
            Some(1),
        )
        .unwrap();
    let StrategyExecutor::Wasmtime { executor } = executor else {
        panic!("expected wasmtime executor");
    };
    exercise_store(&mut TypedStore::Wasmtime(executor));

    let wasm = wat::parse_str(
        r#"
            (module
                (func (import "host" "call"))
                (memory (export "memory") 1)
                (func (export "main")
                    call 0)
            )
        "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("host", "call"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    let import_linker = Arc::new(import_linker);
    let config = CompilationConfig::default_strategy_compatible()
        .with_import_linker(import_linker.clone())
        .with_entrypoint_name("main".into());
    let definition = StrategyDefinition::new_as_wasmtime(config, &wasm, None).unwrap();
    let mut executor = definition
        .create_executor(
            import_linker,
            7,
            exercise_wasmtime_caller,
            Some(100),
            Some(1),
        )
        .unwrap();
    executor.execute("main", &[], &mut []).unwrap();
    assert_eq!(executor.data(), &8);
}

/// Covers constant and quadratic syscall-fuel block compilation.
#[test]
fn compiler_emits_constant_and_quadratic_syscall_fuel_blocks() {
    let wasm = wat::parse_str(
        r#"
            (module
                (func (import "fuel" "constant"))
                (func (import "fuel" "quadratic") (param i32))
                (func (export "main")
                    call 0
                    i32.const 64
                    call 1)
            )
        "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("fuel", "constant"),
        1,
        SyscallFuelParams::Const(3),
        &[],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fuel", "quadratic"),
        2,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 2,
            divisor: 4,
            fuel_denom_rate: 3,
        }),
        &[ValType::I32],
        &[],
    );
    let config = CompilationConfig::default()
        .with_builtins_consume_fuel(true)
        .with_import_linker(Arc::new(import_linker))
        .with_entrypoint_name("main".into());

    RwasmModule::compile(config, &wasm).unwrap();
}

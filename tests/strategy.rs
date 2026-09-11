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

/// Both strategies must accept exactly the rwasm language and report a rejected binary as an
/// error. The Wasmtime path used to compile through Wasmtime alone and `expect` the result, so
/// anything rwasm rejects but Wasmtime accepts slipped through, and anything Wasmtime rejected
/// panicked.
mod accepted_language {
    use super::*;
    use rwasm::CompilationError;

    /// A valid header followed by garbage.
    const MALFORMED: &[u8] = b"\0asm\x01\0\0\0\xff\xff\xff\xff";

    /// Wasmtime accepts SIMD; rwasm does not translate it.
    const SIMD_WAT: &str = r#"
        (module
            (func (export "main")
                (drop (v128.const i32x4 0 0 0 0))))
    "#;

    const START_WAT: &str = r#"
        (module
            (func $start)
            (start $start)
            (func (export "main")))
    "#;

    #[test]
    fn malformed_binary_is_an_error_on_every_constructor() {
        assert!(matches!(
            StrategyDefinition::new_as_rwasm(strategy_config(), MALFORMED),
            Err(CompilationError::MalformedWasmBinary(_))
        ));
        assert!(matches!(
            StrategyDefinition::new_as_wasmtime(strategy_config(), MALFORMED, None),
            Err(CompilationError::MalformedWasmBinary(_))
        ));
        assert!(matches!(
            StrategyDefinition::new(strategy_config(), MALFORMED, None),
            Err(CompilationError::MalformedWasmBinary(_))
        ));
        assert!(matches!(
            StrategyExecutor::compile_and_instantiate(
                strategy_config(),
                MALFORMED,
                None,
                Arc::new(ImportLinker::default()),
                (),
                always_failing_syscall_handler,
                None,
            ),
            Err(rwasm::StrategyError::CompilationError(
                CompilationError::MalformedWasmBinary(_)
            ))
        ));
    }

    #[test]
    fn wasmtime_strategy_rejects_what_rwasm_rejects() {
        let simd = wat::parse_str(SIMD_WAT).unwrap();
        let rwasm_err = StrategyDefinition::new_as_rwasm(strategy_config(), &simd)
            .err()
            .expect("rwasm rejects SIMD");
        let wasmtime_err = StrategyDefinition::new_as_wasmtime(strategy_config(), &simd, None)
            .err()
            .expect("the Wasmtime strategy must reject SIMD too");
        assert!(
            matches!(
                rwasm_err,
                CompilationError::NotSupportedOpcode
                    | CompilationError::NotSupportedExtension
                    | CompilationError::MalformedWasmBinary(_)
            ),
            "unexpected error: {rwasm_err:?}"
        );
        assert_eq!(format!("{rwasm_err:?}"), format!("{wasmtime_err:?}"));

        let start = wat::parse_str(START_WAT).unwrap();
        assert!(matches!(
            StrategyDefinition::new_as_rwasm(strategy_config(), &start),
            Err(CompilationError::StartSectionsAreNotAllowed)
        ));
        assert!(matches!(
            StrategyDefinition::new_as_wasmtime(strategy_config(), &start, Some([9; 32])),
            Err(CompilationError::StartSectionsAreNotAllowed)
        ));
        // the rejection is not cached under the key
        assert!(matches!(
            StrategyDefinition::new_as_wasmtime(strategy_config(), &start, Some([9; 32])),
            Err(CompilationError::StartSectionsAreNotAllowed)
        ));

        let missing_entrypoint = wat::parse_str("(module)").unwrap();
        assert!(matches!(
            StrategyDefinition::new_as_wasmtime(strategy_config(), &missing_entrypoint, None),
            Err(CompilationError::MissingEntrypoint)
        ));
    }

    /// `compile_wasmtime_module_cached` validates with Wasmtime only. A module it primed under a
    /// key must not satisfy `new_as_wasmtime` under the same key, or the constructor's rwasm
    /// validation could be skipped.
    #[test]
    fn wasmtime_only_cache_entries_do_not_bypass_rwasm_validation() {
        let start = wat::parse_str(START_WAT).unwrap();
        let key = [0x42; 32];
        rwasm::wasmtime::compile_wasmtime_module_cached(strategy_config(), &start, key)
            .expect("Wasmtime accepts a start section");
        assert!(matches!(
            StrategyDefinition::new_as_wasmtime(strategy_config(), &start, Some(key)),
            Err(CompilationError::StartSectionsAreNotAllowed)
        ));
    }

    #[test]
    fn for_each_strategy_reports_compile_errors() {
        let result = rwasm::for_each_strategy(|_| Ok(()), strategy_config(), MALFORMED);
        assert!(matches!(
            result,
            Err(rwasm::StrategyError::CompilationError(
                CompilationError::MalformedWasmBinary(_)
            ))
        ));
    }
}

/// A config enabling the rwasm-only fuel injections charges different fuel on the two strategies,
/// so the strategy-agnostic constructors reject it instead of silently under-metering on Wasmtime.
/// The differential check pins that the strategy-compatible default charges identical fuel on the
/// audit's counter-example (a function with locals doing a large `memory.fill`).
mod strategy_compatibility {
    use super::*;
    use rwasm::{for_each_strategy, CompilationError, StrategyError};

    const FILL_WAT: &str = r#"
        (module
            (memory 1)
            (func (export "main") (result i32)
                (local i32 i32 i32 i32 i64 i64 f32 f64)
                i32.const 0
                i32.const 42
                i32.const 60000
                memory.fill
                local.get 0
                i64.const 7
                local.set 4
                local.get 4
                i32.wrap_i64
                i32.add))
    "#;

    fn incompatible() -> CompilationConfig {
        CompilationConfig::default()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
    }

    #[test]
    fn strategy_agnostic_constructors_reject_incompatible_configs() {
        let wasm = wat::parse_str(FILL_WAT).unwrap();
        assert!(!incompatible().is_strategy_compatible());
        assert!(matches!(
            StrategyDefinition::new(incompatible(), &wasm, None),
            Err(CompilationError::StrategyIncompatibleConfig)
        ));
        assert!(matches!(
            StrategyDefinition::new_as_wasmtime(incompatible(), &wasm, Some([3; 32])),
            Err(CompilationError::StrategyIncompatibleConfig)
        ));
        assert!(matches!(
            for_each_strategy(|_| Ok(()), incompatible(), &wasm),
            Err(StrategyError::CompilationError(
                CompilationError::StrategyIncompatibleConfig
            ))
        ));
        // each flag alone is enough to diverge
        for config in [
            strategy_config().with_consume_fuel_for_bulk_ops(true),
            strategy_config().with_consume_fuel_for_params_and_locals(true),
        ] {
            assert!(matches!(
                StrategyDefinition::new(config, &wasm, None),
                Err(CompilationError::StrategyIncompatibleConfig)
            ));
        }
        // the rwasm VM implements the injections, so the explicit rwasm constructor accepts them
        StrategyDefinition::new_as_rwasm(incompatible(), &wasm).unwrap();
        // and the compatible default is accepted everywhere
        StrategyDefinition::new(strategy_config(), &wasm, None).unwrap();
    }

    #[test]
    fn strategy_compatible_default_charges_identical_fuel() {
        let wasm = wat::parse_str(FILL_WAT).unwrap();
        let outcomes = for_each_strategy(
            |strategy| {
                let mut executor = strategy.create_executor(
                    Arc::new(ImportLinker::default()),
                    (),
                    always_failing_syscall_handler,
                    Some(1_000_000),
                    None,
                )?;
                let fuel_before = executor.remaining_fuel().unwrap();
                let mut result = [Value::I32(0)];
                let trap = executor.execute("main", &[], &mut result).err();
                let fuel_consumed = fuel_before - executor.remaining_fuel().unwrap();
                Ok((trap, result[0].clone(), fuel_consumed))
            },
            strategy_config(),
            &wasm,
        )
        .unwrap();
        assert!(outcomes.len() >= 2, "both strategies must run");
        assert_eq!(outcomes[0].0, None);
        assert_eq!(outcomes[0].1, Value::I32(7));
        assert!(outcomes[0].2 > 0);
        for outcome in &outcomes[1..] {
            assert_eq!(outcome, &outcomes[0]);
        }
    }
}

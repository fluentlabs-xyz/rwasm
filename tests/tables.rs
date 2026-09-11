//! Tables that a module references but never grows must behave as zero-length tables instead of
//! panicking the interpreter.

use rwasm::{
    always_failing_syscall_handler, instruction_set, ExecutionEngine, ImportLinker, InstructionSet,
    RwasmModuleBuilder, RwasmStore, TrapCode, Value,
};

/// The index of a table that no test module ever grows.
const UNGROWN_TABLE: u16 = 3;

fn module(code_section: InstructionSet, elem_section: &[u32]) -> rwasm::RwasmModule {
    RwasmModuleBuilder::new(code_section)
        .with_elem_section(elem_section)
        .build()
}

fn execute(code_section: InstructionSet, result: &mut [Value]) -> Result<(), TrapCode> {
    let module = module(code_section, &[]);
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    engine.execute(&mut store, &module, &[], result)
}

fn execute_and_trap(code_section: InstructionSet) -> TrapCode {
    execute(code_section, &mut []).expect_err("execution must trap")
}

#[test]
fn test_table_size_of_ungrown_table_is_zero() {
    let mut result = [Value::I32(-1)];
    execute(
        instruction_set! {
            TableSize(UNGROWN_TABLE)
            Return
        },
        &mut result,
    )
    .unwrap();
    assert_eq!(result[0].i32(), Some(0));
}

#[test]
fn test_table_get_of_ungrown_table_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0)
        TableGet(UNGROWN_TABLE)
        Drop
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_table_set_of_ungrown_table_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0) // index
        I32Const(1) // value
        TableSet(UNGROWN_TABLE)
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_table_fill_of_ungrown_table_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0) // d
        I32Const(0) // val
        I32Const(1) // n
        TableFill(UNGROWN_TABLE)
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_empty_table_fill_of_ungrown_table_succeeds() {
    execute(
        instruction_set! {
            I32Const(0) // d
            I32Const(0) // val
            I32Const(0) // n
            TableFill(UNGROWN_TABLE)
            Return
        },
        &mut [],
    )
    .unwrap();
}

#[test]
fn test_table_copy_of_ungrown_tables_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0) // d
        I32Const(0) // s
        I32Const(1) // n
        .op_table_copy(UNGROWN_TABLE, UNGROWN_TABLE + 1)
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_empty_table_copy_of_ungrown_tables_succeeds() {
    execute(
        instruction_set! {
            I32Const(0) // d
            I32Const(0) // s
            I32Const(0) // n
            .op_table_copy(UNGROWN_TABLE, UNGROWN_TABLE + 1)
            Return
        },
        &mut [],
    )
    .unwrap();
}

#[test]
fn test_table_copy_within_ungrown_table_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0) // d
        I32Const(0) // s
        I32Const(1) // n
        .op_table_copy(UNGROWN_TABLE, UNGROWN_TABLE)
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_table_init_into_ungrown_table_traps() {
    let module = module(
        instruction_set! {
            I32Const(0) // d
            I32Const(0) // s
            I32Const(1) // n
            TableInit(1)
            TableGet(UNGROWN_TABLE) // table index payload
            Return
        },
        &[0],
    );
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    let trap_code = engine
        .execute(&mut store, &module, &[], &mut [])
        .expect_err("execution must trap");
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_call_indirect_through_ungrown_table_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0) // func index
        CallIndirect(0)
        TableGet(UNGROWN_TABLE) // table index payload
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_return_call_indirect_through_ungrown_table_traps() {
    let trap_code = execute_and_trap(instruction_set! {
        I32Const(0) // func index
        ReturnCallIndirect(0)
        TableGet(UNGROWN_TABLE) // table index payload
        Return
    });
    assert_eq!(trap_code, TrapCode::TableOutOfBounds);
}

#[test]
fn test_grown_table_still_reports_its_size() {
    let mut result = [Value::I32(-1)];
    execute(
        instruction_set! {
            I32Const(0) // init
            I32Const(2) // delta
            TableGrow(UNGROWN_TABLE)
            Drop
            TableSize(UNGROWN_TABLE)
            Return
        },
        &mut result,
    )
    .unwrap();
    assert_eq!(result[0].i32(), Some(2));
}

/// Tables are capped at `N_MAX_TABLE_SIZE` elements on both strategies: a larger declaration is a
/// compile error on every compile path, a declaration at the cap works, and a runtime `table.grow`
/// past the cap fails identically. Without the compile-time check the rwasm prologue's `table.grow`
/// silently failed, leaving the rwasm VM with an empty table where Wasmtime had the declared one.
#[cfg(feature = "wasmtime")]
mod table_size_cap {
    use rwasm::{
        always_failing_syscall_handler, for_each_strategy, wasmtime::compile_wasmtime_module,
        CompilationConfig, CompilationError, RwasmModule, StrategyDefinition, StrategyError, Value,
        N_MAX_TABLE_SIZE,
    };

    fn config() -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
    }

    fn table_module(initial: u32) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
                (table {initial} funcref)
                (func (export "main") (param i32) (result i32)
                    local.get 0
                    i32.eqz
                    if (result i32)
                        table.size 0
                    else
                        ref.null func
                        local.get 0
                        table.grow 0
                    end))"#
        ))
        .unwrap()
    }

    /// Runs `main(param)` on every strategy and returns the per-strategy results.
    fn run_on_each_strategy(wasm: &[u8], param: i32) -> Result<Vec<i32>, StrategyError> {
        for_each_strategy(
            |strategy| {
                let mut executor = strategy.create_executor(
                    Default::default(),
                    (),
                    always_failing_syscall_handler,
                    Some(1_000_000),
                    None,
                )?;
                let mut result = [Value::I32(0)];
                executor.execute("main", &[Value::I32(param)], &mut result)?;
                Ok(result[0].i32().unwrap())
            },
            config(),
            wasm,
        )
    }

    #[test]
    fn table_above_the_cap_is_rejected_on_every_compile_path() {
        let wasm = table_module(N_MAX_TABLE_SIZE + 1);
        let expected = |err: &CompilationError| {
            matches!(
                err,
                CompilationError::TableSizeExceedsLimit { size, limit }
                    if *size == N_MAX_TABLE_SIZE + 1 && *limit == N_MAX_TABLE_SIZE
            )
        };
        assert!(expected(
            &RwasmModule::compile(config(), &wasm).unwrap_err()
        ));
        assert!(expected(
            &compile_wasmtime_module(config(), &wasm).unwrap_err()
        ));
        assert!(expected(
            &StrategyDefinition::new_as_wasmtime(config(), &wasm, None)
                .err()
                .expect("the Wasmtime strategy must reject the table")
        ));
        assert!(matches!(
            run_on_each_strategy(&wasm, 0),
            Err(StrategyError::CompilationError(err)) if expected(&err)
        ));
    }

    #[test]
    fn table_at_the_cap_has_the_declared_size_on_every_strategy() {
        let sizes = run_on_each_strategy(&table_module(N_MAX_TABLE_SIZE), 0).unwrap();
        assert!(sizes.len() >= 2, "both strategies must run");
        assert!(sizes.iter().all(|size| *size == N_MAX_TABLE_SIZE as i32));
    }

    #[test]
    fn table_grow_past_the_cap_fails_on_every_strategy() {
        let wasm = table_module(1000);
        let room = (N_MAX_TABLE_SIZE - 1000) as i32;
        let grown = run_on_each_strategy(&wasm, room).unwrap();
        assert!(grown.iter().all(|old_size| *old_size == 1000));
        let refused = run_on_each_strategy(&wasm, room + 1).unwrap();
        assert!(refused.iter().all(|result| *result == -1));
    }
}

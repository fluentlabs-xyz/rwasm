//! Tables that a module references but never grows must behave as zero-length tables instead of
//! panicking the interpreter.

use rwasm::{
    always_failing_syscall_handler, instruction_set, ExecutionEngine, ImportLinker, InstructionSet,
    RwasmModule, RwasmModuleBuilder, RwasmStore, TrapCode, Value,
};

/// The index of a table that no test module ever grows.
const UNGROWN_TABLE: u16 = 3;

fn verified_module(code_section: InstructionSet, elem_section: &[u32]) -> RwasmModule {
    let module = RwasmModuleBuilder::new(code_section)
        .with_elem_section(elem_section)
        .build();
    RwasmModule::new_verified_exact(&module.serialize()).expect("module must pass verification")
}

fn execute(code_section: InstructionSet, result: &mut [Value]) -> Result<(), TrapCode> {
    let module = verified_module(code_section, &[]);
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
    let module = verified_module(
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

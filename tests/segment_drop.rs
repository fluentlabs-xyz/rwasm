use rwasm::{
    always_failing_syscall_handler, instruction_set, ExecutionEngine, ImportLinker, InstructionSet,
    RwasmModuleBuilder, RwasmStore, TrapCode,
};

fn execute(code_section: InstructionSet, data: &[u8], elem: &[u32]) -> Result<(), TrapCode> {
    let module = RwasmModuleBuilder::new(code_section)
        .with_data_section(data)
        .with_elem_section(elem)
        .build();
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    engine
        .execute(&mut store, &module, &[], &mut [])
        .map(|_| ())
}

fn data_drop_code(dropped: &[u32]) -> InstructionSet {
    let mut code = instruction_set! {
        I32Const(1)
        MemoryGrow
        Drop
    };
    for segment in dropped {
        code.op_data_drop(*segment);
    }
    code.op_i32_const(0); // d
    code.op_i32_const(0); // s
    code.op_i32_const(1); // n
    code.op_memory_init(5);
    code.op_return();
    code
}

fn elem_drop_code(dropped: &[u32]) -> InstructionSet {
    let mut code = instruction_set! {
        I32Const(0) // init
        I32Const(1) // delta
        TableGrow(0)
        Drop
    };
    for segment in dropped {
        code.op_elem_drop(*segment);
    }
    code.op_i32_const(0); // d
    code.op_i32_const(0); // s
    code.op_i32_const(1); // n
    code.op_table_init(5);
    code.op_table_get(0); // table index payload of `table.init`
    code.op_return();
    code
}

#[test]
fn test_data_drop_keeps_higher_dropped_segments() {
    let data = &[0xaa, 0xbb, 0xcc, 0xdd];
    // not dropped at all, `memory.init` reads the real data section
    assert_eq!(execute(data_drop_code(&[]), data, &[]), Ok(()));
    // dropped, `memory.init` must observe a zero-length segment and trap
    assert_eq!(
        execute(data_drop_code(&[5]), data, &[]),
        Err(TrapCode::MemoryOutOfBounds),
    );
    // dropping a lower-indexed segment afterward must not resurrect segment 5
    assert_eq!(
        execute(data_drop_code(&[5, 2]), data, &[]),
        Err(TrapCode::MemoryOutOfBounds),
    );
}

#[test]
fn test_elem_drop_keeps_higher_dropped_segments() {
    let elem = &[1u32, 2, 3, 4];
    // not dropped at all, `table.init` reads the real element section
    assert_eq!(execute(elem_drop_code(&[]), &[], elem), Ok(()));
    // dropped, `table.init` must observe a zero-length segment and trap
    assert_eq!(
        execute(elem_drop_code(&[5]), &[], elem),
        Err(TrapCode::TableOutOfBounds),
    );
    // dropping a lower-indexed segment afterward must not resurrect segment 5
    assert_eq!(
        execute(elem_drop_code(&[5, 2]), &[], elem),
        Err(TrapCode::TableOutOfBounds),
    );
}

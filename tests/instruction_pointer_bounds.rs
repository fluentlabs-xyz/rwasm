//! Bytecode that the compiler never emits must not crash or hang the interpreter.
//!
//! `RwasmModuleBuilder` and `InstructionSet` are safe public API, and `RwasmModule::new_checked`
//! decodes modules that were not produced in-process. Displacements (branch offsets, call targets,
//! table payloads, `source_pc`) are therefore validated at run time: they trap instead of reading
//! memory outside the code section, panicking inside the executor, or sizing an allocation from an
//! immediate.

use rwasm::{
    always_failing_syscall_handler, instruction_set, BranchOffset, ExecutionEngine, ImportLinker,
    RwasmModule, RwasmModuleBuilder, RwasmStore, TrapCode,
};

fn execute(module: &RwasmModule) -> Result<(), TrapCode> {
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    engine.execute(&mut store, module, &[], &mut [])
}

fn execute_mut(module: &RwasmModule) -> Result<(), TrapCode> {
    execute(module)
}

/// A branch far outside the code section used to move the raw instruction pointer out of the
/// section and dereference it (SIGSEGV in both debug and release builds).
#[test]
fn branch_outside_the_code_section_traps() {
    // The code section holds three instructions, so every offset below leaves it. A small
    // offset such as `-1` stays inside and is legitimate control flow instead.
    for offset in [i32::MAX, i32::MIN, 1_000_000, -1_000_000, -4] {
        let module = RwasmModuleBuilder::new(instruction_set! {
            I32Const(0)
            Drop
            Br(offset)
        })
        .build();
        assert_eq!(
            execute_mut(&module),
            Err(TrapCode::UnreachableCodeReached),
            "branch offset {offset} must trap"
        );
    }
}

/// The same module decoded from its own encoding (the documented round trip) must trap too.
#[test]
fn decoded_module_with_an_out_of_range_branch_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        I32Const(1)
        Br(BranchOffset::from(i32::MAX))
    })
    .build();
    let bytes = module.serialize();
    let (decoded, _) = RwasmModule::new_checked(&bytes).expect("the encoding is valid");
    assert_eq!(
        execute_mut(&decoded),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// `targets as usize - 1` used to underflow: a panic with overflow checks, a wrapped clamp plus an
/// 8 GiB jump without them.
#[test]
fn br_table_with_zero_targets_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(5)
        BrTable(0)
        Return
    })
    .build();
    assert_eq!(
        execute_mut(&module),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// A syscall index the linker cannot resolve used to panic the interpreter.
#[test]
fn unresolved_syscall_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        Call(0x1234_5678u32)
        Return
    })
    .build();
    assert_eq!(
        execute_mut(&module),
        Err(TrapCode::UnknownExternalFunction)
    );
}

/// `CallIndirect`/`TableInit` carry their table index in the payload word that follows them; a
/// module without that word used to hit `unreachable!` in the executor.
#[test]
fn missing_table_index_payload_traps() {
    let call_indirect = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(0)
        CallIndirect(0)
        Return
    })
    .build();
    assert_eq!(
        execute_mut(&call_indirect),
        Err(TrapCode::UnreachableCodeReached)
    );

    let table_init = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(0)
        I32Const(0)
        I32Const(0)
        TableInit(1)
        Return
    })
    .build();
    assert_eq!(
        execute_mut(&table_init),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// An empty code section used to fetch an opcode through the dangling pointer of an empty `Vec`.
#[test]
fn empty_code_section_traps() {
    assert_eq!(
        execute_mut(&RwasmModule::empty()),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// A code section without a terminator used to walk off the end of the section.
#[test]
fn code_section_without_terminator_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! { I32Const(1) }).build();
    assert_eq!(
        execute_mut(&module),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// `source_pc` is a decoded module field; an out-of-range value used to be checked by a
/// `debug_assert!` only.
#[test]
fn out_of_range_source_pc_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! { Return })
        .with_source_pc(u32::MAX)
        .build();
    assert_eq!(
        execute_mut(&module),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// One immediate used to size the dropped-segment bitset at ~512 MiB.
#[test]
fn drop_with_an_oversized_segment_index_traps() {
    let data_drop = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        DataDrop(u32::MAX)
        Return
    })
    .build();
    assert_eq!(execute_mut(&data_drop), Err(TrapCode::MemoryOutOfBounds));

    let elem_drop = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        ElemDrop(u32::MAX)
        Return
    })
    .build();
    assert_eq!(execute_mut(&elem_drop), Err(TrapCode::TableOutOfBounds));
}

/// `BulkConst(u32::MAX)` used to spin 4.29e9 times without charging fuel.
#[test]
fn bulk_const_with_an_oversized_immediate_traps_immediately() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        BulkConst(u32::MAX)
        Return
    })
    .build();
    let start = std::time::Instant::now();
    assert_eq!(execute_mut(&module), Err(TrapCode::StackOverflow));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "the oversized bulk constant must not spin"
    );
}

/// Legitimate control flow keeps working: a counted loop with a backward branch and a forward
/// branch over an untaken arm.
#[test]
fn well_formed_control_flow_is_not_affected() {
    // let i = 3; while (i != 0) { i -= 1; }  -- the counter stays on the stack across the back
    // edge, so the loop head is entered with the same height every iteration.
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(3)    // 1: [i]
        LocalGet(1)    // 2: loop head, [i, i]
        I32Eqz         // 3: [i, i == 0]
        BrIfNez(4)     // 4: -> 8 (Drop) when the counter reached zero
        I32Const(1)    // 5
        I32Sub         // 6: [i - 1]
        Br(-5)         // 7: -> 2
        Drop           // 8
        Return         // 9
    })
    .build();
    assert_eq!(execute_mut(&module), Ok(()));
}

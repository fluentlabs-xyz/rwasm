//! Regression tests for out-of-range control flow in hand-built and decoded rWasm modules.
//! Construction records invalid static targets; execution rejects them before fetching opcodes.
//! Mutable table entries are also checked when used as indirect-call targets.

use rwasm::{
    always_failing_syscall_handler, instruction_set, ExecutionEngine, ImportLinker, RwasmModule,
    RwasmModuleBuilder, RwasmStore, TrapCode,
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

/// A branch whose target is ~17 GiB past the three-instruction code section.
#[test]
fn branch_target_outside_the_code_section_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        I32Const(0)
        Drop
        Br(i32::MAX)
    })
    .build();
    assert_eq!(execute(&module), Err(TrapCode::UnreachableCodeReached));
}

/// The same module through the documented round trip: encode, decode with the public decode entry
/// point, execute. Nothing between `serialize` and `execute` validates the branch target.
#[test]
fn decoded_branch_target_outside_the_code_section_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        I32Const(1)
        Br(i32::MAX)
    })
    .build();
    let (decoded, _) =
        RwasmModule::new_checked(&module.serialize()).expect("the encoding is valid");
    assert_eq!(execute_mut(&decoded), Err(TrapCode::UnreachableCodeReached));
}

fn execute_mut(module: &RwasmModule) -> Result<(), TrapCode> {
    execute(module)
}

#[test]
fn invalid_static_targets_and_missing_payloads_trap() {
    for code in [
        instruction_set! { Br(-1) Return },
        instruction_set! { Br(2) Return },
        instruction_set! { I32Const(1) BrIfEqz(i32::MIN) Return },
        instruction_set! { I32Const(0) BrIfNez(i32::MAX) Return },
        instruction_set! { CallInternal(u32::MAX) Return },
        instruction_set! { ReturnCallInternal(u32::MAX) },
        instruction_set! { RefFunc(u32::MAX) Drop Return },
        instruction_set! { I32Const(0) BrTable(u32::MAX) Return },
        instruction_set! { I32Const(0) BrTable(1) Return },
        instruction_set! { CallIndirect(0) },
        instruction_set! { ReturnCallIndirect(0) },
        instruction_set! { TableInit(0) },
        instruction_set! { TableInit(0) TableGet(0) },
    ] {
        let module = RwasmModuleBuilder::new(code).build();
        assert_eq!(execute(&module), Err(TrapCode::UnreachableCodeReached));
        let decoded = RwasmModule::new_checked_exact(&module.serialize()).unwrap();
        assert_eq!(execute(&decoded), Err(TrapCode::UnreachableCodeReached));
    }
}

#[test]
fn invalid_code_is_rejected_on_initialization_and_low_level_paths() {
    let module = RwasmModuleBuilder::new(instruction_set! { Br(i32::MAX) }).build();
    let mut store = RwasmStore::<()>::default();
    assert_eq!(
        ExecutionEngine::new().entrypoint(&mut store, &module),
        Err(TrapCode::UnreachableCodeReached)
    );
    let mut stack = rwasm::ValueStack::default();
    let mut calls = rwasm::CallStack::default();
    let mut executor =
        rwasm::RwasmExecutor::entrypoint(&module, &mut stack, &mut calls, &mut store);
    assert_eq!(
        executor.run(&[], &mut []),
        Err(TrapCode::UnreachableCodeReached)
    );
    assert_eq!(
        executor.run_with_stack_check(),
        Err(TrapCode::UnreachableCodeReached)
    );
}

#[test]
fn table_values_cannot_escape_the_code_section() {
    for tail_call in [false, true] {
        for target in [u32::MAX, 10] {
            let mut code = instruction_set! {
                StackCheck(16)
                I32Const(target)
                I32Const(1)
                TableGrow(0)
                Drop
                I32Const(0)
            };
            if tail_call {
                code.op_return_call_indirect(0);
            } else {
                code.op_call_indirect(0);
            }
            code.op_table_get(0);
            code.op_return();
            let module = RwasmModuleBuilder::new(code).build();
            assert_eq!(execute(&module), Err(TrapCode::UnreachableCodeReached));
        }
    }
}

#[test]
fn final_tail_call_payload_is_read_but_never_executed() {
    // Target 1 is a Return; target 9 is the final TableGet metadata word.
    for (target, expected) in [(1, Ok(())), (9, Err(TrapCode::UnreachableCodeReached))] {
        let module = RwasmModuleBuilder::new(instruction_set! {
            Br(2)
            Return
            StackCheck(16)
            I32Const(target)
            I32Const(1)
            TableGrow(0)
            Drop
            I32Const(0)
            ReturnCallIndirect(0)
            TableGet(0)
        })
        .build();
        assert_eq!(execute(&module), expected);
    }
    let module = RwasmModuleBuilder::new(instruction_set! {
        Br(2) ReturnCallIndirect(0) TableGet(0)
    })
    .build();
    assert_eq!(execute(&module), Err(TrapCode::UnreachableCodeReached));
}

#[cfg(feature = "serde")]
#[test]
fn serde_recomputes_instruction_bounds_without_changing_the_representation() {
    for (code, expected) in [
        (instruction_set! { Return }, Ok(())),
        (
            instruction_set! { Br(i32::MAX) },
            Err(TrapCode::UnreachableCodeReached),
        ),
    ] {
        let module = RwasmModuleBuilder::new(code).build();
        let mut json = serde_json::to_value(&module).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 1);
        assert!(json.get("inner").is_some());
        let decoded: RwasmModule = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(decoded, module);
        assert_eq!(execute(&decoded), expected);
        // An external validation claim is ignored, even if supplied next to the old wire fields.
        json["executable_len"] = serde_json::json!(u32::MAX);
        let decoded: RwasmModule = serde_json::from_value(json).unwrap();
        assert_eq!(execute(&decoded), expected);
    }
}

/// A code section that simply ends without a terminator: the fetch walks one `Opcode` past the
/// allocation.
#[test]
fn code_section_without_terminator_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! { I32Const(0) }).build();
    assert_eq!(execute_mut(&module), Err(TrapCode::UnreachableCodeReached));
}

//! Regression tests for out-of-bounds value stack accesses driven by hand-crafted bytecode.
//!
//! Every test runs its bytecode through the interpreter to prove that the value stack refuses the
//! access, which is the guarantee that holds however the trusted module was decoded.

use rwasm::{
    instruction_set, ExecutionEngine, ImportLinker, ImportName, InstructionSet, RwasmModuleBuilder,
    RwasmStore, StoreTr, TrapCode, TypedCaller, Value,
};
use rwasm_fuel_policy::SyscallFuelParams;
use std::sync::Arc;
use wasmparser::ValType;

fn execute(code_section: InstructionSet) -> Result<(), TrapCode> {
    let module = RwasmModuleBuilder::new(code_section).build();
    let mut store = RwasmStore::new(
        Arc::new(ImportLinker::default()),
        (),
        rwasm::always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );
    ExecutionEngine::new().execute(&mut store, &module, &[], &mut [])
}

#[test]
fn local_get_beyond_the_stack_traps_instead_of_reading_out_of_bounds() {
    let code = instruction_set! {
        StackCheck(16)
        LocalGet(0x0800_0000)
        Drop
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn local_set_beyond_the_stack_traps_instead_of_writing_out_of_bounds() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(0xdead_beefu32 as i32)
        LocalSet(0x0800_0000)
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn local_tee_beyond_the_stack_traps_instead_of_writing_out_of_bounds() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(0xdead_beefu32 as i32)
        LocalTee(0x0800_0000)
        Drop
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn dropping_below_the_stack_base_traps() {
    let code = instruction_set! {
        StackCheck(16)
        Drop
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn bulk_drop_below_the_stack_base_traps() {
    let code = instruction_set! {
        StackCheck(16)
        BulkDrop(0x0800_0000)
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn popping_below_the_stack_base_traps() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(1)
        I32Add
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

/// A syscall whose parameters underflow the value stack must trap before the host observes them.
///
/// The runtime is the only thing standing between a malformed trusted module and the host. Popping
/// a parameter that is not there yields a fabricated zero, and the handler returning
/// `ExecutionHalted` would otherwise carry the whole run out as a success.
#[test]
fn syscall_with_underflowing_params_traps_before_reaching_the_host() {
    const EXIT_IDX: u32 = 75;

    fn halting_handler(
        caller: &mut TypedCaller<'_, Vec<i32>>,
        _idx: u32,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        caller
            .data_mut()
            .extend(params.iter().map(|p| p.i32().unwrap_or(i32::MIN)));
        Err(TrapCode::ExecutionHalted)
    }

    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_exit"),
        EXIT_IDX,
        SyscallFuelParams::default(),
        &[ValType::I32],
        &[],
    );

    let code = instruction_set! {
        StackCheck(16)
        Call(EXIT_IDX)
        Return
    };
    let module = RwasmModuleBuilder::new(code).build();
    let mut store = RwasmStore::new(
        Arc::new(import_linker),
        Vec::<i32>::new(),
        halting_handler,
        Some(1_000_000),
        None,
    );
    let result = ExecutionEngine::new().execute(&mut store, &module, &[], &mut []);

    assert_eq!(result, Err(TrapCode::StackOverflow));
    assert!(
        store.data().is_empty(),
        "the host must not have been called: {:?}",
        store.data()
    );
}

/// An instruction that both underflows the stack and traps on its own terms reports the underflow:
/// the trap it raised is an artifact of the zero the suppressed pop substituted.
#[test]
fn a_trap_caused_by_an_underflow_is_reported_as_the_underflow() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(0)
        I32DivU
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn pushing_past_the_reserved_stack_window_traps() {
    // `StackCheck` reserves a single cell, so the pushes run out of stack long before the last one.
    let mut code = instruction_set! { StackCheck(1) };
    for _ in 0..(rwasm::N_MAX_STACK_SIZE + 1) {
        code.op_i32_const(1);
    }
    code.op_return();
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

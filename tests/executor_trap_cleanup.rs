//! `RwasmExecutor::run_with_stack_check` must leave the stacks and the store the way `run` does
//! after a trap. It used to return straight out of its loop on a trap, skipping the cleanup: the
//! call stack kept the trapped frames' return addresses and the store kept the signature an
//! indirect call had announced, which failed the next entry's `SignatureCheck`.

use rwasm::{
    instruction_set, CallStack, RwasmExecutor, RwasmModuleBuilder, RwasmStore, TrapCode, ValueStack,
};

#[test]
fn run_with_stack_check_cleans_up_after_a_trap() {
    // `CallInternal` jumps to the raw code offset 3; the indirect call there announces signature
    // 5 and traps on the empty table
    let trapping = RwasmModuleBuilder::new(instruction_set! {
        CallInternal(3)
        Return
        Unreachable
        I32Const(0)
        CallIndirect(5)
        TableGet(0)
        Return
    })
    .build();
    let mut store = RwasmStore::<()>::default();
    let mut value_stack = ValueStack::default();
    value_stack.reserve(16).unwrap();
    let mut call_stack = CallStack::default();
    let trap = RwasmExecutor::entrypoint(&trapping, &mut value_stack, &mut call_stack, &mut store)
        .run_with_stack_check();
    assert_eq!(trap, Err(TrapCode::TableOutOfBounds));
    assert!(call_stack.is_empty(), "the trapped frame was not popped");
    let stack_ptr = value_stack.stack_ptr();
    assert_eq!(value_stack.stack_len(stack_ptr), 0);

    // the announced signature no longer applies to the next entry
    let checked = RwasmModuleBuilder::new(instruction_set! {
        SignatureCheck(7)
        Return
    })
    .build();
    let mut value_stack = ValueStack::default();
    let mut call_stack = CallStack::default();
    let mut executor =
        RwasmExecutor::entrypoint(&checked, &mut value_stack, &mut call_stack, &mut store);
    assert_eq!(executor.run_with_stack_check(), Ok(()));
}

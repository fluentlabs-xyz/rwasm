use crate::{CallStack, RwasmExecutor, RwasmModule, Store, TrapCode, ValueStack};

/// Represents the core execution engine for managing the execution of a program,
/// including the handling of values and function calls.
///
/// The `ExecutionEngine` struct contains two primary parts:
/// 1. `value_stack`: A stack data structure to manage and store stack values during the execution
///    of instructions or operations.
/// 2. `call_stack`: A stack data structure to track active function calls and their contexts,
///    enabling function call management and stack-based control flow.
///
/// # Fields
/// - `value_stack`: A `ValueStack` instance used for pushing and popping intermediate values during
///   execution.
/// - `call_stack`: A `CallStack` instance for managing the state of function invocations and
///   tracking their execution context.
///
/// # Usage
/// This struct is designed to serve as the central execution environment in a virtual
/// machine or interpreter. The `ExecutionEngine` ensures proper management of execution
/// states, facilitating efficient value handling and nested function calls.
///
/// Example scenarios include evaluating expressions, executing bytecode, or managing the
/// execution flow of a higher-level program.
pub struct ExecutionEngine {
    value_stack: ValueStack,
    call_stack: CallStack,
}

impl ExecutionEngine {
    pub fn new() -> Self {
        Self {
            value_stack: ValueStack::default(),
            call_stack: CallStack::default(),
        }
    }

    pub fn value_stack(&mut self) -> &mut ValueStack {
        &mut self.value_stack
    }

    pub fn call_stack(&mut self) -> &mut CallStack {
        &mut self.call_stack
    }

    pub fn create_executor<'a, T>(
        &'a mut self,
        store: &'a mut Store<T>,
        module: &'a RwasmModule,
    ) -> RwasmExecutor<'a, T> {
        RwasmExecutor::new(&module, &mut self.value_stack, &mut self.call_stack, store)
    }

    pub fn execute<T>(
        &mut self,
        store: &mut Store<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        debug_assert!(
            self.call_stack.is_empty(),
            "the call stack must be empty before an execution, use `resume` instead"
        );
        RwasmExecutor::new(&module, &mut self.value_stack, &mut self.call_stack, store).run()
    }

    pub fn resume<T>(
        &mut self,
        store: &mut Store<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        let (ip, sp) = self.call_stack.pop().unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        RwasmExecutor::resumable(
            &module,
            &mut self.value_stack,
            sp,
            &mut self.call_stack,
            ip,
            store,
        )
        .run()
    }

    pub fn reset<T>(&mut self, store: &mut Store<T>, keep_flags: bool) {
        self.value_stack.reset();
        self.call_stack.reset();
        store.reset(keep_flags)
    }
}

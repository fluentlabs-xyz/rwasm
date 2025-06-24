use crate::{CallStack, RwasmExecutor, RwasmModule, RwasmStore, TrapCode, ValueStack};

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

    pub fn create_callable_executor<'a, T>(
        &'a mut self,
        store: &'a mut RwasmStore<T>,
        module: &'a RwasmModule,
    ) -> RwasmExecutor<'a, T> {
        debug_assert!(
            self.call_stack.is_empty(),
            "the call stack must be empty before an execution, use `resume` instead"
        );
        RwasmExecutor::new(&module, &mut self.value_stack, &mut self.call_stack, store)
    }

    pub fn create_resumable_executor<'a, T>(
        &'a mut self,
        store: &'a mut RwasmStore<T>,
        module: &'a RwasmModule,
    ) -> RwasmExecutor<'a, T> {
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
    }

    pub fn execute<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        self.create_callable_executor(store, module).run()
    }

    pub fn resume<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        self.create_resumable_executor(store, module).run()
    }

    pub fn reset<T>(&mut self, store: &mut RwasmStore<T>, keep_flags: bool) {
        self.value_stack.reset();
        self.call_stack.reset();
        store.reset(keep_flags)
    }
}

#[cfg(feature = "std")]
thread_local! {
    static ENGINE: core::cell::RefCell<ExecutionEngine> = core::cell::RefCell::new(ExecutionEngine::new());
}

#[cfg(feature = "std")]
impl ExecutionEngine {
    pub fn acquire_shared<R, F: FnOnce(&mut ExecutionEngine) -> R>(f: F) -> R {
        ENGINE.with(|cell| f(&mut cell.borrow_mut()))
    }
}

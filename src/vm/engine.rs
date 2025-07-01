use crate::{CallStack, RwasmExecutor, RwasmModule, RwasmStore, TrapCode, Value, ValueStack};
use alloc::vec::Vec;

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
    value_stack: Vec<ValueStack>,
    call_stack: Vec<CallStack>,
}

impl ExecutionEngine {
    pub fn new() -> Self {
        Self {
            value_stack: Vec::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn execute<T: Send + Sync>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.value_stack.push(ValueStack::default());
        self.call_stack.push(CallStack::default());
        let mut executor = RwasmExecutor::new(
            &module,
            self.value_stack.last_mut().unwrap(),
            self.call_stack.last_mut().unwrap(),
            store,
        );
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => Err(TrapCode::InterruptionCalled),
            res => {
                let value_stack = self.value_stack.pop().unwrap();
                // debug_assert!(value_stack.is_empty() || res.is_err());
                let call_stack = self.call_stack.pop().unwrap();
                // debug_assert!(call_stack.is_empty() || res.is_err());
                res
            }
        }
    }

    pub fn resume<T: Send + Sync>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let (value_stack, call_stack) = (
            self.value_stack.last_mut().unwrap(),
            self.call_stack.last_mut().unwrap(),
        );
        let (ip, sp) = call_stack.pop().unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        let mut executor =
            RwasmExecutor::resumable(&module, value_stack, sp, call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => Err(TrapCode::InterruptionCalled),
            res => {
                let value_stack = self.value_stack.pop().unwrap();
                // debug_assert!(value_stack.is_empty() || res.is_err());
                let call_stack = self.call_stack.pop().unwrap();
                // debug_assert!(call_stack.is_empty() || res.is_err());
                res
            }
        }
    }
}

#[cfg(feature = "std")]
thread_local! {
    static ENGINE: std::rc::Rc<std::cell::RefCell<ExecutionEngine>> = std::rc::Rc::new(std::cell::RefCell::new(ExecutionEngine::new()));
}

#[cfg(feature = "std")]
impl ExecutionEngine {
    pub fn acquire_shared() -> std::rc::Rc<std::cell::RefCell<ExecutionEngine>> {
        ENGINE.with(|e| e.clone())
    }
}

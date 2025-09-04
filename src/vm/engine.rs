use crate::{
    CallStack, InstructionPtr, RwasmExecutor, RwasmModule, RwasmStore, TrapCode, Value, ValueStack,
    ValueStackPtr,
};
use smallvec::SmallVec;

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
#[derive(Default)]
pub struct ExecutionEngine {
    value_stack: SmallVec<[ValueStack; 8]>,
    call_stack: SmallVec<[CallStack; 8]>,
    resume_stack: SmallVec<[(InstructionPtr, ValueStackPtr); 8]>,
}

impl ExecutionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Executes a rWasm module's function with the given parameters and stores the result.
    ///
    /// This method is designed to run an entry point of a Wasm module in the context of a runtime
    /// execution environment. It handles the value stack and call stack during execution and
    /// manages interruption and error scenarios.
    ///
    /// # Type Parameters
    /// * `T` - A type that implements the `Send` and `Sync` traits, representing a custom shared
    ///   state that can be used during execution.
    ///
    /// # Parameters
    /// * `store` - A mutable reference to an `RwasmStore` instance, which holds the runtime state
    ///   for the execution.
    /// * `module` - A reference to the `RwasmModule` representing the compiled WebAssembly module
    ///   to execute.
    /// * `params` - A slice of `Value` representing the input parameters to pass to the Wasm
    ///   function being executed.
    /// * `result` - A mutable slice of `Value` where the result of the function execution will be
    ///   stored.
    ///
    /// # Returns
    /// * `Ok(())` - If the Wasm module's function executes successfully without any interruption or
    ///   trap.
    /// * `Err(TrapCode)` - If the execution is interrupted or encounters a trap. Possible trap
    ///   codes include:
    ///   - `TrapCode::InterruptionCalled`: Indicates that execution was interrupted explicitly.
    ///
    /// # Panics
    /// This function assumes that the `value_stack` and `call_stack` are properly initialized. If
    /// the stacks are accessed while empty due to a programming error, it may result in a
    /// panic.
    pub fn execute<T: Send>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.value_stack.push(ValueStack::default());
        self.call_stack.push(CallStack::default());
        let mut executor = RwasmExecutor::entrypoint(
            &module,
            self.value_stack.last_mut().unwrap(),
            self.call_stack.last_mut().unwrap(),
            store,
        );
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                self.resume_stack.push((executor.ip, executor.sp));
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                self.value_stack.pop().unwrap();
                self.call_stack.pop().unwrap();
                res
            }
        }
    }

    /// Resumes the execution of a WASM (WebAssembly) function that was previously interrupted.
    ///
    /// # Parameters
    /// * `store`: A mutable reference to the `RwasmStore` where all WASM runtime resources and
    ///   shared states are stored. This is required for managing the execution and maintaining host
    ///   environment interactions.
    /// * `module`: A reference to the `RwasmModule` object, representing the compiled rWASM module
    ///   associated with the function being resumed. This contains the function definitions and
    ///   other module-specific data.
    /// * `params`: A reference to a slice of `Value` objects, representing the parameters to be
    ///   passed to the WASM function being resumed. These correspond to the function's input
    ///   arguments.
    /// * `result`: A mutable reference to a slice of `Value` objects, where the results of the
    ///   executed rWASM function will be written. The caller must ensure the slice is large enough
    ///   to accommodate the expected output values.
    ///
    /// # Returns
    /// - `Ok(())`: If the function execution resumes successfully and completes without traps.
    /// - `Err(TrapCode)`: If a trap (error or interruption) occurs during execution. For example:
    ///   - `TrapCode::InterruptionCalled`: Indicates the execution was interrupted explicitly.
    ///
    /// # Behavior
    /// * Retrieves the relevant value stack and call stack from the `resume_stack` to continue
    ///   execution.
    /// * Creates a new `RwasmExecutor` object to run the instructions from the previous instruction
    ///   pointer (`ip`) and stack pointer (`sp`).
    /// * If an `InterruptionCalled` trap occurs, the current `ip` and `sp` state are saved to
    ///   `resume_stack`, allowing further resumption of execution later.
    /// * After successful execution or interruption, modifies the state of internal execution
    ///   stacks (`value_stack` and `call_stack`).
    ///
    /// # Panics
    /// This function will panic in the following cases:
    /// - If there is no remaining call stack in `resume_stack` (indicates an invalid or
    ///   inconsistent state).
    ///
    /// This function assumes that it is only called in valid scenarios where there is an already
    /// interrupted call to be resumed.
    pub fn resume<T: Send>(
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
        let (ip, sp) = self.resume_stack.pop().unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        let mut executor = RwasmExecutor::new(&module, value_stack, sp, call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                self.resume_stack.push((executor.ip, executor.sp));
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                self.value_stack.pop().unwrap();
                self.call_stack.pop().unwrap();
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

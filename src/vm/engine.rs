use crate::{
    CallStack, InstructionPtr, ReusableContext, RwasmExecutor, RwasmModule, RwasmStore, TrapCode,
    Value, ValueStack,
};
use core::mem::take;

/// Runs rwasm modules against an [`RwasmStore`].
///
/// The engine holds no state of its own: every call allocates its value and call stacks and hands
/// them to a fresh executor, and an interrupted execution parks them in the store's resumable
/// context. It therefore needs no synchronization and is safe to use re-entrantly, e.g. from a
/// syscall handler that runs another module on [`ExecutionEngine::acquire_shared`] while an
/// execution on the same engine is in progress.
///
/// An earlier version wrapped an empty inner struct in a spin lock. The lock protected nothing,
/// serialized every execution in the process, and made such a re-entrant handler busy-spin
/// forever.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExecutionEngine;

impl ExecutionEngine {
    /// Creates a stateless engine that can execute modules on independent stores.
    pub fn new() -> Self {
        Self
    }

    /// Returns the process-wide engine.
    ///
    /// The engine is stateless, so this is equivalent to [`ExecutionEngine::new`]; it is kept for
    /// hosts that name their engine once and hand it around.
    pub fn acquire_shared() -> ExecutionEngine {
        Self
    }

    /// Runs a module's initialization prologue, parking its stacks if a syscall interrupts it.
    #[inline(always)]
    pub fn entrypoint<T>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        let mut value_stack = ValueStack::default();
        let mut call_stack = CallStack::default();
        if store.resumable_context.is_some() {
            return Err(TrapCode::IllegalOpcode);
        }
        let mut executor =
            RwasmExecutor::entrypoint(module, &mut value_stack, &mut call_stack, store);
        match executor.run_raw(&[], &mut []) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(
                    store,
                    ReusableContext {
                        module: module.clone(),
                        value_stack,
                        call_stack,
                        ip,
                        initializing: true,
                    },
                )
            }
            res => res,
        }
    }

    /// Executes a rWasm module's function with the given parameters and stores the result.
    #[inline(always)]
    pub fn execute<T>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut value_stack = ValueStack::default();
        let mut call_stack = CallStack::default();
        if store.resumable_context.is_some() {
            return Err(TrapCode::IllegalOpcode);
        }
        let sp = value_stack.stack_ptr();
        // `source_pc` is a module-declared entry offset. It used to be checked with a
        // `debug_assert!` only, so a module whose entry offset is outside the code section made
        // the interpreter fetch instructions from outside the section in release builds.
        if module.source_pc as usize >= module.code_section.len() {
            return Err(TrapCode::UnreachableCodeReached);
        }
        let mut ip = InstructionPtr::new(module.code_section.as_ptr());
        ip.offset(module.source_pc as isize);
        let mut executor =
            RwasmExecutor::new(module, &mut value_stack, sp, &mut call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(
                    store,
                    ReusableContext {
                        module: module.clone(),
                        value_stack,
                        call_stack,
                        ip,
                        initializing: false,
                    },
                )
            }
            res => res,
        }
    }

    /// Resumes an execution on `store` that returned [`TrapCode::InterruptionCalled`].
    /// Completing a replacement initializer commits its state; a trap restores the old instance.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCode::IllegalOpcode`] when the store holds no interrupted execution: the
    /// host called `resume` without a preceding interruption, or a second time after the resumed
    /// execution finished. That is a host bug, but one a node cannot rule out from the outside,
    /// so it is reported instead of aborting the process.
    #[inline(always)]
    pub fn resume<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let ReusableContext {
            module,
            mut call_stack,
            ip,
            mut value_stack,
            initializing,
        } = take(&mut store.resumable_context).ok_or(TrapCode::IllegalOpcode)?;
        let sp = value_stack.stack_ptr();
        let mut executor =
            RwasmExecutor::new(&module, &mut value_stack, sp, &mut call_stack, ip, store);
        let outcome = if initializing {
            executor.run_raw(params, result)
        } else {
            executor.run(params, result)
        };
        match outcome {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(
                    store,
                    ReusableContext {
                        module,
                        value_stack,
                        call_stack,
                        ip,
                        initializing,
                    },
                )
            }
            res if initializing => store.finish_instantiation(res),
            res => res,
        }
    }

    /// Parks interpreter stacks and instruction position for the next resume call.
    fn remember_context<T>(
        &self,
        store: &mut RwasmStore<T>,
        context: ReusableContext,
    ) -> Result<(), TrapCode> {
        store.resumable_context = Some(context);
        Err(TrapCode::InterruptionCalled)
    }
}

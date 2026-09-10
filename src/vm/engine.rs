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

    #[inline(always)]
    pub fn entrypoint<T>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        let mut value_stack = ValueStack::default();
        let mut call_stack = CallStack::default();
        debug_assert!(
            store.resumable_context.is_none(),
            "rwasm: resumable context is presented"
        );
        let mut executor =
            RwasmExecutor::entrypoint(module, &mut value_stack, &mut call_stack, store);
        match executor.run(&[], &mut []) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(module.clone(), store, value_stack, call_stack, ip)
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
        debug_assert!(
            store.resumable_context.is_none(),
            "rwasm: resumable context is presented"
        );
        let sp = value_stack.stack_ptr();
        let mut ip = InstructionPtr::new(module.code_section.as_ptr());
        debug_assert!(module.source_pc < module.code_section.len() as u32);
        ip.offset(module.source_pc as isize);
        let mut executor =
            RwasmExecutor::new(module, &mut value_stack, sp, &mut call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(module.clone(), store, value_stack, call_stack, ip)
            }
            res => res,
        }
    }

    /// Resumes the execution of a WASM (WebAssembly) function that was previously interrupted.
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
        } = take(&mut store.resumable_context).unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        let sp = value_stack.stack_ptr();
        let mut executor =
            RwasmExecutor::new(&module, &mut value_stack, sp, &mut call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(module, store, value_stack, call_stack, ip)
            }
            res => res,
        }
    }

    fn remember_context<T>(
        &self,
        module: RwasmModule,
        store: &mut RwasmStore<T>,
        value_stack: ValueStack,
        call_stack: CallStack,
        ip: InstructionPtr,
    ) -> Result<(), TrapCode> {
        store.resumable_context = Some(ReusableContext {
            module,
            call_stack,
            ip,
            value_stack,
        });
        Err(TrapCode::InterruptionCalled)
    }
}

use crate::{
    CallStack, InstructionPtr, ReusableContext, RwasmExecutor, RwasmModule, RwasmStore, TrapCode,
    Value, ValueStack,
};
use alloc::sync::Arc;
use core::mem::take;
use spin::Mutex;

/// Represents the core execution engine for managing the execution of a program,
/// including the handling of values and function calls.
#[derive(Default, Clone)]
pub struct ExecutionEngine {
    inner: Arc<Mutex<ExecutionEngineInner>>,
}

impl ExecutionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn execute<T: Send + Sync>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.execute(store, module, params, result)
    }

    pub fn resume<T: Send + Sync>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.resume(store, params, result)
    }
}

#[derive(Default)]
struct ExecutionEngineInner {}

impl ExecutionEngineInner {
    /// Executes a rWasm module's function with the given parameters and stores the result.
    pub fn execute<T: Send + Sync>(
        &mut self,
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
        let mut executor =
            RwasmExecutor::entrypoint(&module, &mut value_stack, &mut call_stack, store);
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
    pub fn resume<T: Send + Sync>(
        &mut self,
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

    fn remember_context<T: Send + Sync>(
        &mut self,
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

#[cfg(feature = "std")]
thread_local! {
    static ENGINE: ExecutionEngine = ExecutionEngine::new();
}

#[cfg(feature = "std")]
impl ExecutionEngine {
    pub fn acquire_shared() -> ExecutionEngine {
        ENGINE.with(Clone::clone)
    }
}

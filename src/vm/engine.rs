use crate::vm::reusable_pool;
use crate::vm::reusable_pool::specific::{CallStackItemConfig, ValueStackItemConfig};
use crate::vm::reusable_pool::ReusablePool;
use crate::{
    CallStack, RwasmExecutor, RwasmModule, RwasmStore, TrapCode, Value, ValueStack,
    N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE,
};
use alloc::sync::Arc;
use core::mem::take;
use smallvec::SmallVec;
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
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.resume(store, module, params, result)
    }
}

const REUSABLE_POOL_KEEP_DEFAULT: usize = 100;

struct ExecutionEngineInner {
    value_stack: SmallVec<[ValueStack; 8]>,
    call_stack: SmallVec<[CallStack; 8]>,
    reusable_value_stacks: ReusablePool<ValueStack, ValueStackItemConfig>,
    reusable_call_stacks: ReusablePool<CallStack, CallStackItemConfig>,
}

impl Default for ExecutionEngineInner {
    fn default() -> Self {
        Self {
            value_stack: Default::default(),
            call_stack: Default::default(),
            reusable_value_stacks: ReusablePool::new(reusable_pool::Config::new(
                REUSABLE_POOL_KEEP_DEFAULT,
                ValueStackItemConfig::new(N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE),
            )),
            reusable_call_stacks: ReusablePool::new(reusable_pool::Config::new(
                REUSABLE_POOL_KEEP_DEFAULT,
                CallStackItemConfig::new(),
            )),
        }
    }
}

impl ExecutionEngineInner {
    /// Executes a rWasm module's function with the given parameters and stores the result.
    pub fn execute<T: Send + Sync>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.value_stack
            .push(self.reusable_value_stacks.reuse_or_new());
        self.call_stack
            .push(self.reusable_call_stacks.reuse_or_new());
        let mut executor = RwasmExecutor::entrypoint(
            &module,
            self.value_stack.last_mut().unwrap(),
            self.call_stack.last_mut().unwrap(),
            store,
        );
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                store.resumable_context = Some((executor.ip, executor.sp));
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                let value_stack = self.value_stack.pop().unwrap();
                self.reusable_value_stacks.recycle(value_stack);
                let call_stack = self.call_stack.pop().unwrap();
                self.reusable_call_stacks.recycle(call_stack);
                res
            }
        }
    }

    /// Resumes the execution of a WASM (WebAssembly) function that was previously interrupted.
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
        let (ip, sp) = take(&mut store.resumable_context).unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        let mut executor = RwasmExecutor::new(&module, value_stack, sp, call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                store.resumable_context = Some((executor.ip, executor.sp));
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                let value_stack = self.value_stack.pop().unwrap();
                self.reusable_value_stacks.recycle(value_stack);
                let call_stack = self.call_stack.pop().unwrap();
                self.reusable_call_stacks.recycle(call_stack);
                res
            }
        }
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

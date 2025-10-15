use crate::{
    vm::{
        memory::OnDemandGlobalMemory,
        reusable_pool::{ItemBehavior, ReusablePool, ReusablePoolConfig},
    },
    CallStack, IGlobalMemory, Pages, RwasmExecutor, RwasmModule, RwasmStore, TrapCode, Value,
    ValueStack, N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE,
};
use alloc::{sync::Arc, vec::Vec};
use core::{
    mem::take,
    ops::{Deref, DerefMut},
};
use spin::Mutex;

#[derive(Clone)]
pub struct GlobalMemoryConfig {
    initial_pages: Pages,
}

impl GlobalMemoryConfig {
    pub fn new(initial_pages: Pages) -> Self {
        Self { initial_pages }
    }
}

pub const GLOBAL_MEMORY_ITEM_BEHAVIOR_SIMPLE_CREATE_STRATEGY: usize = 0;
pub const GLOBAL_MEMORY_ITEM_BEHAVIOR_PREALLOC_CREATE_STRATEGY: usize = 1;

pub enum GlobalMemory {
    OnDemand(OnDemandGlobalMemory),
    #[cfg(all(feature = "unix-memory", unix))]
    Pooling(crate::vm::memory::mmap::PoolingGlobalMemory),
}

impl Deref for GlobalMemory {
    type Target = dyn IGlobalMemory;

    fn deref(&self) -> &Self::Target {
        match self {
            GlobalMemory::OnDemand(v) => v,
            #[cfg(all(feature = "unix-memory", unix))]
            GlobalMemory::Pooling(v) => v,
        }
    }
}

impl DerefMut for GlobalMemory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            GlobalMemory::OnDemand(v) => v,
            #[cfg(all(feature = "unix-memory", unix))]
            GlobalMemory::Pooling(v) => v,
        }
    }
}

#[cfg(all(feature = "unix-memory", unix))]
impl From<crate::vm::memory::mmap::PoolingGlobalMemory> for GlobalMemory {
    fn from(value: crate::vm::memory::mmap::PoolingGlobalMemory) -> Self {
        GlobalMemory::Pooling(value)
    }
}

impl From<OnDemandGlobalMemory> for GlobalMemory {
    fn from(value: OnDemandGlobalMemory) -> Self {
        GlobalMemory::OnDemand(value)
    }
}

impl ItemBehavior<GlobalMemory> for GlobalMemoryConfig {
    fn create_item(&self) -> GlobalMemory {
        self.create_item_with_strategy::<GLOBAL_MEMORY_ITEM_BEHAVIOR_SIMPLE_CREATE_STRATEGY>()
    }

    fn create_item_with_strategy<const STRATEGY: usize>(&self) -> GlobalMemory {
        match STRATEGY {
            GLOBAL_MEMORY_ITEM_BEHAVIOR_SIMPLE_CREATE_STRATEGY => {
                OnDemandGlobalMemory::new(self.initial_pages).into()
            }
            #[cfg(all(feature = "unix-memory", unix))]
            GLOBAL_MEMORY_ITEM_BEHAVIOR_PREALLOC_CREATE_STRATEGY => {
                crate::vm::memory::mmap::PoolingGlobalMemory::new(self.initial_pages).into()
            }
            _ => OnDemandGlobalMemory::new(self.initial_pages).into(),
        }
    }

    fn reset_for_reuse(item: &mut GlobalMemory) {
        item.reset()
    }
}

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

const ESTIMATED_CALL_DEPTH: usize = 1024;
const REUSABLE_POOL_KEEP: usize = 128;

#[derive(Clone)]
pub struct ReusableStackConfig {
    initial_len: usize,
    maximum_len: usize,
}

impl ReusableStackConfig {
    pub fn new(initial_len: usize, maximum_len: usize) -> Self {
        Self {
            initial_len,
            maximum_len,
        }
    }
}

impl ItemBehavior<(ValueStack, CallStack)> for ReusableStackConfig {
    #[inline(always)]
    fn create_item(&self) -> (ValueStack, CallStack) {
        (
            ValueStack::new(self.initial_len, self.maximum_len),
            CallStack::default(),
        )
    }

    #[inline]
    fn create_item_with_strategy<const STRATEGY: usize>(&self) -> (ValueStack, CallStack) {
        self.create_item()
    }

    fn reset_for_reuse(item: &mut (ValueStack, CallStack)) {
        item.0.reset();
        item.1.reset();
    }
}

struct ExecutionEngineInner {
    acquired_stacks: Vec<(ValueStack, CallStack)>,
    reusable_stacks: ReusablePool<(ValueStack, CallStack), ReusableStackConfig>,
    global_memory_pool: ReusablePool<GlobalMemory, GlobalMemoryConfig>,
}

impl Default for ExecutionEngineInner {
    fn default() -> Self {
        let mut global_memory_pool = ReusablePool::new(ReusablePoolConfig::new(
            REUSABLE_POOL_KEEP,
            GlobalMemoryConfig::new(0.into()),
        ));
        let reusable_stacks = ReusablePool::new(ReusablePoolConfig::new(
            REUSABLE_POOL_KEEP,
            ReusableStackConfig::new(N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE),
        ));
        global_memory_pool.warmup::<GLOBAL_MEMORY_ITEM_BEHAVIOR_PREALLOC_CREATE_STRATEGY>(None);
        Self {
            acquired_stacks: Vec::with_capacity(ESTIMATED_CALL_DEPTH),
            reusable_stacks,
            global_memory_pool,
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
        let (value_stack, call_stack) = self.reusable_stacks.reuse_or_new_item::<0>();
        self.acquired_stacks.push((value_stack, call_stack));
        let (value_stack_ref, call_stack_ref) = self.acquired_stacks.last_mut().unwrap();
        if store.global_memory.is_none() {
            store.global_memory = self
                .global_memory_pool
                .reuse_or_new_item::<GLOBAL_MEMORY_ITEM_BEHAVIOR_SIMPLE_CREATE_STRATEGY>()
                .into();
        }
        let mut executor =
            RwasmExecutor::entrypoint(&module, value_stack_ref, call_stack_ref, store);
        let result = match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                store.resumable_context = Some((executor.ip, executor.sp));
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                let stacks = self.acquired_stacks.pop().unwrap();
                self.reusable_stacks.recycle(stacks);
                if let Some(global_memory) = store.global_memory.take() {
                    self.global_memory_pool.recycle(global_memory);
                }
                res
            }
        };
        result
    }

    /// Resumes the execution of a WASM (WebAssembly) function that was previously interrupted.
    pub fn resume<T: Send + Sync>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let (value_stack_ref, call_stack_ref) = self.acquired_stacks.last_mut().unwrap();
        let (ip, sp) = take(&mut store.resumable_context).unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        let mut executor =
            RwasmExecutor::new(&module, value_stack_ref, sp, call_stack_ref, ip, store);
        let result = match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                store.resumable_context = Some((executor.ip, executor.sp));
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                let value_stack = self.acquired_stacks.pop().unwrap();
                self.reusable_stacks.recycle(value_stack);
                self.global_memory_pool
                    .try_recycle_option(&mut store.global_memory);
                res
            }
        };
        result
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

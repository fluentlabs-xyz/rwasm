use crate::{
    vm::{
        config::Config,
        engine::{
            memories::{MemoryAllocator, MemoryAllocatorTr},
            stacks::ReusableStacks,
        },
        reusable_pool::ReusablePool,
        ResumableContext,
    },
    Pages, RwasmExecutor, RwasmModule, RwasmStore, TrapCode, Value,
};
use alloc::sync::Arc;
use core::mem::take;
use spin::Mutex;

mod memories;
mod stacks;

/// Represents the core execution engine for managing the execution of a program,
/// including the handling of values and function calls.
#[derive(Clone)]
pub struct ExecutionEngine {
    inner: Arc<Mutex<ExecutionEngineInner>>,
}

impl Default for ExecutionEngine {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl ExecutionEngine {
    pub fn new(config: Config) -> Self {
        let mut reusable_stacks =
            ReusablePool::<ReusableStacks>::new(config.reusable_stack.maximum_len);
        for _ in 0..config.reusable_stack.initial_len {
            reusable_stacks.recycle(ReusableStacks::default());
        }
        let memory_allocator = MemoryAllocator::new(&config);
        let inner = ExecutionEngineInner {
            reusable_stacks,
            memory_allocator,
            config,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
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

struct ExecutionEngineInner {
    reusable_stacks: ReusablePool<ReusableStacks>,
    memory_allocator: MemoryAllocator,
    config: Config,
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
        assert!(
            store.resumable_context.is_none(),
            "the store contains reusable context"
        );
        let ReusableStacks {
            mut value_stack,
            mut call_stack,
        } = self
            .reusable_stacks
            .try_reuse_item()
            .unwrap_or_else(ReusableStacks::default);
        assert!(
            store.global_memory.is_none(),
            "rwasm: store must recycle its memory after execution, this should never happen"
        );
        _ = store.global_memory.get_or_insert_with(|| {
            let pages = Pages::new(self.config.default_memory_pages).unwrap();
            self.memory_allocator.allocate_memory(pages)
        });
        let mut executor =
            RwasmExecutor::entrypoint(&module, &mut value_stack, &mut call_stack, store);
        let result = match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let sp = executor.sp;
                let ip = executor.ip;
                store.resumable_context = Some(ResumableContext {
                    value_stack,
                    sp,
                    call_stack,
                    ip,
                });
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                let mut reusable_stacks = ReusableStacks {
                    value_stack,
                    call_stack,
                };
                reusable_stacks.make_recyclable();
                self.reusable_stacks.recycle(reusable_stacks);
                if let Some(global_memory) = store.global_memory.take() {
                    self.memory_allocator.recycle_memory(global_memory);
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
        let ResumableContext {
            mut value_stack,
            sp,
            mut call_stack,
            ip,
        } = take(&mut store.resumable_context).unwrap_or_else(|| {
            unreachable!("resume calling without a remaining call stack");
        });
        let mut executor =
            RwasmExecutor::new(&module, &mut value_stack, sp, &mut call_stack, ip, store);
        let result = match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let sp = executor.sp;
                let ip = executor.ip;
                store.resumable_context = Some(ResumableContext {
                    value_stack,
                    sp,
                    call_stack,
                    ip,
                });
                Err(TrapCode::InterruptionCalled)
            }
            res => {
                let mut reusable_stacks = ReusableStacks {
                    value_stack,
                    call_stack,
                };
                reusable_stacks.make_recyclable();
                self.reusable_stacks.recycle(reusable_stacks);
                if let Some(global_memory) = store.global_memory.take() {
                    self.memory_allocator.recycle_memory(global_memory);
                }
                res
            }
        };
        result
    }
}

#[cfg(feature = "std")]
impl ExecutionEngine {
    pub fn acquire_shared() -> ExecutionEngine {
        use crate::{MemoryAllocationStrategy, PoolingAllocatorConfig, ReusableStackConfig};
        static ENGINE: std::sync::OnceLock<ExecutionEngine> = std::sync::OnceLock::new();
        ENGINE
            .get_or_init(|| {
                ExecutionEngine::new(Config {
                    memory_allocation_strategy: MemoryAllocationStrategy::Pooling(
                        PoolingAllocatorConfig::default(),
                    ),
                    reusable_stack: ReusableStackConfig::default(),
                    default_memory_pages: 1,
                })
            })
            .clone()
    }
}

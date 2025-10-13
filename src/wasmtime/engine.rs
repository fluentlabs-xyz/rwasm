use crate::N_MAX_STACK_SIZE;
use std::{mem::size_of, sync::OnceLock};
use wasmtime::{Config, Engine, OptLevel, Strategy};

static ENGINE: OnceLock<Engine> = OnceLock::new();

pub fn wasmtime_engine() -> &'static Engine {
    ENGINE.get_or_init(factory_wasmtime_engine)
}

fn factory_wasmtime_engine() -> Engine {
    let mut cfg = Config::new();
    #[cfg(feature = "pooling-allocator")]
    {
        use wasmtime::{InstanceAllocationStrategy, PoolingAllocationConfig};
        // TODO(dmitry123): How many concurrent instances do we want to have?
        const CONCURRENCY: u32 = 4096;
        const MEMORIES_PER_INST: u32 = 1;
        const TABLES_PER_INST: u32 = 5;
        // Create pooling allocator config
        let mut pool = PoolingAllocationConfig::default();
        pool.total_core_instances(CONCURRENCY);
        pool.total_memories(CONCURRENCY * MEMORIES_PER_INST);
        pool.total_tables(CONCURRENCY * TABLES_PER_INST);
        pool.total_stacks(CONCURRENCY);
        pool.linear_memory_keep_resident(16 << 20);
        pool.table_keep_resident(0);
        pool.max_unused_warm_slots(0);
        pool.decommit_batch_size(128);
        // Enable pooling allocator
        cfg.allocation_strategy(InstanceAllocationStrategy::Pooling(pool));
    }
    cfg.strategy(Strategy::Cranelift);
    cfg.collector(wasmtime::Collector::Null);
    cfg.max_wasm_stack(N_MAX_STACK_SIZE * size_of::<u32>());
    cfg.async_support(true);
    // 32-bit memories are cheaper and pool better unless you truly need >4 GiB
    cfg.wasm_memory64(false);
    // Make initial memory image cheap (copy-on-write for data segments)
    cfg.memory_init_cow(true);
    cfg.cranelift_opt_level(OptLevel::SpeedAndSize);
    cfg.parallel_compilation(false);
    cfg.consume_fuel(true);
    // Enable debug info and backtrace for debug mode
    #[cfg(debug_assertions)]
    {
        cfg.debug_info(true);
        cfg.wasm_backtrace(true);
    }
    cfg.wasm_simd(true);
    Engine::new(&cfg).expect("failed to create engine")
}

use crate::N_MAX_STACK_SIZE;
use std::sync::OnceLock;
use wasmtime::Engine;

static ENGINE: OnceLock<Engine> = OnceLock::new();

pub fn wasmtime_engine() -> &'static Engine {
    ENGINE.get_or_init(factory_wasmtime_engine)
}

fn factory_wasmtime_engine() -> Engine {
    let concurrency: u32 = 256;
    let memories_per_inst: u32 = 1;
    let tables_per_inst: u32 = 10;

    // --- Pooling allocator
    // let mut pool = PoolingAllocationConfig::new();

    // // Cap total pools (avoid over-reservation); enough to cover peak:
    // pool.total_core_instances(concurrency);
    // pool.total_memories(concurrency * memories_per_inst);
    // pool.total_tables(concurrency * tables_per_inst);

    // // Aggressively reuse “warm” slots so we don’t expand the set of used slots
    // // (reduces RSS & faults when Stores churn).
    // pool.max_unused_warm_slots(0);
    //
    // // Keep a small portion of memory resident after dealloc to avoid page-fault storms
    // // on the next instantiation (Linux-only optimization). Tune 4–32 MiB empirically.
    // pool.linear_memory_keep_resident(16 << 20);
    // pool.table_keep_resident(0);
    //
    // pool.max_memory_size(128 << 20);
    //
    // // Speed up reset by batching decommits a bit (defaults to 1).
    // pool.decommit_batch_size(128);

    // --- Engine config
    let mut cfg = wasmtime::Config::new();

    cfg.strategy(wasmtime::Strategy::Cranelift);
    cfg.collector(wasmtime::Collector::Null);
    cfg.max_wasm_stack(N_MAX_STACK_SIZE * size_of::<u32>());
    cfg.async_support(true);

    // Use the pooling allocator
    // cfg.allocation_strategy(InstanceAllocationStrategy::Pooling(pool));

    // Keep memories 32-bit unless you truly need >4GiB (faster codegen & cheaper pools).
    cfg.wasm_memory64(false);

    // Make (re)instantiation cheap:
    cfg.memory_init_cow(true); // COW data segments: very fast initial memory image

    // Shrink default gigantic reservations; must be >= `max_memory_size` you allow below.
    // 128 MiB is a good starting point if your modules’ max <= 128 MiB.
    // cfg.memory_reservation(128 << 20);

    // Guard sizes: keep small to reduce VA pressure during churn.
    // cfg.memory_guard_size(64 << 10); // small guard
    // cfg.memory_reservation_for_growth(64 << 10);

    // Usual perf toggles:
    cfg.cranelift_opt_level(wasmtime::OptLevel::SpeedAndSize);
    cfg.parallel_compilation(true);
    cfg.consume_fuel(false);
    cfg.debug_info(false);
    cfg.wasm_backtrace(false);
    cfg.wasm_simd(true);

    // use caching for artifacts
    #[cfg(feature = "cache-compiled-artifacts")]
    {
        use directories::ProjectDirs;
        use std::path::PathBuf;
        use wasmtime::{Cache, CacheConfig};
        let project_dirs = ProjectDirs::from("com", "bytecodealliance", "wasmtime").unwrap();
        let cache_dir = project_dirs.cache_dir();
        std::fs::create_dir_all(cache_dir).expect("failed to create cache dir");
        let mut cache_config = CacheConfig::default();
        cache_config.with_directory(PathBuf::from(cache_dir));
        let cache = Cache::new(cache_config).expect("failed to create cache config");
        cfg.cache(Some(cache));
    }

    Engine::new(&cfg).expect("failed to create engine")
}

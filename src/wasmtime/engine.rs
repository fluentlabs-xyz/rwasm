use crate::{ImportLinker, N_MAX_STACK_SIZE};
use alloc::rc::Rc;

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::{Arc, OnceLock};
use wasmtime::{
    Config, Engine, LinearFuelParams, OptLevel, QuadraticFuelParams, Strategy, SyscallFuelParams,
    SyscallName,
};

static ENGINE: OnceLock<Engine> = OnceLock::new();

pub fn wasmtime_engine() -> &'static Engine {
    ENGINE.get_or_init(factory_wasmtime_engine)
}

fn factory_wasmtime_engine() -> Engine {
    factory_wasmtime_engine_with_linker(None, false)
}

pub fn wasmtime_engine_with_linker(
    import_linker: Option<Arc<ImportLinker>>,
    consume_fuel: bool,
) -> &'static Engine {
    ENGINE.get_or_init(|| factory_wasmtime_engine_with_linker(import_linker, consume_fuel))
}

#[cfg(test)]
pub fn wasmtime_new_engine_with_linker(
    import_linker: Option<Arc<ImportLinker>>,
    consume_fuel: bool,
) -> Engine {
    factory_wasmtime_engine_with_linker(import_linker, consume_fuel)
}

fn factory_wasmtime_engine_with_linker(
    import_linker: Option<Arc<ImportLinker>>,
    consume_fuel: bool,
) -> Engine {
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
    cfg.consume_fuel(consume_fuel);
    // Enable debug info and backtrace for debug mode
    #[cfg(debug_assertions)]
    {
        cfg.debug_info(true);
        cfg.wasm_backtrace(true);
    }
    cfg.wasm_simd(true);

    if let Some(import_linker) = import_linker {
        let mut syscall_params = HashMap::new();
        for (import_name, import_entity) in import_linker.iter() {
            match import_entity.syscall_fuel_param {
                crate::SyscallFuelParams::None => {}
                crate::SyscallFuelParams::Const(base) => {
                    syscall_params.insert(
                        SyscallName {
                            module: import_name.module.to_string(),
                            name: import_name.field.to_string(),
                        },
                        SyscallFuelParams::Const(base),
                    );
                }
                crate::SyscallFuelParams::LinearFuel(crate::LinearFuelParams {
                    base_fuel,
                    param_index,
                    word_cost,
                    max_linear,
                }) => {
                    syscall_params.insert(
                        SyscallName {
                            module: import_name.module.to_string(),
                            name: import_name.field.to_string(),
                        },
                        SyscallFuelParams::LinearFuel(LinearFuelParams {
                            base_fuel,
                            word_cost,
                            linear_param_index: param_index,
                            max_linear,
                        }),
                    );
                }
                crate::SyscallFuelParams::QuadraticFuel(crate::QuadraticFuelParams {
                    param_index,
                    word_cost,
                    divisor,
                    max_quadratic,
                    fuel_denom_rate,
                }) => {
                    syscall_params.insert(
                        SyscallName {
                            module: import_name.module.to_string(),
                            name: import_name.field.to_string(),
                        },
                        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
                            local_depth: param_index,
                            word_cost,
                            divisor,
                            max_quadratic,
                            fuel_denom_rate,
                        }),
                    );
                }
            }
        }
        cfg.syscall_fuel_params(syscall_params);
    }

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

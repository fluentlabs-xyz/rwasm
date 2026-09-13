use crate::{CompilationConfig, N_MAX_STACK_SIZE};
use std::mem::size_of;
use wasmtime::{Config, Engine, OptLevel, Strategy};

/// Builds a Wasmtime engine for `compilation_config`.
///
/// The engine bakes in the config's fuel metering and stack limit, so an engine is never shared
/// between configs: each compiled module carries the engine it was built with, and the module
/// cache keys on the config identity. The syscall fuel schedule is deliberately *not* handed to
/// the engine: Cranelift can only charge it at direct `call`/`return_call` sites, which leaves
/// `call_indirect`, `return_call_indirect`, an exported import and a `start` import unmetered.
/// It travels with the [`crate::wasmtime::WasmtimeModule`] instead and is charged by the host
/// trampolines, which every path into an import goes through.
pub fn wasmtime_engine(compilation_config: &CompilationConfig) -> Engine {
    let mut cfg = Config::new();
    cfg.strategy(Strategy::Cranelift);
    cfg.collector(wasmtime::Collector::Null);

    // rWasm stack size is defined in 32-bit slots; Wasmtime expects bytes.
    cfg.max_wasm_stack(N_MAX_STACK_SIZE * size_of::<u32>());

    // Leave these alone (defaults are already tuned for 64-bit hosts):
    // - memory_reservation: big VA reservation (e.g. ~4GiB) enabling most bounds checks to disappear
    // - memory_guard_size: guard region (e.g. 32MiB) enabling “trap instead of check” for small offsets
    // cfg.memory_reservation(...);
    // cfg.memory_guard_size(...);

    cfg.wasm_memory64(false);
    cfg.memory_init_cow(false);
    cfg.cranelift_opt_level(OptLevel::Speed);
    cfg.parallel_compilation(true);

    // Mirror `CompilationConfig::wasm_features()`: the Wasmtime engine is enabled by default for
    // proposals the rwasm translator does not implement (SIMD, multi-memory, threads, ...), which
    // used to let a module the rwasm compiler rejects run on this backend. `StrategyDefinition`
    // also compiles with the rwasm compiler first; pinning the features here keeps the low-level
    // `compile_wasmtime_module` entry point — and therefore every caller of it — on the same
    // language.
    cfg.wasm_multi_value(true);
    cfg.wasm_bulk_memory(true);
    cfg.wasm_reference_types(true);
    cfg.wasm_tail_call(true);
    cfg.wasm_extended_const(true);
    cfg.wasm_simd(false);
    cfg.wasm_relaxed_simd(false);
    cfg.wasm_threads(false);
    cfg.wasm_multi_memory(false);
    cfg.wasm_exceptions(false);
    cfg.wasm_component_model(false);
    cfg.wasm_gc(false);
    cfg.wasm_wide_arithmetic(false);
    cfg.wasm_custom_page_sizes(false);

    // Fuel accounting is handled externally via RuntimeContext.
    cfg.consume_fuel(compilation_config.consume_fuel);

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

    Engine::new(&cfg).unwrap()
}

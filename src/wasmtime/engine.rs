use crate::{CompilationConfig, N_MAX_STACK_SIZE};
use rwasm_fuel_policy::SyscallName;
use std::{collections::HashMap, mem::size_of, sync::OnceLock};
use wasmtime::{Config, Engine, OptLevel, Strategy};

/// Returns the shared Wasmtime engine instance.
///
/// The engine is configured once and reused globally.
/// Fuel metering is disabled (`consume_fuel(false)`) because fuel is accounted
/// inside `RuntimeContext` and system runtimes are expected to self-manage.
pub fn wasmtime_shared_engine(compilation_config: &CompilationConfig) -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| wasmtime_engine(compilation_config))
}

pub fn wasmtime_engine(compilation_config: &CompilationConfig) -> Engine {
    let mut cfg = Config::new();
    cfg.strategy(Strategy::Cranelift);
    cfg.collector(wasmtime::Collector::Null);

    // rWasm stack size is defined in 32-bit slots; Wasmtime expects bytes.
    cfg.max_wasm_stack(N_MAX_STACK_SIZE * size_of::<u32>());

    cfg.async_support(false);
    cfg.wasm_memory64(false);
    cfg.memory_init_cow(false);
    cfg.cranelift_opt_level(OptLevel::Speed);
    cfg.parallel_compilation(true);

    // Fuel accounting is handled externally via RuntimeContext.
    cfg.consume_fuel(compilation_config.consume_fuel);

    if let Some(import_linker) = compilation_config
        .import_linker
        .as_ref()
        .filter(|_| compilation_config.builtins_consume_fuel)
    {
        let mut syscall_params = HashMap::new();
        for (import_name, import_entity) in import_linker.iter() {
            let syscall_name = SyscallName {
                module: import_name.module.to_string(),
                name: import_name.field.to_string(),
            };
            syscall_params.insert(syscall_name, import_entity.syscall_fuel_param);
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

    Engine::new(&cfg).unwrap()
}

use crate::{
    CompilationConfig, N_MAX_RECURSION_DEPTH, N_MAX_STACK_SIZE, N_STACK_TRAMPOLINE_HEADROOM,
};
use wasmtime::{Config, Engine, OptLevel, Strategy};

/// Native stack bytes Cranelift needs per 32-bit slot of the rwasm value-stack window.
///
/// Every live value is kept in an 8-byte spill slot whatever its Wasm width, so a frame of `i32`
/// values costs twice its rwasm size (measured: 8.2 bytes per slot for a maximal frame of live
/// `i32` locals, 4.1 for `i64`). Sizing the stack as one byte per slot byte, as it used to be,
/// trapped `StackOverflow` at entry on a frame the compiler accepts and the rwasm VM runs.
const NATIVE_BYTES_PER_SLOT: usize = 8;

/// Native stack bytes allowed per frame of the deepest call chain, on top of the spill budget
/// above: return address, frame pointer, callee-saved registers and outgoing arguments.
///
/// Measured on aarch64 with Cranelift at `OptLevel::Speed`: a frame with no live values costs
/// 49 bytes; across recursive chains that fill the rwasm window (1 to 60 live `i32` locals per
/// frame, 120 to 1023 frames), the native stack divided by the frame count never exceeded 160
/// bytes, spilled values included. The allowance takes that whole figure, so the spills of a
/// deep chain are counted twice and the budget stays conservative for other targets.
const NATIVE_BYTES_PER_FRAME: usize = 160;

/// The native stack the Wasmtime backend gives Wasm code.
///
/// The rwasm VM bounds an execution by its value-stack window (`N_MAX_STACK_SIZE` plus the
/// headroom for a compiler-injected frame) and by `N_MAX_RECURSION_DEPTH` frames. This holds
/// every execution that window admits, so a module the rwasm VM runs never runs out of native
/// stack here. The reverse stays unsynchronized (see `docs/pipeline.md`): a chain the rwasm
/// window rejects may still fit here.
pub const WASMTIME_MAX_WASM_STACK: usize = (N_MAX_STACK_SIZE + N_STACK_TRAMPOLINE_HEADROOM)
    * NATIVE_BYTES_PER_SLOT
    + N_MAX_RECURSION_DEPTH * NATIVE_BYTES_PER_FRAME;

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

    cfg.max_wasm_stack(WASMTIME_MAX_WASM_STACK);

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
    cfg.wasm_wide_arithmetic(true);
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

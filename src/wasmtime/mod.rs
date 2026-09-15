mod engine;
mod instance;

mod context;
mod import_linker;
mod syscall_handler;
#[cfg(test)]
mod tests;
mod types;

pub use self::{
    context::WasmtimeCaller, import_linker::wasmtime_import_linker, instance::WasmtimeExecutor,
    syscall_handler::wasmtime_syscall_handler,
};
use crate::{
    wasmtime::{context::WrappedContext, engine::wasmtime_engine},
    CompilationConfig, CompilationError, ImportName, N_MAX_TABLE_SIZE,
};
use lru::LruCache;
use rwasm_fuel_policy::SyscallFuelParams;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    ops::Deref,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};
use wasmparser::{Parser, Payload};

pub type WasmtimeLinker<T> = wasmtime::Linker<WrappedContext<T>>;

/// A compiled Wasmtime module together with the syscall fuel schedule it was compiled under.
///
/// The rwasm compiler bakes `SyscallFuelParams` into the import trampoline of the bytecode, so
/// every way of reaching an import (`call`, `call_indirect`, tail calls, an exported or `start`
/// import) charges it. The Wasmtime engine used to receive the same schedule and charge it at
/// Cranelift `call` sites, which left every other path unmetered. The schedule now travels with
/// the module and is charged by the host trampolines that [`WasmtimeExecutor`] installs, which
/// are the only way any of those paths can reach the host.
///
/// Derefs to the underlying [`wasmtime::Module`], so exports and types are read as before.
#[derive(Clone, Debug)]
pub struct WasmtimeModule {
    module: wasmtime::Module,
    /// Syscall fuel of every import the compiling config's linker knew, by import name, when the
    /// config enabled `builtins_consume_fuel`; empty otherwise.
    syscall_fuel: Arc<HashMap<ImportName, SyscallFuelParams>>,
}

impl WasmtimeModule {
    /// Pairs a module compiled by `compilation_config`'s engine with that config's syscall fuel
    /// schedule.
    pub fn new(module: wasmtime::Module, compilation_config: &CompilationConfig) -> Self {
        Self {
            module,
            syscall_fuel: Arc::new(syscall_fuel_schedule(compilation_config)),
        }
    }

    /// The underlying Wasmtime module.
    pub fn module(&self) -> &wasmtime::Module {
        &self.module
    }

    /// Returns the underlying Wasmtime module, dropping the syscall fuel schedule.
    pub fn into_module(self) -> wasmtime::Module {
        self.module
    }

    /// The syscall fuel charged for each import, by import name.
    pub fn syscall_fuel(&self) -> &HashMap<ImportName, SyscallFuelParams> {
        &self.syscall_fuel
    }
}

impl Deref for WasmtimeModule {
    type Target = wasmtime::Module;

    fn deref(&self) -> &Self::Target {
        &self.module
    }
}

/// A bare module charges no syscall fuel, like a config with `builtins_consume_fuel` off.
impl From<wasmtime::Module> for WasmtimeModule {
    fn from(module: wasmtime::Module) -> Self {
        Self {
            module,
            syscall_fuel: Arc::default(),
        }
    }
}

/// The syscall fuel a config compiles into its import trampolines: the linker's parameters when
/// `builtins_consume_fuel` is set (mirrors `ModuleParser::process_imports`), nothing otherwise.
fn syscall_fuel_schedule(
    compilation_config: &CompilationConfig,
) -> HashMap<ImportName, SyscallFuelParams> {
    compilation_config
        .import_linker
        .as_ref()
        .filter(|_| compilation_config.builtins_consume_fuel)
        .map(|import_linker| {
            import_linker
                .iter()
                .map(|(name, entity)| (name, entity.syscall_fuel_param))
                .collect()
        })
        .unwrap_or_default()
}

/// Loads a module from bytes produced by `wasmtime::Module::serialize` on a compatible build.
///
/// # Safety
///
/// This is a native-code load, not a parse. `wasmtime::Module::deserialize` trusts
/// `wasmtime_binary` to be an artifact it produced itself and performs no authentication, so
/// attacker-influenced bytes are arbitrary code execution rather than a decode error. The caller
/// must guarantee that the bytes come from a trusted producer and reached this call with their
/// integrity intact (for example, verified by a keyed MAC that the producer computed), and must
/// never pass unauthenticated cache contents or any bytes received from a network peer.
pub unsafe fn deserialize_wasmtime_module(
    compilation_config: CompilationConfig,
    wasmtime_binary: impl AsRef<[u8]>,
) -> wasmtime::Result<WasmtimeModule> {
    #[cfg(feature = "debug-print")]
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    // SAFETY: forwarded to the caller, see the function's safety contract.
    let module = unsafe { wasmtime::Module::deserialize(&engine, wasmtime_binary) };
    #[cfg(feature = "debug-print")]
    println!("{:?}", start.elapsed());
    module.map(|module| WasmtimeModule::new(module, &compilation_config))
}

/// Applies the rwasm compile-time resource caps to a wasm binary before Wasmtime compiles it.
///
/// `RwasmModule::compile` rejects a module whose declared initial memory exceeds
/// `config.max_allowed_memory_pages` or whose table declares more than [`N_MAX_TABLE_SIZE`]
/// elements. Wasmtime has no compile-time equivalent: its store limiter acts at instantiation,
/// against a cap the runtime picks independently of the compiler. Without this check a deployment
/// whose runtime cap exceeds the compile cap accepts a module on the Wasmtime strategy that the
/// rwasm strategy rejects at compile time.
///
/// Only the section headers up to the code section are read, so this costs a fraction of the
/// compilation itself.
fn check_compile_limits(
    config: &CompilationConfig,
    wasm_binary: &[u8],
) -> Result<(), CompilationError> {
    let mut total_pages: u32 = 0;
    for payload in Parser::new(0).parse_all(wasm_binary) {
        match payload? {
            Payload::MemorySection(section) => {
                for memory_type in section.into_iter() {
                    let initial_pages = u32::try_from(memory_type?.initial)
                        .map_err(|_| CompilationError::MaxReadonlyDataReached)?;
                    total_pages = total_pages.saturating_add(initial_pages);
                    // inclusive, like `SegmentBuilder::add_memory_pages`
                    if total_pages > config.max_allowed_memory_pages {
                        return Err(CompilationError::MaxReadonlyDataReached);
                    }
                }
            }
            Payload::TableSection(section) => {
                for table_type in section.into_iter() {
                    let size = table_type?.initial;
                    if size > N_MAX_TABLE_SIZE {
                        return Err(CompilationError::TableSizeExceedsLimit {
                            size,
                            limit: N_MAX_TABLE_SIZE,
                        });
                    }
                }
            }
            // every section this check cares about precedes the code section
            Payload::CodeSectionStart { .. } | Payload::End(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// Compiles a wasm binary with the Wasmtime engine configured by `compilation_config`.
///
/// Beyond Wasmtime's own validation this only enforces the rwasm compile-time resource caps
/// (see [`check_compile_limits`]). The rwasm strategy accepts a strict subset of what Wasmtime
/// accepts (see [`crate::RwasmModule::compile`]); callers that need both strategies to agree on
/// the accepted language go through [`crate::StrategyDefinition::new_as_wasmtime`], which runs
/// the rwasm front end first.
pub fn compile_wasmtime_module(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> Result<WasmtimeModule, CompilationError> {
    let wasm_binary = wasm_binary.as_ref();
    check_compile_limits(&compilation_config, wasm_binary)?;
    #[cfg(feature = "debug-print")]
    print!("compiling wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = wasmtime::Module::new(&engine, wasm_binary)
        .map_err(CompilationError::WasmtimeCompilationFailed);
    #[cfg(feature = "debug-print")]
    println!("{:?}", start.elapsed());
    module.map(|module| WasmtimeModule::new(module, &compilation_config))
}

const MAX_CACHED_COMPILED_MODULES: usize = 10_000;

/// Like [`compile_wasmtime_module`], but memoizes the compiled module in a process-wide LRU cache.
///
/// The entry is keyed by `module_caching_key` together with the config's
/// [`CompilationConfig::codegen_identity`]. A compiled module embeds its engine, which bakes in
/// the config's fuel schedule and stack limit, and carries the config's syscall fuel parameters
/// (see [`WasmtimeModule`]), so two callers sharing a key but not a config get two modules
/// instead of the second silently running on the first caller's metering.
///
/// Entries made here are validated by Wasmtime only and never satisfy a lookup from
/// [`crate::StrategyDefinition::new_as_wasmtime`], which caches under its own
/// [`CachePolicy`].
pub fn compile_wasmtime_module_cached(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
    module_caching_key: [u8; 32],
) -> Result<WasmtimeModule, CompilationError> {
    let wasm_binary = wasm_binary.as_ref();
    compile_wasmtime_module_cached_with(
        compilation_config,
        module_caching_key,
        wasm_identity(wasm_binary),
        CachePolicy::WasmtimeOnly,
        |config| compile_wasmtime_module(config, wasm_binary),
    )
}

/// Which front end validated a cached module.
///
/// The policy is part of the cache key: a module that only Wasmtime validated must never be
/// returned to a caller whose contract includes rwasm validation, or a module rwasm rejects (SIMD,
/// a start section, a missing entrypoint) could be primed under a key by one caller and then
/// accepted under the same key by the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CachePolicy {
    /// Validated by Wasmtime plus the rwasm compile-time resource caps only.
    WasmtimeOnly,
    /// Validated by the full rwasm front end before Wasmtime compiled it.
    RwasmValidated,
}

/// The key of a cached module: the validation policy, the caller's key, the identity of the config
/// it was compiled with and the identity of the bytecode itself.
///
/// The bytecode hash is what keeps a reused caller key from returning a module compiled from other
/// bytes: a host that keys by contract address and upgrades the contract in place would otherwise
/// execute the previous version's code.
type ModuleCacheKey = (CachePolicy, [u8; 32], [u8; 32], [u8; 32]);

/// Hashes the compiled input, so the cache key identifies the module and not just its address.
pub(crate) fn wasm_identity(wasm_binary: &[u8]) -> [u8; 32] {
    use tiny_keccak::{Hasher, Keccak};
    let mut hasher = Keccak::v256();
    hasher.update(wasm_binary);
    let mut identity = [0u8; 32];
    hasher.finalize(&mut identity);
    identity
}

/// Returns the module cached under `module_caching_key`, `policy` and `compilation_config`, or
/// compiles it with `compile` and caches the result. The cache lock is held across `compile`, so
/// concurrent callers with the same key compile once.
pub(crate) fn compile_wasmtime_module_cached_with<E>(
    compilation_config: CompilationConfig,
    module_caching_key: [u8; 32],
    wasm_identity: [u8; 32],
    policy: CachePolicy,
    compile: impl FnOnce(CompilationConfig) -> Result<WasmtimeModule, E>,
) -> Result<WasmtimeModule, E> {
    static COMPILED_MODULES: OnceLock<Mutex<LruCache<ModuleCacheKey, WasmtimeModule>>> =
        OnceLock::new();
    let compiled_modules = COMPILED_MODULES.get_or_init(|| {
        Mutex::new(LruCache::new(
            NonZeroUsize::new(MAX_CACHED_COMPILED_MODULES).unwrap(),
        ))
    });
    let cache_key = (
        policy,
        module_caching_key,
        compilation_config.codegen_identity(),
        wasm_identity,
    );
    let mut guard = compiled_modules.lock().unwrap();
    if let Some(module) = guard.get(&cache_key) {
        return Ok(module.clone());
    }
    let module = compile(compilation_config)?;
    guard.push(cache_key, module.clone());
    Ok(module)
}

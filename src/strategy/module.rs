use crate::{
    always_failing_syscall_handler, CompilationConfig, CompilationError, ExecutionEngine,
    ImportLinker, RwasmInstance, RwasmModule, RwasmStore, StoreTr, StrategyError, SyscallHandler,
    TrapCode, Value,
};
use alloc::{sync::Arc, vec::Vec};

#[derive(Clone)]
pub enum StrategyDefinition {
    Rwasm {
        engine: ExecutionEngine,
        module: RwasmModule,
    },
    #[cfg(feature = "wasmtime")]
    Wasmtime {
        // A wasmtime module that stores engine inside
        module: crate::wasmtime::WasmtimeModule,
    },
}

impl StrategyDefinition {
    /// Compiles a wasm binary with whichever strategy the crate was built with (Wasmtime when the
    /// `wasmtime` feature is on, the rwasm VM otherwise).
    ///
    /// Because the strategy is a build-time choice, the config's fuel semantics must not depend
    /// on it: a config that enables the rwasm-only fuel injections (the plain
    /// [`CompilationConfig::default`]) is rejected with
    /// [`CompilationError::StrategyIncompatibleConfig`] regardless of the feature set, so a
    /// module never silently burns different fuel on the two strategies. Use
    /// [`CompilationConfig::default_strategy_compatible`].
    pub fn new(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
        #[allow(unused_variables)] module_caching_key: Option<[u8; 32]>,
    ) -> Result<Self, CompilationError> {
        Self::ensure_strategy_compatible(&compilation_config)?;
        #[cfg(feature = "wasmtime")]
        return Self::new_as_wasmtime(compilation_config, wasm_binary, module_caching_key);
        #[cfg(not(feature = "wasmtime"))]
        return Self::new_as_rwasm(compilation_config, wasm_binary);
    }

    /// Rejects a config whose fuel accounting depends on the strategy.
    ///
    /// `is_strategy_compatible` used to be advisory only, so a divergent `default()` config was
    /// accepted and the rwasm-only injections were silently dropped on the Wasmtime strategy.
    pub(crate) fn ensure_strategy_compatible(
        compilation_config: &CompilationConfig,
    ) -> Result<(), CompilationError> {
        if compilation_config.is_strategy_compatible() {
            Ok(())
        } else {
            Err(CompilationError::StrategyIncompatibleConfig)
        }
    }

    pub fn new_as_rwasm(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
    ) -> Result<Self, CompilationError> {
        let (module, _) = RwasmModule::compile(compilation_config, wasm_binary.as_ref())?;
        Ok(Self::Rwasm {
            module,
            engine: ExecutionEngine::new(),
        })
    }

    /// Compiles a wasm binary for the Wasmtime strategy.
    ///
    /// The binary is first run through the rwasm front end ([`RwasmModule::compile`]) and only
    /// then handed to Wasmtime. Wasmtime accepts a superset of the rwasm language, so without
    /// that step the two strategies would disagree on which modules compile at all: rwasm
    /// enforces the memory and table caps, the start-section and import rules and the accepted
    /// proposal set, and Wasmtime does not. A module that rwasm rejects is rejected here with the
    /// same error, and a Wasmtime failure is reported as
    /// [`CompilationError::WasmtimeCompilationFailed`] instead of a panic.
    ///
    /// With a `module_caching_key` the validation runs only on a cache miss.
    ///
    /// The Wasmtime engine does not implement `consume_fuel_for_bulk_ops` or
    /// `consume_fuel_for_params_and_locals`, so a config enabling either is rejected with
    /// [`CompilationError::StrategyIncompatibleConfig`] rather than silently under-metered.
    #[cfg(feature = "wasmtime")]
    pub fn new_as_wasmtime(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
        module_caching_key: Option<[u8; 32]>,
    ) -> Result<Self, CompilationError> {
        use crate::wasmtime::{
            compile_wasmtime_module, compile_wasmtime_module_cached_with, CachePolicy,
        };
        Self::ensure_strategy_compatible(&compilation_config)?;
        let wasm_binary = wasm_binary.as_ref();
        let compile = |config: CompilationConfig| -> Result<_, CompilationError> {
            RwasmModule::compile(config.clone(), wasm_binary)?;
            compile_wasmtime_module(config, wasm_binary)
        };
        let module = match module_caching_key {
            Some(module_caching_key) => compile_wasmtime_module_cached_with(
                compilation_config,
                module_caching_key,
                CachePolicy::RwasmValidated,
                compile,
            )?,
            None => compile(compilation_config)?,
        };
        Ok(Self::Wasmtime { module })
    }

    pub fn default_executor(&self) -> Result<StrategyExecutor<()>, TrapCode> {
        self.create_executor::<()>(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            None,
        )
    }

    pub fn create_executor<T>(
        &self,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> Result<StrategyExecutor<T>, TrapCode> {
        match self {
            StrategyDefinition::Rwasm { engine, module } => {
                let mut store = RwasmStore::new(
                    import_linker.clone(),
                    context,
                    syscall_handler,
                    fuel_limit,
                    max_allowed_memory_pages,
                );
                let instance =
                    import_linker.instantiate(&mut store, engine.clone(), module.clone())?;
                Ok(StrategyExecutor::Rwasm { store, instance })
            }
            #[cfg(feature = "wasmtime")]
            StrategyDefinition::Wasmtime { module } => {
                let executor = crate::wasmtime::WasmtimeExecutor::new(
                    module.clone(),
                    import_linker,
                    context,
                    syscall_handler,
                    fuel_limit,
                    max_allowed_memory_pages,
                )?;
                Ok(StrategyExecutor::Wasmtime { executor })
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum StrategyExecutor<T: 'static> {
    Rwasm {
        store: RwasmStore<T>,
        instance: RwasmInstance,
    },
    #[cfg(feature = "wasmtime")]
    Wasmtime {
        // An executor for wasmtime
        executor: crate::wasmtime::WasmtimeExecutor<T>,
    },
}

impl<T: 'static> StoreTr<T> for StrategyExecutor<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.memory_read(offset, buffer),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.memory_read(offset, buffer),
        }
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.memory_read_into_vec(offset, length),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => {
                executor.memory_read_into_vec(offset, length)
            }
        }
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.memory_write(offset, buffer),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.memory_write(offset, buffer),
        }
    }

    fn data_mut(&mut self) -> &mut T {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.data_mut(),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.data_mut(),
        }
    }

    fn data(&self) -> &T {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.data(),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.data(),
        }
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.try_consume_fuel(delta),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.try_consume_fuel(delta),
        }
    }

    fn remaining_fuel(&self) -> Option<u64> {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.remaining_fuel(),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.remaining_fuel(),
        }
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        match self {
            StrategyExecutor::Rwasm { store, .. } => store.reset_fuel(new_fuel_limit),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.reset_fuel(new_fuel_limit),
        }
    }
}

impl<T: 'static> StrategyExecutor<T> {
    pub fn compile_and_instantiate(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
        module_caching_key: Option<[u8; 32]>,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
    ) -> Result<Self, StrategyError> {
        let definition =
            StrategyDefinition::new(compilation_config, wasm_binary, module_caching_key)?;
        let executor = definition.create_executor(
            import_linker,
            context,
            syscall_handler,
            fuel_limit,
            None,
        )?;
        Ok(executor)
    }

    pub fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, instance } => instance.execute(store, params, result),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.execute(func_name, params, result),
        }
    }

    pub fn resume(
        &mut self,
        interruption_result: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, instance } => {
                instance.resume(store, interruption_result, result)
            }
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.resume(interruption_result, result),
        }
    }

    pub fn snapshot_memory(&mut self) -> Result<Vec<u8>, TrapCode> {
        match self {
            StrategyExecutor::Rwasm { store, .. } => Ok(store.memory_snapshot()),
            #[cfg(feature = "wasmtime")]
            StrategyExecutor::Wasmtime { executor } => executor.snapshot_memory(),
        }
    }
}

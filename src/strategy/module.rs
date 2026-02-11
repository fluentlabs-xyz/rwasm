use crate::{
    always_failing_syscall_handler, CompilationConfig, CompilationError, ExecutionEngine,
    ImportLinker, RwasmInstance, RwasmModule, RwasmStore, StrategyError, SyscallHandler, TrapCode,
    Value,
};
use alloc::sync::Arc;

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
    pub fn new(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
        module_caching_key: Option<[u8; 32]>,
    ) -> Result<Self, CompilationError> {
        #[cfg(feature = "wasmtime")]
        return Self::new_as_wasmtime(compilation_config, wasm_binary, module_caching_key);
        #[cfg(not(feature = "wasmtime"))]
        return Self::new_as_rwasm(compilation_config, wasm_binary, module_caching_key);
    }

    pub fn new_as_rwasm(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
        binary_caching_key: Option<[u8; 32]>,
    ) -> Result<Self, CompilationError> {
        let (module, _) = RwasmModule::compile(compilation_config, wasm_binary.as_ref())?;
        Ok(Self::Rwasm {
            module,
            engine: ExecutionEngine::new(),
        })
    }

    #[cfg(feature = "wasmtime")]
    pub fn new_as_wasmtime(
        compilation_config: CompilationConfig,
        wasm_binary: impl AsRef<[u8]>,
        module_caching_key: Option<[u8; 32]>,
    ) -> Result<Self, CompilationError> {
        use crate::wasmtime::{compile_wasmtime_module, compile_wasmtime_module_cached};
        let module = if let Some(binary_caching_key) = module_caching_key {
            compile_wasmtime_module_cached(compilation_config, wasm_binary, binary_caching_key)
        } else {
            compile_wasmtime_module(compilation_config, wasm_binary)
        };
        let module = module.expect(
            "rwasm: compilation of wasmtime module can't fail since it's followed by rwasm validation rules, or it's a bug (the binary follows rwasm rules?)",
        );
        Ok(Self::Wasmtime { module })
    }

    pub fn default_executor(&self) -> Result<StrategyExecutor<()>, TrapCode> {
        self.create_executor::<()>(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
        )
    }

    pub fn create_executor<T>(
        &self,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
    ) -> Result<StrategyExecutor<T>, TrapCode> {
        match self {
            StrategyDefinition::Rwasm { engine, module } => {
                let mut store =
                    RwasmStore::new(import_linker.clone(), context, syscall_handler, fuel_limit);
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
                );
                Ok(StrategyExecutor::Wasmtime { executor })
            }
        }
    }
}

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
        let executor =
            definition.create_executor(import_linker, context, syscall_handler, fuel_limit)?;
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
}

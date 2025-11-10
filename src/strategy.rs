use crate::{
    wasmi::{WasmiModule, WasmiStore},
    CompilationError, ExecutionEngine, ImportLinker, RwasmCaller, RwasmExecutor, RwasmModule,
    RwasmStore, TrapCode, UntypedValue, Value, WasmiCaller,
};
#[cfg(feature = "wasmtime")]
use crate::{WasmtimeCaller, WasmtimeModule, WasmtimeStore};
use alloc::sync::Arc;

pub trait Store<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode>;

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode>;

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R;

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R;

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode>;

    fn remaining_fuel(&self) -> Option<u64>;
}

pub trait Caller<T>: Store<T> {
    // #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn program_counter(&self) -> u32;

    // #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn stack_push(&mut self, value: UntypedValue);

    fn consume_fuel(&mut self, fuel: u64) -> Result<(), TrapCode>;
}

pub type SyscallHandler<T> =
    fn(&mut TypedCaller<'_, T>, u32, &[Value], &mut [Value]) -> Result<(), TrapCode>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyscallFuelParams {
    pub base_fuel: u64,
    pub param_index: u64,
    pub linear_fuel: u64,
}

pub fn always_failing_syscall_handler<T: 'static + Send + Sync>(
    _caller: &mut TypedCaller<'_, T>,
    _func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::UnknownExternalFunction)
}

pub enum TypedCaller<'a, T: 'static + Send + Sync> {
    Rwasm(RwasmCaller<'a, T>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(WasmtimeCaller<'a, T>),
    Wasmi(WasmiCaller<'a, T>),
}

impl<'a, T: Send + Sync> TypedCaller<'a, T> {
    pub fn as_rwasm_mut(&mut self) -> &mut RwasmCaller<'a, T> {
        match self {
            TypedCaller::Rwasm(store) => store,
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    pub fn as_rwasm_ref(&self) -> &RwasmCaller<'a, T> {
        match self {
            TypedCaller::Rwasm(store) => store,
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "wasmtime")]
    pub fn as_wasmtime_mut(&mut self) -> &mut WasmtimeCaller<'a, T> {
        match self {
            TypedCaller::Wasmtime(store) => store,
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "wasmtime")]
    pub fn as_wasmtime_ref(&self) -> &WasmtimeCaller<'a, T> {
        match self {
            TypedCaller::Wasmtime(store) => store,
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "wasmtime")]
    pub fn into_wasmtime(self) -> WasmtimeCaller<'a, T> {
        match self {
            TypedCaller::Wasmtime(store) => store,
            _ => unreachable!(),
        }
    }
}

impl<'a, T: Send + Sync> Store<T> for TypedCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(store) => store.memory_read(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.memory_read(offset, buffer),
            TypedCaller::Wasmi(store) => store.memory_read(offset, buffer),
        }
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(store) => store.memory_write(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.memory_write(offset, buffer),
            TypedCaller::Wasmi(store) => store.memory_write(offset, buffer),
        }
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        match self {
            TypedCaller::Rwasm(store) => store.context_mut(func),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.context_mut(func),
            TypedCaller::Wasmi(store) => store.context_mut(func),
        }
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        match self {
            TypedCaller::Rwasm(store) => store.context(func),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.context(func),
            TypedCaller::Wasmi(store) => store.context(func),
        }
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(store) => store.try_consume_fuel(delta),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.try_consume_fuel(delta),
            TypedCaller::Wasmi(store) => store.try_consume_fuel(delta),
        }
    }

    fn remaining_fuel(&self) -> Option<u64> {
        match self {
            TypedCaller::Rwasm(store) => store.remaining_fuel(),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.remaining_fuel(),
            TypedCaller::Wasmi(store) => store.remaining_fuel(),
        }
    }
}

impl<'a, T: Send + Sync> Caller<T> for TypedCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        match self {
            TypedCaller::Rwasm(store) => store.program_counter(),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.program_counter(),
            TypedCaller::Wasmi(store) => store.program_counter(),
        }
    }

    fn stack_push(&mut self, value: UntypedValue) {
        match self {
            TypedCaller::Rwasm(store) => store.stack_push(value),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.stack_push(value),
            TypedCaller::Wasmi(store) => store.stack_push(value),
        }
    }

    fn consume_fuel(&mut self, fuel: u64) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(caller) => caller.consume_fuel(fuel),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(caller) => caller.consume_fuel(fuel),
            TypedCaller::Wasmi(caller) => caller.consume_fuel(fuel),
        }
    }
}

pub enum TypedExecutor<'a, T: Send + Sync + 'static> {
    RwasmExecutor(RwasmExecutor<'a, T>),
}

impl<'a, T: Send + Sync + 'static> TypedExecutor<'a, T> {
    pub fn advance_ip(&mut self, ip: usize) {
        match self {
            TypedExecutor::RwasmExecutor(executor) => executor.advance_ip(ip),
        }
    }

    pub fn run(&mut self, params: &[Value], result: &mut [Value]) -> Result<(), TrapCode> {
        match self {
            TypedExecutor::RwasmExecutor(executor) => executor.run(params, result),
        }
    }
}

pub enum Strategy {
    Rwasm {
        module: RwasmModule,
        engine: ExecutionEngine,
    },
    #[cfg(feature = "wasmtime")]
    Wasmtime {
        module: WasmtimeModule,
    },
    Wasmi {
        module: WasmiModule,
    },
}

pub enum TypedStore<T: 'static + Send + Sync> {
    Rwasm(RwasmStore<T>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(WasmtimeStore<T>),
    Wasmi(WasmiStore<T>),
}

impl<T: Send + Sync> Store<T> for TypedStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        match self {
            TypedStore::Rwasm(store) => store.memory_read(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.memory_read(offset, buffer),
            TypedStore::Wasmi(store) => store.memory_read(offset, buffer),
        }
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        match self {
            TypedStore::Rwasm(store) => store.memory_write(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.memory_write(offset, buffer),
            TypedStore::Wasmi(store) => store.memory_write(offset, buffer),
        }
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        match self {
            TypedStore::Rwasm(store) => store.context_mut(func),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.context_mut(func),
            TypedStore::Wasmi(store) => store.context_mut(func),
        }
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        match self {
            TypedStore::Rwasm(store) => store.context(func),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.context(func),
            TypedStore::Wasmi(store) => store.context(func),
        }
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        match self {
            TypedStore::Rwasm(store) => store.try_consume_fuel(delta),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.try_consume_fuel(delta),
            TypedStore::Wasmi(store) => store.try_consume_fuel(delta),
        }
    }

    fn remaining_fuel(&self) -> Option<u64> {
        match self {
            TypedStore::Rwasm(store) => store.remaining_fuel(),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.remaining_fuel(),
            TypedStore::Wasmi(store) => store.remaining_fuel(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FuelConfig {
    pub fuel_limit: Option<u64>,
}

impl FuelConfig {
    pub fn with_fuel_limit(mut self, fuel_limit: u64) -> Self {
        self.fuel_limit = Some(fuel_limit);
        self
    }
}

impl<T: Send + Sync> TypedStore<T> {
    pub fn reset(&mut self, keep_flags: bool) {
        match self {
            TypedStore::Rwasm(store) => store.reset(keep_flags),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(_) => {}
            TypedStore::Wasmi(_) => {}
        }
    }
}

impl Strategy {
    pub fn empty_store(&self) -> TypedStore<()> {
        self.create_store::<()>(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            FuelConfig::default(),
        )
    }

    pub fn create_store<T: Send + Sync>(
        &self,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_config: FuelConfig,
    ) -> TypedStore<T> {
        match self {
            Strategy::Rwasm { engine, .. } => TypedStore::Rwasm(RwasmStore::new(
                import_linker,
                context,
                syscall_handler,
                fuel_config,
            )),
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { module, .. } => TypedStore::Wasmtime(WasmtimeStore::new(
                module.clone(),
                import_linker,
                context,
                syscall_handler,
                fuel_config,
            )),
            Strategy::Wasmi { module } => TypedStore::Wasmi(WasmiStore::new(
                module,
                import_linker,
                context,
                syscall_handler,
                fuel_config,
            )),
        }
    }

    pub fn execute<'a, T: Send + Sync>(
        &'a self,
        store: &mut TypedStore<T>,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
        fuel_config: FuelConfig,
    ) -> Result<(), TrapCode> {
        match self {
            Strategy::Rwasm { module, engine } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };

                engine.execute(store, &module, params, result)
            }
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { .. } => {
                let store = match store {
                    TypedStore::Wasmtime(store) => store,
                    _ => unreachable!(),
                };
                if let Some(s) = store.store.as_mut() {
                    if let Some(fuel) = fuel_config.fuel_limit {
                        s.set_fuel(fuel).unwrap();
                    }
                }

                store.execute(func_name, params, result)
            }
            Strategy::Wasmi { module, .. } => {
                let store = match store {
                    TypedStore::Wasmi(store) => store,
                    _ => unreachable!(),
                };
                store.execute(func_name, params, result)
            }
        }
    }

    pub fn resume<'a, T: Send + Sync>(
        &'a self,
        store: &mut TypedStore<T>,
        interruption_result: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match self {
            Strategy::Rwasm { engine, .. } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };
                engine.resume(store, interruption_result, result)
            }
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { .. } => {
                let store = match store {
                    TypedStore::Wasmtime(store) => store,
                    _ => unreachable!(),
                };
                store.resume(Ok(interruption_result), result)
            }
            Strategy::Wasmi { module, .. } => {
                let store = match store {
                    TypedStore::Wasmi(store) => store,
                    _ => unreachable!(),
                };
                store.resume(interruption_result, result)
            }
        }
    }
}

#[derive(Debug)]
pub enum StrategyError {
    CompilationError(CompilationError),
    TrapCode(TrapCode),
}

impl From<CompilationError> for StrategyError {
    fn from(err: CompilationError) -> Self {
        StrategyError::CompilationError(err)
    }
}
impl From<TrapCode> for StrategyError {
    fn from(err: TrapCode) -> Self {
        StrategyError::TrapCode(err)
    }
}

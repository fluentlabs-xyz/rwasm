use crate::wasmi::WasmiCaller;
use crate::{
    wasmi::{WasmiModule, WasmiStore},
    CompilationError, ExecutionEngine, ExecutorConfig, ImportLinker, RwasmCaller, RwasmModule,
    RwasmStore, TrapCode, UntypedValue, Value,
};
#[cfg(feature = "wasmtime")]
use crate::{WasmtimeCaller, WasmtimeModule, WasmtimeWorker};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::RefCell;

pub trait Store<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode>;

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode>;

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, func: F) -> R;

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R;

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode>;

    fn remaining_fuel(&mut self) -> Option<u64>;
}

pub trait Caller<T>: Store<T> {
    // #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn program_counter(&self) -> u32;

    // #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn stack_push(&mut self, value: UntypedValue);
}

pub type SyscallHandler<T> =
    fn(&mut TypedCaller<'_, T>, u32, &[Value], &mut [Value]) -> Result<(), TrapCode>;

pub fn always_failing_syscall_handler<T: Send + Sync>(
    _caller: &mut TypedCaller<'_, T>,
    _func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::UnknownExternalFunction)
}

pub enum TypedCaller<'a, T: Send + Sync + 'static> {
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
}

impl<'a, T: Send + Sync> Store<T> for TypedCaller<'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
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

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, func: F) -> R {
        match self {
            TypedCaller::Rwasm(store) => store.context_mut(func),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.context_mut(func),
            TypedCaller::Wasmi(store) => store.context_mut(func),
        }
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
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

    fn remaining_fuel(&mut self) -> Option<u64> {
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
}

pub enum Strategy {
    Rwasm {
        module: Rc<RwasmModule>,
        engine: Rc<RefCell<ExecutionEngine>>,
    },
    #[cfg(feature = "wasmtime")]
    Wasmtime {
        module: Rc<WasmtimeModule>,
        resumable: bool,
    },
    Wasmi {
        module: Rc<WasmiModule>,
    },
}

pub enum TypedStore<T: Send + Sync + 'static> {
    Rwasm(RwasmStore<T>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(WasmtimeWorker<T>),
    Wasmi(WasmiStore<T>),
}

impl<T: Send + Sync> Store<T> for TypedStore<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
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

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, func: F) -> R {
        match self {
            TypedStore::Rwasm(store) => store.context_mut(func),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.context_mut(func),
            TypedStore::Wasmi(store) => store.context_mut(func),
        }
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
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

    fn remaining_fuel(&mut self) -> Option<u64> {
        match self {
            TypedStore::Rwasm(store) => store.remaining_fuel(),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.remaining_fuel(),
            TypedStore::Wasmi(store) => store.remaining_fuel(),
        }
    }
}

impl Strategy {
    pub fn empty_store(&self, executor_config: ExecutorConfig) -> TypedStore<()> {
        self.create_store::<()>(
            executor_config,
            Rc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
        )
    }

    pub fn create_store<T: Send + Sync>(
        &self,
        config: ExecutorConfig,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
    ) -> TypedStore<T> {
        match self {
            Strategy::Rwasm { .. } => TypedStore::Rwasm(RwasmStore::new(
                config,
                import_linker,
                context,
                syscall_handler,
            )),
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { module, .. } => TypedStore::Wasmtime(WasmtimeWorker::new(
                module.clone(),
                import_linker,
                context,
                syscall_handler,
                config.fuel_limit,
            )),
            Strategy::Wasmi { module } => TypedStore::Wasmi(WasmiStore::new(
                module,
                import_linker,
                context,
                syscall_handler,
                config.fuel_limit,
            )),
        }
    }

    pub fn execute<'a, T: Send + Sync>(
        &'a self,
        store: &mut TypedStore<T>,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match self {
            Strategy::Rwasm { module, engine } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };
                engine.borrow_mut().execute(store, &module, params, result)
            }
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { resumable, .. } => {
                let store = match store {
                    TypedStore::Wasmtime(store) => store,
                    _ => unreachable!(),
                };
                if *resumable {
                    store.execute(func_name, params, result)
                } else {
                    store.execute_not_resumable(func_name, params, result)
                }
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
            Strategy::Rwasm { module, engine } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };
                engine
                    .borrow_mut()
                    .resume(store, &module, interruption_result, result)
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

    pub fn resume_wth_memory<'a, T: Send + Sync>(
        &'a self,
        store: &mut TypedStore<T>,
        interruption_result: &[Value],
        result: &mut [Value],
        memory_changes: Vec<(u32, Box<[u8]>)>,
    ) -> Result<(), TrapCode> {
        match self {
            Strategy::Rwasm { module, engine } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };
                for (addr, buf) in memory_changes {
                    store.memory_write(addr as usize, &buf)?
                }
                engine
                    .borrow_mut()
                    .resume(store, &module, interruption_result, result)
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
                for (addr, buf) in memory_changes {
                    store.memory_write(addr as usize, &buf)?
                }
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

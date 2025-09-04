use crate::{
    Caller, CompilationConfig, ImportLinker, Store, SyscallHandler, TrapCode, TypedCaller,
    UntypedValue, ValType, Value, F32, F64, N_MAX_STACK_SIZE,
};
use alloc::rc::Rc;
use futures::{channel::oneshot, future::Either, task::noop_waker};
use ouroboros::self_referencing;
use smallvec::SmallVec;
use std::{
    cell::{RefCell, RefMut},
    future::Future,
    panic,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Instant,
};
use wasmtime::{AsContext, AsContextMut};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<T>;

#[derive(Debug)]
pub(crate) struct MessageInterruptionResult {
    pub result: Result<SmallVec<[Value; 16]>, TrapCode>,
}

type InterruptChannel = Arc<Mutex<Option<oneshot::Sender<MessageInterruptionResult>>>>;

pub(crate) struct WrappedContext<T: 'static + Send> {
    pub interrupt_channel: InterruptChannel,
    pub inner: Option<T>,
    pub syscall_handler: SyscallHandler<T>,
    pub fuel: Option<u64>,
}

pub(crate) struct StoreWithBuffer<T: 'static + Send> {
    pub store: RefCell<wasmtime::Store<WrappedContext<T>>>,
    pub params_len: usize,
    pub buffer: RefCell<Vec<wasmtime::Val>>,
}

type StoreFuture<'a> = Pin<Box<dyn Future<Output = Result<(), TrapCode>> + 'a>>;

#[self_referencing]
pub struct StoreFutureHolder<T: 'static + Send> {
    owner: StoreWithBuffer<T>,
    #[borrows(owner)]
    #[covariant]
    dependent: StoreFuture<'this>,
}

pub struct WasmtimeStore<T: 'static + Send> {
    store: Option<RefCell<wasmtime::Store<WrappedContext<T>>>>,
    instance: wasmtime::Instance,
    fut: Option<StoreFutureHolder<T>>,
    interrupt_channel: InterruptChannel,
}

impl<T: 'static + Send> WasmtimeStore<T> {
    pub fn new(
        module: Rc<wasmtime::Module>,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel: Option<u64>,
    ) -> Self {
        futures::executor::block_on(async move {
            Self::new_async(module, import_linker, context, syscall_handler, fuel).await
        })
    }

    async fn new_async(
        module: Rc<wasmtime::Module>,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel: Option<u64>,
    ) -> Self {
        let interrupt_channel = Arc::new(Mutex::new(None));
        let context = WrappedContext {
            interrupt_channel: interrupt_channel.clone(),
            inner: Some(context),
            syscall_handler,
            fuel,
        };
        let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
        let linker = wasmtime_import_linker(module.engine(), import_linker);
        let instance = linker
            .instantiate_async(store.as_context_mut(), &module)
            .await
            .unwrap_or_else(|err| panic!("wasmtime: can't instantiate module: {}", err));
        Self {
            store: Some(RefCell::new(store)),
            instance,
            fut: None,
            interrupt_channel,
        }
    }

    pub fn execute(
        &mut self,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        assert!(
            self.fut.is_none(),
            "wasmtime: there is an unfinished future"
        );
        let store = self.store.take().expect("wasmtime: store is not present");
        let entrypoint = self
            .instance
            .get_func(store.borrow_mut().as_context_mut(), func_name)
            .unwrap_or_else(|| unreachable!("wasmtime: missing entrypoint: {}", func_name));
        let mut buffer = Vec::<wasmtime::Val>::default();
        for (i, value) in params.iter().enumerate() {
            let value = match value {
                Value::I32(value) => wasmtime::Val::I32(*value),
                Value::I64(value) => wasmtime::Val::I64(*value),
                Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                // this should never happen because rWasm rejects such binaries during compilation
                _ => unreachable!("wasmtime: not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        let params_len = params.len();
        buffer.extend(std::iter::repeat(wasmtime::Val::I32(0)).take(result.len()));
        let buffer = RefCell::new(buffer);
        let fut = StoreFutureHolderBuilder {
            owner: StoreWithBuffer {
                store,
                params_len,
                buffer,
            },
            dependent_builder: move |s: &StoreWithBuffer<T>| {
                // func and params are moved into this async future (owned)
                Box::pin(async move {
                    // call_async returns a future borrowing &func; we await it *inside*,
                    // so the borrow doesn't escape.
                    let mut store = s.store.borrow_mut();
                    let mut buffer = s.buffer.borrow_mut();
                    let (mapped_params, mapped_result) = buffer.split_at_mut(params_len);
                    entrypoint
                        .call_async(store.as_context_mut(), mapped_params, mapped_result)
                        .await
                        .map_err(map_anyhow_error)
                        .or_else(|trap_code| {
                            if trap_code == TrapCode::ExecutionHalted {
                                Ok(())
                            } else {
                                Err(trap_code)
                            }
                        })
                })
            },
        }
        .build();
        self.fut = Some(fut);
        self.step(result)
    }

    fn step(&mut self, result: &mut [Value]) -> Result<(), TrapCode> {
        let Some(exec) = &mut self.fut else {
            panic!("wasmtime: no in-flight exec");
        };
        let w = noop_waker();
        let mut cx = Context::from_waker(&w);
        exec.with_mut(|user| {});
        let polled = exec.with_dependent_mut(|fut| fut.as_mut().poll(&mut cx));
        match polled {
            Poll::Pending => Err(TrapCode::InterruptionCalled),
            Poll::Ready(res) => {
                let heads = self.fut.take().unwrap().into_heads();
                self.store = Some(heads.owner.store);
                for (i, x) in heads.owner.buffer.borrow()[heads.owner.params_len..]
                    .iter()
                    .enumerate()
                {
                    result[i] = match x {
                        wasmtime::Val::I32(value) => Value::I32(*value),
                        wasmtime::Val::I64(value) => Value::I64(*value),
                        wasmtime::Val::F32(value) => Value::F32(F32::from_bits(*value)),
                        wasmtime::Val::F64(value) => Value::F64(F64::from_bits(*value)),
                        _ => unreachable!("wasmtime: not supported type: {:?}", x),
                    };
                }
                res
            }
        }
    }

    fn with_store_mut<R, F: FnOnce(RefMut<wasmtime::Store<WrappedContext<T>>>) -> R>(
        &self,
        f: F,
    ) -> R {
        if let Some(fut) = self.fut.as_ref() {
            fut.with_owner(|owner| f(owner.store.borrow_mut()))
        } else {
            f(self.store.as_ref().unwrap().borrow_mut())
        }
    }

    pub fn resume(
        &mut self,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let interrupt_channel = self.interrupt_channel
            .lock()
            .expect("wasmtime: lock poisoned, can't resume")
            .take()
            .expect("wasmtime: missing interruption channel, don't call resume for non-interrupted functions");
        let interruption_result =
            interruption_result.map(|values| SmallVec::<[Value; 16]>::from(values));
        interrupt_channel
            .send(MessageInterruptionResult {
                result: interruption_result,
            })
            .expect("wasmtime: interruption channel is closed");
        self.step(result)
    }
}

impl<T: Send> Store<T> for WasmtimeStore<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.with_store_mut(|mut store| {
            let global_memory = self
                .instance
                .get_export(store.as_context_mut(), "memory")
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                })
                .into_memory()
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                });
            global_memory
                .read(store.as_context(), offset, buffer)
                .map_err(|_| TrapCode::MemoryOutOfBounds)
        })
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.with_store_mut(|mut store| {
            let global_memory = self
                .instance
                .get_export(store.as_context_mut(), "memory")
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                })
                .into_memory()
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                });
            global_memory
                .write(store.as_context_mut(), offset, &buffer)
                .map_err(|_| TrapCode::MemoryOutOfBounds)
        })
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        self.with_store_mut(|mut store| func(store.data_mut().inner.as_mut().unwrap()))
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        self.with_store_mut(|store| func(store.data().inner.as_ref().unwrap()))
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.with_store_mut(|mut store| {
            if let Ok(remaining_fuel) = store.get_fuel() {
                let new_fuel = remaining_fuel
                    .checked_sub(delta)
                    .ok_or(TrapCode::OutOfFuel)?;
                store.set_fuel(new_fuel).unwrap_or_else(|_| {
                    unreachable!("wasmtime: fuel mode is disabled in wasmtime")
                });
            } else if let Some(fuel) = store.data_mut().fuel.as_mut() {
                *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
            }
            Ok(())
        })
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        self.with_store_mut(|store| {
            if let Ok(fuel) = store.get_fuel() {
                Some(fuel)
            } else if let Some(fuel) = store.data().fuel.as_ref() {
                Some(*fuel)
            } else {
                None
            }
        })
    }
}

fn wasmtime_config() -> anyhow::Result<wasmtime::Config> {
    let mut config = wasmtime::Config::new();
    // TODO(dmitry123): "make sure config is correct"
    config.strategy(wasmtime::Strategy::Cranelift);
    config.collector(wasmtime::Collector::Null);
    config.max_wasm_stack(N_MAX_STACK_SIZE * size_of::<u32>());
    config.async_support(true);
    // TODO(dmitry123): "adjust wasmtime config if needed"
    // use caching for artifacts
    #[cfg(feature = "cache-compiled-artifacts")]
    {
        use directories::ProjectDirs;
        use std::path::{Path, PathBuf};
        use wasmtime::{Cache, CacheConfig};
        let project_dirs = ProjectDirs::from("com", "bytecodealliance", "wasmtime").unwrap();
        let cache_dir = project_dirs.cache_dir();
        std::fs::create_dir_all(cache_dir)?;
        let mut cache_config = CacheConfig::default();
        cache_config.with_directory(PathBuf::from(cache_dir));
        config.cache(Some(Cache::new(cache_config)?));
        // make sure the caching dir exists
        std::fs::create_dir_all(Path::new(cache_dir))?;
    }
    Ok(config)
}

pub fn deserialize_wasmtime_module(
    wasmtime_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime::Engine::new(&wasmtime_config()?)?;
    let module = unsafe { wasmtime::Module::deserialize(&engine, wasmtime_binary) };
    println!("{:?}", start.elapsed());
    module
}

pub fn compile_wasmtime_module(
    _compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("compiling wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime::Engine::new(&wasmtime_config()?)?;
    let module = wasmtime::Module::new(&engine, wasm_binary);
    println!("{:?}", start.elapsed());
    module
}

async fn wasmtime_syscall_handler<'a, T: Send + 'static>(
    sys_func_idx: u32,
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    params: &[wasmtime::Val],
    result: &mut [wasmtime::Val],
) -> anyhow::Result<()> {
    // convert input values from wasmtime format into rwasm format
    let mut buffer = SmallVec::<[Value; 32]>::new();
    buffer.extend(params.iter().map(|x| match x {
        wasmtime::Val::I32(value) => Value::I32(*value),
        wasmtime::Val::I64(value) => Value::I64(*value),
        wasmtime::Val::F32(value) => Value::F32(F32::from_bits(*value)),
        wasmtime::Val::F64(value) => Value::F64(F64::from_bits(*value)),
        _ => unreachable!("wasmtime: not supported type: {:?}", x),
    }));
    buffer.extend(std::iter::repeat(Value::I32(0)).take(result.len()));
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    // caller adapter is required to provide operations for accessing memory and context
    let either = {
        let syscall_handler = caller.data().syscall_handler;
        let mut caller_adapter = TypedCaller::Wasmtime(WasmtimeCaller::<'a, T> {
            caller: RefCell::new(caller),
        });
        match syscall_handler(
            &mut caller_adapter,
            sys_func_idx,
            mapped_params,
            mapped_result,
        ) {
            Err(TrapCode::InterruptionCalled) => {
                let (resp_tx, resp_rx) = oneshot::channel();
                caller_adapter
                    .as_wasmtime_mut()
                    .caller
                    .borrow()
                    .data()
                    .interrupt_channel
                    .lock()
                    .expect("interruption called, but lock was poisoned")
                    .replace(resp_tx);
                Either::Left(resp_rx)
            }
            result => Either::Right(result),
        }
    };
    match either {
        Either::Left(resp_rx) => {
            let MessageInterruptionResult {
                result: interruption_result,
            } = resp_rx.await.expect("wasmtime: interruption dropped");
            // if let Ok(fuel_remaining) = caller.get_fuel() {
            //     let new_fuel = fuel_remaining
            //         .checked_sub(fuel_consumed)
            //         .ok_or(TrapCode::OutOfFuel)?;
            //     caller.set_fuel(new_fuel).unwrap_or_else(|_| unreachable!());
            // }
            for (i, value) in interruption_result?.iter().enumerate() {
                result[i] = match value {
                    Value::I32(value) => wasmtime::Val::I32(*value),
                    Value::I64(value) => wasmtime::Val::I64(*value),
                    Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                    Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                    _ => unreachable!("wasmtime: not supported type: {:?}", value),
                };
            }
        }
        Either::Right(syscall_result) => {
            // make sure a syscall result is successful
            let should_terminate = syscall_result.map(|_| false).or_else(|trap_code| {
                // if syscall returns execution halted,
                // then don't return this trap code since it's a successful error code
                if trap_code == TrapCode::ExecutionHalted {
                    Ok(true)
                } else {
                    Err(trap_code)
                }
            })?;
            // after call map all values back to wasmtime format
            for (i, value) in mapped_result.iter().enumerate() {
                result[i] = match value {
                    Value::I32(value) => wasmtime::Val::I32(*value),
                    Value::I64(value) => wasmtime::Val::I64(*value),
                    Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                    Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                    _ => unreachable!("wasmtime: not supported type: {:?}", value),
                };
            }
            // terminate execution if required
            if should_terminate {
                return Err(TrapCode::ExecutionHalted.into());
            }
        }
    }
    Ok(())
}

fn wasmtime_import_linker<T: Send + 'static>(
    engine: &wasmtime::Engine,
    import_linker: Rc<ImportLinker>,
) -> wasmtime::Linker<WrappedContext<T>> {
    let mut linker = wasmtime::Linker::<WrappedContext<T>>::new(engine);
    for (import_name, import_entity) in import_linker.iter() {
        let params = import_entity
            .params
            .iter()
            .copied()
            .map(map_val_type)
            .collect::<Vec<_>>();
        let result = import_entity
            .result
            .iter()
            .copied()
            .map(map_val_type)
            .collect::<Vec<_>>();
        let func_type = wasmtime::FuncType::new(engine, params, result);
        linker
            .func_new_async(
                import_name.module(),
                import_name.name(),
                func_type,
                move |caller,
                      params,
                      result|
                      -> Box<dyn Future<Output = anyhow::Result<()>> + Send> {
                    Box::new(wasmtime_syscall_handler(
                        import_entity.sys_func_idx,
                        caller,
                        params,
                        result,
                    ))
                },
            )
            .unwrap_or_else(|_| panic!("function import collision: {}", import_name));
    }
    linker
}

fn map_anyhow_error(err: anyhow::Error) -> TrapCode {
    if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
        // map wasmtime trap codes into our trap codes
        use wasmtime::Trap;
        match trap {
            Trap::StackOverflow => TrapCode::StackOverflow,
            Trap::MemoryOutOfBounds => TrapCode::MemoryOutOfBounds,
            Trap::HeapMisaligned => TrapCode::MemoryOutOfBounds,
            Trap::TableOutOfBounds => TrapCode::TableOutOfBounds,
            Trap::IndirectCallToNull => TrapCode::IndirectCallToNull,
            Trap::BadSignature => TrapCode::BadSignature,
            Trap::IntegerOverflow => TrapCode::IntegerOverflow,
            Trap::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            Trap::BadConversionToInteger => TrapCode::BadConversionToInteger,
            Trap::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
            Trap::Interrupt => unreachable!("interrupt is not supported"),
            Trap::AlwaysTrapAdapter => unreachable!("component-model is not supported"),
            Trap::OutOfFuel => TrapCode::OutOfFuel,
            Trap::AtomicWaitNonSharedMemory => {
                unreachable!("atomic extension is not supported")
            }
            Trap::NullReference => TrapCode::IndirectCallToNull,
            Trap::ArrayOutOfBounds | Trap::AllocationTooLarge => {
                unreachable!("gc is not supported")
            }
            Trap::CastFailure => TrapCode::BadConversionToInteger,
            Trap::CannotEnterComponent => unreachable!("component-model is not supported"),
            Trap::NoAsyncResult => unreachable!("async mode must be disabled"),
            _ => unreachable!("unknown trap wasmtime code"),
        }
    } else if let Some(trap) = err.downcast_ref::<TrapCode>() {
        // if our trap code is initiated, then just return the trap code
        *trap
    } else {
        eprintln!("wasmtime unknown trap: {:?}", err);
        // TODO(dmitry123): "what type of error to use here in case of unknown error?"
        TrapCode::IllegalOpcode
    }
}

fn map_val_type(val_type: ValType) -> wasmtime::ValType {
    match val_type {
        ValType::I32 => wasmtime::ValType::I32,
        ValType::I64 => wasmtime::ValType::I64,
        ValType::F32 => wasmtime::ValType::F32,
        ValType::F64 => wasmtime::ValType::F64,
        _ => unreachable!("wasmtime: not supported type: {:?}", val_type),
    }
}

pub struct WasmtimeCaller<'a, T: 'static + Send> {
    pub(crate) caller: RefCell<wasmtime::Caller<'a, WrappedContext<T>>>,
}

impl<'a, T: 'static + Send> Store<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .read(self.caller.borrow().as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .write(self.caller.borrow_mut().as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        func(self.caller.borrow_mut().data_mut().inner.as_mut().unwrap())
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        func(self.caller.borrow_mut().data().inner.as_ref().unwrap())
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let mut ctx = self.caller.borrow_mut();
        if let Ok(remaining_fuel) = ctx.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            ctx.set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("wasmtime: fuel mode is disabled in wasmtime"));
        } else if let Some(fuel) = ctx.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        let ctx = self.caller.borrow();
        // TODO(dmitry123): "do we want to deal with wasmtime's fuel?"
        if let Ok(fuel) = ctx.get_fuel() {
            Some(fuel)
        } else if let Some(fuel) = ctx.data().fuel.as_ref() {
            Some(*fuel)
        } else {
            None
        }
    }
}

impl<'a, T: 'static + Send> Caller<T> for WasmtimeCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed im wasmtime mode")
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("not allowed in wasmtime mode")
    }
}

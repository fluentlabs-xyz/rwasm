mod engine;

use crate::{
    wasmtime::engine::wasmtime_engine, Caller, CompilationConfig, FuelConfig, ImportLinker, Store,
    SyscallHandler, TrapCode, TypedCaller, UntypedValue, ValType, Value, F32, F64,
};
use futures::{channel::oneshot, future::Either, task::noop_waker};
use smallvec::SmallVec;
use std::{
    future::Future,
    panic,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
    time::Instant,
};
use wasmi::{Mutability, StoreLimitsBuilder, Val};
use wasmtime::{AsContext, AsContextMut, Extern, Func, Global, GlobalType, Rooted, WasmParams};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<T>;

#[derive(Debug)]
pub struct MessageInterruptionResult {
    pub result: Result<SmallVec<[Value; 16]>, TrapCode>,
}

pub enum FutureStateChange {
    MemoryWrite { offset: usize, buffer: Vec<u8> },
    TryConsumeFuel { delta: u64 },
}

#[derive(Default)]
pub struct SharedControlState<T: 'static + Send + Sync> {
    interrupt_channel: Option<oneshot::Sender<MessageInterruptionResult>>,
    inner: T,
    state_changes: SmallVec<[FutureStateChange; 8]>,
    fuel_remaining: Option<u64>,
}

impl<T: 'static + Send + Sync> SharedControlState<T> {
    fn new(inner: T) -> Self {
        Self {
            interrupt_channel: None,
            inner,
            state_changes: Default::default(),
            fuel_remaining: None,
        }
    }
}

pub struct WrappedContext<T: 'static + Send + Sync> {
    shared_control_state: Arc<RwLock<SharedControlState<T>>>,
    syscall_handler: SyscallHandler<T>,
    fuel: Option<u64>,
    resource_limiter: StoreLimits,
}

pub type ExecFuture<T> = Pin<
    Box<
        dyn Future<
            Output = (
                wasmtime::Store<WrappedContext<T>>,
                Vec<wasmtime::Val>,
                usize,
                Result<(), TrapCode>,
            ),
        >,
    >,
>;

pub struct WasmtimeStore<T: 'static + Send + Sync> {
    pub store: Option<wasmtime::Store<WrappedContext<T>>>,
    pub instance_pre: wasmtime::InstancePre<WrappedContext<T>>,
    pub fut: Option<ExecFuture<T>>,
    pub shared_control_state: Arc<RwLock<SharedControlState<T>>>,
    pub instance: Option<wasmtime::Instance>,
}

impl<T: 'static + Send + Sync> WasmtimeStore<T> {
    pub fn new(
        module: wasmtime::Module,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_config: FuelConfig,
    ) -> Self {
        let shared_control_state = Arc::new(RwLock::new(SharedControlState::new(context)));
        let context = WrappedContext {
            shared_control_state: shared_control_state.clone(),
            syscall_handler,
            fuel: None,
            resource_limiter: StoreLimitsBuilder::new()
                .instances(usize::MAX)
                .tables(usize::MAX)
                .memories(usize::MAX)
                .build(),
        };
        let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
        store.limiter(|ctx| &mut ctx.resource_limiter);
        if let Some(fuel) = fuel_config.fuel_limit {
            if let Ok(_) = store.get_fuel() {
                store.set_fuel(fuel).expect("wasmtime: fuel is not enabled");
            } else {
                store.data_mut().fuel = Some(fuel);
            }
        }
        let mut linker = wasmtime_import_linker(module.engine(), import_linker);
        let global = Extern::Global(
            Global::new(
                store.as_context_mut(),
                GlobalType::new(wasmtime::ValType::I32, wasmtime::Mutability::Const),
                wasmtime::Val::I32(666),
            )
                .unwrap(),
        );
        linker
            .define(store.as_context_mut(), "spectest", "global_i32", global)
            .unwrap();

        let global = Extern::Global(
            Global::new(
                store.as_context_mut(),
                GlobalType::new(wasmtime::ValType::I64, wasmtime::Mutability::Const),
                wasmtime::Val::I64(666),
            )
                .unwrap(),
        );
        linker
            .define(store.as_context_mut(), "spectest", "global_i64", global)
            .unwrap();

        let instance_pre = linker
            .instantiate_pre(&module)
            .unwrap_or_else(|err| panic!("wasmtime: can't pre-instantiate module: {}", err));
        Self {
            store: Some(store),
            instance_pre,
            fut: None,
            shared_control_state,
            instance: None,
        }
    }

    pub fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        assert!(
            self.fut.is_none(),
            "wasmtime: there is an unfinished future"
        );
        let mut store = self.store.take().expect("wasmtime: store is not present");
        let mut shared_control_state = self
            .shared_control_state
            .write()
            .expect("wasmtime: lock poisoned, can't resume");
        debug_assert!(shared_control_state.state_changes.is_empty());
        shared_control_state.fuel_remaining = None;
        debug_assert!(shared_control_state.interrupt_channel.is_none());
        drop(shared_control_state);

        let (instance, entrypoint) = futures::executor::block_on(async {
            let instance = self
                .instance_pre
                .instantiate_async(store.as_context_mut())
                .await
                .unwrap_or_else(|err| panic!("wasmtime: can't instantiate module: {}", err));
            let entrypoint = instance
                .get_func(store.as_context_mut(), func_name)
                .unwrap_or_else(|| unreachable!("wasmtime: missing entrypoint: {}", func_name));
            (instance, entrypoint)
        });
        self.instance = Some(instance);

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
        let fut: ExecFuture<T> = Box::pin(async move {
            let mut store = store;
            let mut buffer = buffer;
            let params_len = params_len;
            let res = {
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
            };
            (store, buffer, params_len, res)
        });
        self.fut = Some(fut);
        self.poll_step(result)
    }

    pub fn resume(
        &mut self,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let interrupt_channel = self.shared_control_state
            .write()
            .expect("wasmtime: lock poisoned, can't resume")
            .interrupt_channel
            .take()
            .expect("wasmtime: missing interruption channel, don't call resume for non-interrupted functions");
        let interruption_result =
            interruption_result.map(|values| SmallVec::<[Value; 16]>::from(values));
        interrupt_channel
            .send(MessageInterruptionResult {
                result: interruption_result,
            })
            .expect("wasmtime: interruption channel is closed");
        self.poll_step(result)
    }

    fn poll_step(&mut self, result: &mut [Value]) -> Result<(), TrapCode> {
        if self.fut.is_none() {
            panic!("wasmtime: no in-flight exec");
        }
        let w = noop_waker();
        let mut cx = Context::from_waker(&w);
        let polled = self.fut.as_mut().unwrap().as_mut().poll(&mut cx);
        match polled {
            Poll::Pending => Err(TrapCode::InterruptionCalled),
            Poll::Ready((store, buffer, params_len, res)) => {
                self.fut = None;
                self.store = Some(store);
                for (i, x) in buffer[params_len..].iter().cloned().enumerate() {
                    result[i] = match x {
                        wasmtime::Val::I32(value) => Value::I32(value),
                        wasmtime::Val::I64(value) => Value::I64(value),
                        wasmtime::Val::F32(value) => Value::F32(F32::from_bits(value)),
                        wasmtime::Val::F64(value) => Value::F64(F64::from_bits(value)),
                        _ => unreachable!("wasmtime: not supported type: {:?}", x),
                    };
                }
                res
            }
        }
    }

    fn with_store_mut<R, F: FnOnce(&mut wasmtime::Store<WrappedContext<T>>) -> R>(
        &mut self,
        f: F,
    ) -> R {
        if let Some(fut) = self.fut.as_ref() {
            unimplemented!("wasmtime: you can't access store with locked future state")
        } else {
            f(self.store.as_mut().unwrap())
        }
    }

    fn with_store<R, F: FnOnce(&wasmtime::Store<WrappedContext<T>>) -> R>(&self, f: F) -> R {
        if let Some(fut) = self.fut.as_ref() {
            unimplemented!("wasmtime: you can't access store with locked future state")
        } else {
            f(self.store.as_ref().unwrap())
        }
    }
}

impl<T: Send + Sync> Store<T> for WasmtimeStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let instance = self.instance.clone().unwrap();
        self.with_store_mut(|store| {
            let global_memory = instance
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
        if self.fut.is_some() {
            self.shared_control_state
                .write()
                .unwrap()
                .state_changes
                .push(FutureStateChange::MemoryWrite {
                    offset,
                    buffer: buffer.to_vec(),
                });
            return Ok(());
        }
        let instance = self.instance.clone().unwrap();
        self.with_store_mut(|store| {
            let global_memory = instance
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

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        let mut context = self.shared_control_state.write().unwrap();
        func(&mut context.inner)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        let context = self.shared_control_state.read().unwrap();
        func(&context.inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        if self.fut.is_some() {
            let mut ctx = self.shared_control_state.write().unwrap();
            // Make sure we have enough fuel before writing state change
            // (state change should never fail)
            if let Some(fuel_remaining) = ctx.fuel_remaining {
                if delta > fuel_remaining {
                    return Err(TrapCode::OutOfFuel);
                }
            }
            // Write consume fuel event to execute once we have access to the store context
            ctx.state_changes
                .push(FutureStateChange::TryConsumeFuel { delta });
            return Ok(());
        }
        self.with_store_mut(|store| {
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

    fn remaining_fuel(&self) -> Option<u64> {
        if self.fut.is_some() {
            return self.shared_control_state.read().unwrap().fuel_remaining;
        }
        self.with_store(|store| {
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

pub fn deserialize_wasmtime_module(
    _compilation_config: CompilationConfig,
    wasmtime_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine();
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
    let engine = wasmtime_engine();
    let module = wasmtime::Module::new(&engine, wasm_binary);
    println!("{:?}", start.elapsed());
    module
}

async fn wasmtime_syscall_handler<'a, T: Send + Sync + 'static>(
    sys_func_idx: u32,
    mut caller: wasmtime::Caller<'a, WrappedContext<T>>,
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
        // wasmtime::Val::FuncRef(value) => Value::FuncRef(FuncRef::new(
        //     value
        //         .map(|r| r.vmgcref_pointing_to_object_count())
        //         .unwrap_or_default(),
        // )),
        // wasmtime::Val::ExternRef(value) => Value::ExternRef(ExternRef::new(
        //     value
        //         .map(|r| unsafe { r.to_raw(&mut store).unwrap_or_default() })
        //         .unwrap_or_default(),
        // )),
        _ => unreachable!("wasmtime: not supported type: {:?}", x),
    }));
    buffer.extend(std::iter::repeat(Value::I32(0)).take(result.len()));
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    // caller adapter is required to provide operations for accessing memory and context
    let either = {
        let syscall_handler = caller.data().syscall_handler;
        let mut caller_adapter = WasmtimeCaller::wrap_typed(caller);
        let result = syscall_handler(
            &mut caller_adapter,
            sys_func_idx,
            mapped_params,
            mapped_result,
        );
        let remaining_fuel = caller_adapter.remaining_fuel();
        caller = caller_adapter.into_wasmtime().unwrap();
        match result {
            Err(TrapCode::InterruptionCalled) => {
                let (resp_tx, resp_rx) = oneshot::channel();
                let shared_control_state = caller.data().shared_control_state.clone();
                {
                    let mut write_lock = shared_control_state
                        .write()
                        .expect("interruption called, but lock was poisoned");
                    write_lock.interrupt_channel.replace(resp_tx);
                    write_lock.fuel_remaining = remaining_fuel;
                }
                Either::Left((resp_rx, shared_control_state))
            }
            result => Either::Right(result),
        }
    };
    match either {
        Either::Left((resp_rx, shared_control_state)) => {
            let MessageInterruptionResult {
                result: interruption_result,
            } = resp_rx.await.expect("wasmtime: interruption dropped");
            for (i, value) in interruption_result?.iter().enumerate() {
                result[i] = match value {
                    Value::I32(value) => wasmtime::Val::I32(*value),
                    Value::I64(value) => wasmtime::Val::I64(*value),
                    Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                    Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                    _ => unreachable!("wasmtime: not supported type: {:?}", value),
                };
            }
            let mut caller_adapter = WasmtimeCaller::wrap_typed(caller);
            for state_change in shared_control_state
                .write()
                .expect("interruption called, but lock was poisoned")
                .state_changes
                .drain(..)
            {
                match state_change {
                    FutureStateChange::MemoryWrite { offset, buffer } => {
                        caller_adapter.memory_write(offset, &buffer)?;
                    }
                    FutureStateChange::TryConsumeFuel { delta } => {
                        caller_adapter.try_consume_fuel(delta)?;
                    }
                }
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

pub fn wasmtime_import_linker<T: Send + Sync + 'static>(
    engine: &wasmtime::Engine,
    import_linker: Arc<ImportLinker>,
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

pub fn map_anyhow_error(err: anyhow::Error) -> TrapCode {
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
            trap => unreachable!("unknown trap wasmtime code {:?}", trap),
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

pub fn map_val_type(val_type: ValType) -> wasmtime::ValType {
    match val_type {
        ValType::I32 => wasmtime::ValType::I32,
        ValType::I64 => wasmtime::ValType::I64,
        ValType::F32 => wasmtime::ValType::F32,
        ValType::F64 => wasmtime::ValType::F64,
        _ => unreachable!("wasmtime: not supported type: {:?}", val_type),
    }
}

pub struct WasmtimeCaller<'a, T: 'static + Send + Sync> {
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
}

impl<'a, T: 'static + Send + Sync> WasmtimeCaller<'a, T> {
    pub fn wrap_typed(caller: wasmtime::Caller<'a, WrappedContext<T>>) -> TypedCaller<'a, T> {
        TypedCaller::Wasmtime(Self { caller })
    }
    pub fn unwrap(self) -> wasmtime::Caller<'a, WrappedContext<T>> {
        self.caller
    }
}

impl<'a, T: 'static + Send + Sync> Store<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .read(self.caller.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .write(self.caller.as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        let mut context = self.caller.data_mut().shared_control_state.write().unwrap();
        func(&mut context.inner)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        let context = self.caller.data().shared_control_state.read().unwrap();
        func(&context.inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        if let Ok(remaining_fuel) = self.caller.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            self.caller
                .set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("wasmtime: fuel mode is disabled in wasmtime"));
        } else if let Some(fuel) = self.caller.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        // TODO(dmitry123): "do we want to deal with wasmtime's fuel?"
        if let Ok(fuel) = self.caller.get_fuel() {
            Some(fuel)
        } else if let Some(fuel) = self.caller.data().fuel.as_ref() {
            Some(*fuel)
        } else {
            None
        }
    }
}

impl<'a, T: 'static + Send + Sync> Caller<T> for WasmtimeCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("wasmtime: not allowed im wasmtime mode")
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("wasmtime: not allowed in wasmtime mode")
    }
}

use crate::{
    Caller, CompilationConfig, FuelConfig, FuncRef, ImportLinker, Store, SysFuncIdx,
    SyscallHandler, TrapCode, TypedCaller, UntypedValue, Value, F32, F64, N_DEFAULT_STACK_SIZE,
    N_MAX_RECURSION_DEPTH, N_MAX_STACK_SIZE,
};
use alloc::{sync::Arc, vec::Vec};
use num_traits::FromPrimitive;
use smallvec::SmallVec;
use wasmi::{
    core::{TableError, UntypedVal},
    errors::{ErrorKind, InstantiationError},
    AsContext, AsContextMut, Global, Mutability, StackLimits, Val,
};
use wasmparser::ValType;

pub type WasmiModule = wasmi::Module;

pub struct WasmiCaller<'a, T: 'static + Send + Sync> {
    caller: wasmi::Caller<'a, WasmiContextWrapper<T>>,
}

impl<'a, T: 'static + Send + Sync> Store<T> for WasmiCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .read(self.caller.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .write(self.caller.as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.caller.data_mut().inner)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.caller.data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let ctx = &mut self.caller;
        if let Ok(remaining_fuel) = ctx.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            ctx.set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("fuel mode is disabled in wasmi"));
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        let ctx = &self.caller;
        // TODO(dmitry123): "do we want to deal with wasmi's fuel?"
        if let Ok(fuel) = ctx.get_fuel() {
            Some(fuel)
        } else {
            None
        }
    }
}

impl<'a, T: 'static + Send + Sync> Caller<T> for WasmiCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed im wasmtime mode")
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("not allowed in wasmtime mode")
    }

    fn consume_fuel(&mut self, fuel: u64) -> Result<(), TrapCode> {
        self.try_consume_fuel(fuel)
    }
}

pub struct WasmiStore<T: 'static + Send + Sync> {
    store: wasmi::Store<WasmiContextWrapper<T>>,
    instance: wasmi::Instance,
    resumable_context: Option<wasmi::ResumableCallHostTrap>,
}

impl Into<TrapCode> for wasmi::core::MemoryError {
    fn into(self) -> TrapCode {
        match self {
            wasmi::core::MemoryError::OutOfFuel { .. } => TrapCode::OutOfFuel,
            _ => TrapCode::MemoryOutOfBounds,
        }
    }
}

struct WasmiContextWrapper<T: 'static + Send + Sync> {
    inner: T,
    syscall_handler: SyscallHandler<T>,
}

fn wasmi_syscall_handler<'a, T: 'static + Send + Sync>(
    sys_func_idx: SysFuncIdx,
    caller: wasmi::Caller<'a, WasmiContextWrapper<T>>,
    params: &[wasmi::Val],
    result: &mut [wasmi::Val],
) -> Result<(), wasmi::Error> {
    // convert input values from wasmi format into rwasm format
    let mut buffer = SmallVec::<[Value; 32]>::new();
    buffer.extend(params.iter().map(|x| match x {
        wasmi::Val::I32(value) => Value::I32(*value),
        wasmi::Val::I64(value) => Value::I64(*value),
        wasmi::Val::F32(value) => Value::F32(F32::from_bits(value.to_bits())),
        wasmi::Val::F64(value) => Value::F64(F64::from_bits(value.to_bits())),
        _ => unreachable!("not supported type: {:?}", x),
    }));
    buffer.extend(core::iter::repeat(Value::I32(0)).take(result.len()));
    // caller adapter is required to provide operations for accessing memory and context
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = TypedCaller::Wasmi(WasmiCaller::<'a, T> { caller });
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    let syscall_result = syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    );
    if let Some(TrapCode::InterruptionCalled) = syscall_result.err() {
        return Err(wasmi::Error::i32_exit(TrapCode::InterruptionCalled as i32));
    }
    // make sure a syscall result is successful
    let should_terminate = syscall_result
        .map(|_| false)
        .or_else(|trap_code| {
            // if syscall returns execution halted, then don't return this trap code since it's a
            // successful error code
            if trap_code == TrapCode::ExecutionHalted {
                Ok(true)
            } else {
                Err(trap_code)
            }
        })
        .map_err(|trap_code: TrapCode| wasmi::Error::i32_exit(trap_code as i32))?;
    // after call map all values back to wasmi format
    for (i, value) in mapped_result.iter().enumerate() {
        result[i] = match value {
            Value::I32(value) => wasmi::Val::I32(*value),
            Value::I64(value) => wasmi::Val::I64(*value),
            Value::F32(value) => wasmi::Val::F32(wasmi::core::F32::from_bits(value.to_bits())),
            Value::F64(value) => wasmi::Val::F64(wasmi::core::F64::from_bits(value.to_bits())),
            _ => unreachable!("not supported type: {:?}", value),
        };
    }
    // terminate execution if required
    if should_terminate {
        return Err(wasmi::Error::i32_exit(TrapCode::ExecutionHalted as i32));
    }
    Ok(())
}

fn map_val_type(val_type: ValType) -> wasmi::core::ValType {
    match val_type {
        ValType::I32 => wasmi::core::ValType::I32,
        ValType::I64 => wasmi::core::ValType::I64,
        ValType::F32 => wasmi::core::ValType::F32,
        ValType::F64 => wasmi::core::ValType::F64,
        ValType::V128 => wasmi::core::ValType::V128,
        ValType::FuncRef => wasmi::core::ValType::FuncRef,
        ValType::ExternRef => wasmi::core::ValType::ExternRef,
    }
}

fn wasmi_import_linker<T: 'static + Send + Sync>(
    engine: &wasmi::Engine,
    import_linker: Arc<ImportLinker>,
) -> wasmi::Linker<WasmiContextWrapper<T>> {
    let mut linker = wasmi::Linker::<WasmiContextWrapper<T>>::new(engine);
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
        let func_type = wasmi::FuncType::new(params, result);
        linker
            .func_new(
                import_name.module(),
                import_name.name(),
                func_type,
                move |caller, params, result| {
                    wasmi_syscall_handler(import_entity.sys_func_idx, caller, params, result)
                },
            )
            .unwrap_or_else(|_| panic!("function import collision: {}", import_name));
    }

    linker
}

impl<T: 'static + Send + Sync> WasmiStore<T> {
    pub fn new(
        module: &WasmiModule,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_config: FuelConfig,
    ) -> Self {
        let data = WasmiContextWrapper {
            inner: data,
            syscall_handler,
        };
        let mut store = wasmi::Store::new(module.engine(), data);
        if let Some(fuel_limit) = fuel_config.fuel_limit {
            store
                .set_fuel(fuel_limit)
                .unwrap_or_else(|_| unreachable!("trying to set fuel with disabled wasmi fuel"));
        }
        let mut linker = wasmi_import_linker(module.engine(), import_linker);
        linker
            .define(
                "spectest",
                "global_i32",
                Global::new(&mut store, Val::I32(666), Mutability::Const),
            )
            .unwrap();

        linker
            .define(
                "spectest",
                "global_i64",
                Global::new(&mut store, Val::I64(666), Mutability::Const),
            )
            .unwrap();

        let instance_pre = linker.instantiate(store.as_context_mut(), &module).unwrap();
        if let Some(fuel_limit) = fuel_config.fuel_limit {
            store
                .set_fuel(fuel_limit)
                .unwrap_or_else(|_| unreachable!("trying to set fuel with disabled wasmi fuel"));
        }
        let instance = instance_pre.start(store.as_context_mut()).unwrap();

        Self {
            store,
            instance,
            resumable_context: None,
        }
    }

    pub(crate) fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let func = self
            .instance
            .get_func(self.store.as_context_mut(), func_name)
            .unwrap();
        let mut buffer = SmallVec::<[wasmi::Val; 32]>::new();
        for (i, value) in params.iter().enumerate() {
            let value = match value {
                Value::I32(value) => wasmi::Val::I32(*value),
                Value::I64(value) => wasmi::Val::I64(*value),
                Value::F32(value) => wasmi::Val::F32(wasmi::core::F32::from_bits(value.to_bits())),
                Value::F64(value) => wasmi::Val::F64(wasmi::core::F64::from_bits(value.to_bits())),
                Value::FuncRef(value) => {
                    wasmi::Val::FuncRef(wasmi::FuncRef::from(UntypedVal::from(value.0)))
                }
                Value::ExternRef(value) => {
                    wasmi::Val::ExternRef(wasmi::ExternRef::from(UntypedVal::from(value.0)))
                }
                // this should never happen because rWasm rejects such binaries during compilation
                _ => unreachable!("not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        buffer.extend(core::iter::repeat(wasmi::Val::I32(0)).take(result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
        let resumable_call = func
            .call_resumable(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_wasmi_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    // execution halted means successful execution
                    Ok(wasmi::ResumableCall::Finished)
                } else {
                    Err(trap_code)
                }
            })?;
        match resumable_call {
            wasmi::ResumableCall::Finished => {}
            wasmi::ResumableCall::HostTrap(resumable_context) => {
                let Some(i32_exit_status) = resumable_context.host_error().i32_exit_status() else {
                    // TODO(dmitry123): "how to map unknown error?"
                    return Err(TrapCode::IllegalOpcode);
                };
                let trap_code = TrapCode::from_i32(i32_exit_status).unwrap();
                // if trap code is execution halted then just terminate an execution without
                // any trap code raised
                if trap_code == TrapCode::ExecutionHalted {
                    return Ok(());
                }
                // same resumable context in case of interruption, otherwise just
                // terminate an execution
                if trap_code == TrapCode::InterruptionCalled {
                    self.resumable_context = Some(resumable_context);
                }
                return Err(trap_code);
            }
            wasmi::ResumableCall::OutOfFuel(_) => return Err(TrapCode::OutOfFuel),
        }
        for (i, x) in mapped_result.iter().enumerate() {
            result[i] = match x {
                wasmi::Val::I32(value) => Value::I32(*value),
                wasmi::Val::I64(value) => Value::I64(*value),
                wasmi::Val::F32(value) => Value::F32(F32::from_bits(value.to_bits())),
                wasmi::Val::F64(value) => Value::F64(F64::from_bits(value.to_bits())),
                wasmi::Val::ExternRef(value) => Value::ExternRef(FuncRef::new(
                    wasmi::core::UntypedVal::from(*value).to_bits64() as u32,
                )),
                wasmi::Val::FuncRef(value) => Value::FuncRef(FuncRef::new(
                    wasmi::core::UntypedVal::from(*value).to_bits64() as u32,
                )),
                _ => unreachable!("not supported type: {:?}", x),
            };
        }
        Ok(())
    }

    pub(crate) fn resume(
        &mut self,
        interruption_result: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let resumable_context = self.resumable_context.take().unwrap_or_else(|| {
            unreachable!("missing wasmi resumable context, this function can't be resumed")
        });
        let mut buffer = SmallVec::<[wasmi::Val; 32]>::new();
        for (i, value) in interruption_result.iter().enumerate() {
            let value = match value {
                Value::I32(value) => wasmi::Val::I32(*value),
                Value::I64(value) => wasmi::Val::I64(*value),
                Value::F32(value) => wasmi::Val::F32(wasmi::core::F32::from_bits(value.to_bits())),
                Value::F64(value) => wasmi::Val::F64(wasmi::core::F64::from_bits(value.to_bits())),
                // this should never happen because rWasm rejects such binaries during compilation
                _ => unreachable!("not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        buffer.extend(core::iter::repeat(wasmi::Val::I32(0)).take(result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(interruption_result.len());
        let resumable_call = resumable_context
            .resume(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_wasmi_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    // execution halted means successful execution
                    Ok(wasmi::ResumableCall::Finished)
                } else {
                    Err(trap_code)
                }
            })?;
        match resumable_call {
            wasmi::ResumableCall::Finished => {}
            wasmi::ResumableCall::HostTrap(resumable_context) => {
                self.resumable_context = Some(resumable_context);
                // host trap means interruption, but we can't forward any errors since we don't support,
                // and we don't use it anywhere inside rWasm
                return Err(TrapCode::InterruptionCalled);
            }
            wasmi::ResumableCall::OutOfFuel(_) => return Err(TrapCode::OutOfFuel),
        }
        for (i, x) in mapped_result.iter().enumerate() {
            result[i] = match x {
                wasmi::Val::I32(value) => Value::I32(*value),
                wasmi::Val::I64(value) => Value::I64(*value),
                wasmi::Val::F32(value) => Value::F32(F32::from_bits(value.to_bits())),
                wasmi::Val::F64(value) => Value::F64(F64::from_bits(value.to_bits())),
                _ => unreachable!("not supported type: {:?}", x),
            };
        }
        Ok(())
    }
}

fn map_wasmi_error(err: wasmi::Error) -> TrapCode {
    if let Some(trap_code) = err.as_trap_code() {
        match trap_code {
            wasmi::core::TrapCode::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
            wasmi::core::TrapCode::MemoryOutOfBounds => TrapCode::MemoryOutOfBounds,
            wasmi::core::TrapCode::TableOutOfBounds => TrapCode::TableOutOfBounds,
            wasmi::core::TrapCode::IndirectCallToNull => TrapCode::IndirectCallToNull,
            wasmi::core::TrapCode::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            wasmi::core::TrapCode::IntegerOverflow => TrapCode::IntegerOverflow,
            wasmi::core::TrapCode::BadConversionToInteger => TrapCode::BadConversionToInteger,
            wasmi::core::TrapCode::StackOverflow => TrapCode::StackOverflow,
            wasmi::core::TrapCode::BadSignature => TrapCode::BadSignature,
            wasmi::core::TrapCode::OutOfFuel => TrapCode::OutOfFuel,
            wasmi::core::TrapCode::GrowthOperationLimited => {
                // this error should never happen because we limit tables/memories during the
                // compilation process from wasm into rwasm
                unreachable!("growth operation limited error is not possible")
            }
        }
    } else if let Some(exit_code) = err.i32_exit_status() {
        // we have nothing to do with an exit code; the exit code should always be zero
        TrapCode::from_i32(exit_code)
            .unwrap_or_else(|| unreachable!("an impossible wasmi error happened: {:?}", err))
    } else if let ErrorKind::Table(table_err) = err.kind() {
        match table_err {
            TableError::CopyOutOfBounds => TrapCode::MemoryOutOfBounds,
            err => unreachable!("an impossible wasmi error happened: {:?}", err),
        }
    } else if let ErrorKind::Instantiation(instantiation_err) = err.kind() {
        match instantiation_err {
            InstantiationError::ElementSegmentDoesNotFit { .. } => TrapCode::TableOutOfBounds,
            err => unreachable!("an impossible wasmi error happened: {:?}", err),
        }
    } else {
        // this should never happen since such cases must be handled by syscall handler or
        // other wrappers
        unreachable!("an impossible wasmi error happened: {:?}", err)
    }
}

impl<T: 'static + Send + Sync> Store<T> for WasmiStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let memory = self
            .instance
            .get_export(self.store.as_context(), "memory")
            .expect("missing memory export")
            .into_memory()
            .expect("missing memory export");
        memory
            .read(self.store.as_context(), offset, buffer)
            .map_err(Into::into)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let memory = self
            .instance
            .get_export(self.store.as_context_mut(), "memory")
            .expect("missing memory export")
            .into_memory()
            .expect("missing memory export");
        memory
            .write(self.store.as_context_mut(), offset, buffer)
            .map_err(Into::into)
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.store.data_mut().inner)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.store.data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        if let Ok(fuel) = self.store.get_fuel() {
            let new_fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
            self.store.set_fuel(new_fuel).unwrap();
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        self.store.get_fuel().ok()
    }
}

pub fn compile_wasmi_module(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> Result<WasmiModule, wasmi::Error> {
    let mut config = wasmi::Config::default();
    config.consume_fuel(compilation_config.consume_fuel);
    // TODO(dmitry123): "in case of FPU opcodes we need to trap"
    // config.floats(cfg!(feature = "fpu"));
    config.set_stack_limits(
        StackLimits::new(
            N_DEFAULT_STACK_SIZE,
            N_MAX_STACK_SIZE,
            N_MAX_RECURSION_DEPTH,
        )
        .unwrap(),
    );
    // TODO(dmitry123): "adjust wasmi config if needed"
    let engine = wasmi::Engine::new(&config);
    WasmiModule::new(&engine, wasm_binary)
}

use crate::{
    Caller, CompilationConfig, ImportLinker, Store, SysFuncIdx, SyscallHandler, TrapCode,
    TypedCaller, UntypedValue, Value, F32, F64, N_DEFAULT_STACK_SIZE, N_MAX_RECURSION_DEPTH,
    N_MAX_STACK_SIZE,
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use num_traits::FromPrimitive;
use rwasm_legacy::engine::RwasmConfig;
use rwasm_legacy::errors::FuelError;
use rwasm_legacy::{AsContext, AsContextMut, StackLimits};
use smallvec::SmallVec;
use wasmparser::ValType;

pub type LegacyEngine = rwasm_legacy::Engine;
pub type LegacyModule = rwasm_legacy::Module;

pub struct LegacyCaller<'a, T: Send + Sync + 'static> {
    caller: RefCell<rwasm_legacy::Caller<'a, LegacyContextWrapper<T>>>,
}

impl<'a, T: Send + Sync> Store<T> for LegacyCaller<'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .read(self.caller.borrow().as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .write(self.caller.borrow_mut().as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        func(&mut self.caller.borrow_mut().data_mut().inner)
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        func(&self.caller.borrow_mut().data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let mut ctx = self.caller.borrow_mut();
        if let Some(remaining_fuel) = ctx.fuel_remaining() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            return match ctx.consume_fuel(delta) {
                Err(FuelError::OutOfFuel) => Err(TrapCode::OutOfFuel),
                _ => unreachable!("fuel mode is disabled in legacy"),
            };
        }
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        let ctx = self.caller.borrow();
        // TODO(dmitry123): "do we want to deal with rwasm_legacy's fuel?"
        if let Some(fuel) = ctx.fuel_remaining() {
            Some(fuel)
        } else {
            None
        }
    }
}

impl<'a, T: Send + Sync> Caller<T> for LegacyCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed im wasmtime mode")
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("not allowed in wasmtime mode")
    }
}

pub struct LegacyStore<T: Send + Sync + 'static> {
    store: rwasm_legacy::Store<LegacyContextWrapper<T>>,
    instance: rwasm_legacy::Instance,
    resumable_context: Option<rwasm_legacy::ResumableInvocation>,
}

impl Into<TrapCode> for rwasm_legacy::memory::MemoryError {
    fn into(self) -> TrapCode {
        TrapCode::MemoryOutOfBounds
    }
}

struct LegacyContextWrapper<T: Send + Sync + 'static> {
    inner: T,
    syscall_handler: SyscallHandler<T>,
}

fn rwasm_legacy_syscall_handler<'a, T: Send + Sync>(
    sys_func_idx: SysFuncIdx,
    caller: rwasm_legacy::Caller<'a, LegacyContextWrapper<T>>,
    params: &[rwasm_legacy::Value],
    result: &mut [rwasm_legacy::Value],
) -> Result<(), rwasm_legacy::core::Trap> {
    // convert input values from rwasm_legacy format into rwasm format
    let mut buffer = SmallVec::<[Value; 32]>::new();
    buffer.extend(params.iter().map(|x| match x {
        rwasm_legacy::Value::I32(value) => Value::I32(*value),
        rwasm_legacy::Value::I64(value) => Value::I64(*value),
        rwasm_legacy::Value::F32(value) => Value::F32(F32::from_bits(value.to_bits())),
        rwasm_legacy::Value::F64(value) => Value::F64(F64::from_bits(value.to_bits())),
        _ => unreachable!("not supported type: {:?}", x),
    }));
    buffer.extend(core::iter::repeat(Value::I32(0)).take(result.len()));
    // caller adapter is required to provide operations for accessing memory and context
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = TypedCaller::Legacy(LegacyCaller::<'a, T> {
        caller: RefCell::new(caller),
    });
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    let syscall_result = syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    );
    if let Some(TrapCode::InterruptionCalled) = syscall_result.err() {
        todo!()
    } else {
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
            .map_err(|trap_code: TrapCode| rwasm_legacy::core::Trap::i32_exit(trap_code as i32))?;
        // after call map all values back to rwasm_legacy format
        for (i, value) in mapped_result.iter().enumerate() {
            result[i] = match value {
                Value::I32(value) => rwasm_legacy::Value::I32(*value),
                Value::I64(value) => rwasm_legacy::Value::I64(*value),
                Value::F32(value) => {
                    rwasm_legacy::Value::F32(rwasm_legacy::core::F32::from_bits(value.to_bits()))
                }
                Value::F64(value) => {
                    rwasm_legacy::Value::F64(rwasm_legacy::core::F64::from_bits(value.to_bits()))
                }
                _ => unreachable!("not supported type: {:?}", value),
            };
        }
        // terminate execution if required
        if should_terminate {
            return Err(
                rwasm_legacy::core::Trap::i32_exit(TrapCode::ExecutionHalted as i32).into(),
            );
        }
    }
    Ok(())
}

fn map_val_type(val_type: ValType) -> rwasm_legacy::core::ValueType {
    match val_type {
        ValType::I32 => rwasm_legacy::core::ValueType::I32,
        ValType::I64 => rwasm_legacy::core::ValueType::I64,
        ValType::F32 => rwasm_legacy::core::ValueType::F32,
        ValType::F64 => rwasm_legacy::core::ValueType::F64,
        ValType::V128 => unreachable!("legacy: not supported v128 type"),
        ValType::FuncRef => rwasm_legacy::core::ValueType::FuncRef,
        ValType::ExternRef => rwasm_legacy::core::ValueType::ExternRef,
    }
}

fn rwasm_legacy_import_linker<T: Send + Sync>(
    engine: &rwasm_legacy::Engine,
    import_linker: Rc<ImportLinker>,
) -> rwasm_legacy::Linker<LegacyContextWrapper<T>> {
    let mut linker = rwasm_legacy::Linker::<LegacyContextWrapper<T>>::new(engine);
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
        let func_type = rwasm_legacy::FuncType::new(params, result);
        linker
            .func_new(
                import_name.module(),
                import_name.name(),
                func_type,
                move |caller, params, result| {
                    rwasm_legacy_syscall_handler(import_entity.sys_func_idx, caller, params, result)
                },
            )
            .unwrap_or_else(|_| panic!("function import collision: {}", import_name));
    }
    linker
}

impl<T: Send + Sync> LegacyStore<T> {
    pub fn new(
        module: &LegacyModule,
        import_linker: Rc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
    ) -> Self {
        let mut store = rwasm_legacy::Store::new(
            module.engine(),
            LegacyContextWrapper {
                inner: data,
                syscall_handler,
            },
        );
        if let Some(fuel_limit) = fuel_limit {
            store
                .add_fuel(fuel_limit)
                .unwrap_or_else(|_| unreachable!("trying to set fuel with disabled legacy fuel"));
        }
        let linker = rwasm_legacy_import_linker(module.engine(), import_linker);

        let instance = linker
            .instantiate(store.as_context_mut(), &module)
            .unwrap()
            .start(store.as_context_mut())
            .unwrap();
        Self {
            store,
            instance,
            resumable_context: None,
        }
    }

    pub(crate) fn execute(
        &mut self,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let func = self
            .instance
            .get_func(self.store.as_context_mut(), func_name)
            .unwrap();
        let mut buffer = SmallVec::<[rwasm_legacy::Value; 32]>::new();
        for (i, value) in params.iter().enumerate() {
            let value = match value {
                Value::I32(value) => rwasm_legacy::Value::I32(*value),
                Value::I64(value) => rwasm_legacy::Value::I64(*value),
                Value::F32(value) => {
                    rwasm_legacy::Value::F32(rwasm_legacy::core::F32::from_bits(value.to_bits()))
                }
                Value::F64(value) => {
                    rwasm_legacy::Value::F64(rwasm_legacy::core::F64::from_bits(value.to_bits()))
                }
                // this should never happen because rWasm rejects such binaries during compilation
                _ => unreachable!("not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        buffer.extend(core::iter::repeat(rwasm_legacy::Value::I32(0)).take(result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
        let resumable_call = func
            .call_resumable(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_legacy_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    // execution halted means successful execution
                    Ok(rwasm_legacy::ResumableCall::Finished)
                } else {
                    Err(trap_code)
                }
            })?;
        match resumable_call {
            rwasm_legacy::ResumableCall::Finished => {}
            rwasm_legacy::ResumableCall::Resumable(resumable_context) => {
                self.resumable_context = Some(resumable_context);
                // host trap means interruption, but we can't forward any errors since we don't support,
                // and we don't use it anywhere inside rWasm
                return Err(TrapCode::InterruptionCalled);
            }
        }
        for (i, x) in mapped_result.iter().enumerate() {
            result[i] = match x {
                rwasm_legacy::Value::I32(value) => Value::I32(*value),
                rwasm_legacy::Value::I64(value) => Value::I64(*value),
                rwasm_legacy::Value::F32(value) => Value::F32(F32::from_bits(value.to_bits())),
                rwasm_legacy::Value::F64(value) => Value::F64(F64::from_bits(value.to_bits())),
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
            unreachable!("missing rwasm_legacy resumable context, this function can't be resumed")
        });
        let mut buffer = SmallVec::<[rwasm_legacy::Value; 32]>::new();
        for (i, value) in interruption_result.iter().enumerate() {
            let value = match value {
                Value::I32(value) => rwasm_legacy::Value::I32(*value),
                Value::I64(value) => rwasm_legacy::Value::I64(*value),
                Value::F32(value) => {
                    rwasm_legacy::Value::F32(rwasm_legacy::core::F32::from_bits(value.to_bits()))
                }
                Value::F64(value) => {
                    rwasm_legacy::Value::F64(rwasm_legacy::core::F64::from_bits(value.to_bits()))
                }
                // this should never happen because rWasm rejects such binaries during compilation
                _ => unreachable!("not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        buffer.extend(core::iter::repeat(rwasm_legacy::Value::I32(0)).take(result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(interruption_result.len());
        let resumable_call = resumable_context
            .resume(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_legacy_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    // execution halted means successful execution
                    Ok(rwasm_legacy::ResumableCall::Finished)
                } else {
                    Err(trap_code)
                }
            })?;
        match resumable_call {
            rwasm_legacy::ResumableCall::Finished => {}
            rwasm_legacy::ResumableCall::Resumable(resumable_context) => {
                self.resumable_context = Some(resumable_context);
                // host trap means interruption, but we can't forward any errors since we don't support,
                // and we don't use it anywhere inside rWasm
                return Err(TrapCode::InterruptionCalled);
            }
        }
        for (i, x) in mapped_result.iter().enumerate() {
            result[i] = match x {
                rwasm_legacy::Value::I32(value) => Value::I32(*value),
                rwasm_legacy::Value::I64(value) => Value::I64(*value),
                rwasm_legacy::Value::F32(value) => Value::F32(F32::from_bits(value.to_bits())),
                rwasm_legacy::Value::F64(value) => Value::F64(F64::from_bits(value.to_bits())),
                _ => unreachable!("not supported type: {:?}", x),
            };
        }
        Ok(())
    }
}

fn map_legacy_error(err: rwasm_legacy::Error) -> TrapCode {
    let err = match err {
        rwasm_legacy::Error::Trap(err) => err,
        _ => unreachable!("unexpected legacy error: {:?}", err),
    };
    if let Some(trap_code) = err.trap_code() {
        match trap_code {
            rwasm_legacy::core::TrapCode::UnreachableCodeReached => {
                TrapCode::UnreachableCodeReached
            }
            rwasm_legacy::core::TrapCode::MemoryOutOfBounds => TrapCode::MemoryOutOfBounds,
            rwasm_legacy::core::TrapCode::TableOutOfBounds => TrapCode::TableOutOfBounds,
            rwasm_legacy::core::TrapCode::IndirectCallToNull => TrapCode::IndirectCallToNull,
            rwasm_legacy::core::TrapCode::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            rwasm_legacy::core::TrapCode::IntegerOverflow => TrapCode::IntegerOverflow,
            rwasm_legacy::core::TrapCode::BadConversionToInteger => {
                TrapCode::BadConversionToInteger
            }
            rwasm_legacy::core::TrapCode::StackOverflow => TrapCode::StackOverflow,
            rwasm_legacy::core::TrapCode::BadSignature => TrapCode::BadSignature,
            rwasm_legacy::core::TrapCode::OutOfFuel => TrapCode::OutOfFuel,
            rwasm_legacy::core::TrapCode::GrowthOperationLimited => {
                // this error should never happen because we limit tables/memories during the
                // compilation process from wasm into rwasm
                unreachable!("growth-operation-limited error is not possible")
            }
            rwasm_legacy::core::TrapCode::UnresolvedFunction => {
                unreachable!("unresolve-function error is not possible")
            }
        }
    } else if let Some(exit_code) = err.i32_exit_status() {
        // we have nothing to do with an exit code; the exit code should always be zero
        TrapCode::from_i32(exit_code)
            .unwrap_or_else(|| unreachable!("an impossible legacy error happened: {:?}", err))
    } else {
        // this should never happen since such cases must be handled by syscall handler or
        // other wrappers
        unreachable!("an impossible legacy error happened: {:?}", err)
    }
}

impl<T: Send + Sync> Store<T> for LegacyStore<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
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

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        func(&mut self.store.data_mut().inner)
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        func(&self.store.data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        match self.store.consume_fuel(delta) {
            Err(FuelError::OutOfFuel) => Err(TrapCode::OutOfFuel),
            _ => unreachable!(),
        }
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        self.store.fuel_remaining()
    }
}

pub fn compile_legacy_module(
    compilation_config: CompilationConfig,
    wasm_binary: &[u8],
) -> Result<rwasm_legacy::Module, rwasm_legacy::Error> {
    let mut config = rwasm_legacy::Config::default();
    let mut rwasm_config = RwasmConfig::default();
    rwasm_config.allow_malformed_entrypoint_func_type = true;
    config.rwasm_config(rwasm_config);
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
    // TODO(dmitry123): "adjust legacy config if needed"
    let engine = rwasm_legacy::Engine::new(&config);
    let rwasm_module = rwasm_legacy::rwasm::RwasmModule::compile_with_config(wasm_binary, &config)?;
    Ok(rwasm_module.to_module(&engine))
}

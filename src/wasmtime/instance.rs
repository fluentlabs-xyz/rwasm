use crate::{
    wasmtime::{types::map_anyhow_error, wasmtime_import_linker, WrappedContext},
    ImportLinker, SyscallHandler, TrapCode, Value, F32, F64, N_BYTES_PER_MEMORY_PAGE,
    N_DEFAULT_MAX_MEMORY_PAGES,
};
use std::sync::Arc;
use wasmtime::{AsContext, AsContextMut, StoreContext, StoreContextMut};

pub struct WasmtimeExecutor<T: 'static> {
    pub linker: wasmtime::Linker<WrappedContext<T>>,
    pub store: wasmtime::Store<WrappedContext<T>>,
    pub instance_pre: wasmtime::InstancePre<WrappedContext<T>>,
    pub instance: wasmtime::Instance,
}

impl<T: 'static> AsContext for WasmtimeExecutor<T> {
    type Data = WrappedContext<T>;

    fn as_context(&self) -> StoreContext<'_, Self::Data> {
        self.store.as_context()
    }
}
impl<T: 'static> AsContextMut for WasmtimeExecutor<T> {
    fn as_context_mut(&mut self) -> StoreContextMut<'_, Self::Data> {
        self.store.as_context_mut()
    }
}

impl<T: 'static> WasmtimeExecutor<T> {
    pub fn new(
        module: wasmtime::Module,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> Self {
        let resource_limiter = wasmtime::StoreLimitsBuilder::new()
            .memory_size(
                (max_allowed_memory_pages.unwrap_or(N_DEFAULT_MAX_MEMORY_PAGES)
                    * N_BYTES_PER_MEMORY_PAGE) as usize,
            )
            .build();

        let context = WrappedContext {
            syscall_handler,
            fuel: None,
            resource_limiter,
            data,
        };
        let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
        store.limiter(|ctx| &mut ctx.resource_limiter);
        if let Some(fuel) = fuel_limit {
            if let Ok(_) = store.get_fuel() {
                store.set_fuel(fuel).expect("wasmtime: fuel is not enabled");
            } else {
                store.data_mut().fuel = Some(fuel);
            }
        }
        #[allow(unused_mut)]
        let mut linker = wasmtime_import_linker(module.engine(), &import_linker);
        #[cfg(feature = "e2e")]
        {
            Self::link_spectest_globals(&mut linker, &mut store);
        }
        let instance_pre = linker
            .instantiate_pre(&module)
            .unwrap_or_else(|err| panic!("wasmtime: can't pre-instantiate module: {}", err));
        let instance = instance_pre
            .instantiate(store.as_context_mut())
            .unwrap_or_else(|err| panic!("wasmtime: can't instantiate module: {}", err));
        Self {
            linker,
            store,
            instance_pre,
            instance,
        }
    }

    #[cfg(feature = "e2e")]
    fn link_spectest_globals(
        linker: &mut wasmtime::Linker<WrappedContext<T>>,
        store: &mut wasmtime::Store<WrappedContext<T>>,
    ) {
        use wasmtime::{Extern, Global, GlobalType, Mutability, ValType};
        let global = Extern::Global(
            Global::new(
                store.as_context_mut(),
                GlobalType::new(ValType::I32, Mutability::Const),
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
                GlobalType::new(ValType::I64, Mutability::Const),
                wasmtime::Val::I64(666),
            )
            .unwrap(),
        );
        linker
            .define(store.as_context_mut(), "spectest", "global_i64", global)
            .unwrap();
    }

    pub fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        use wasmtime::Val;
        let entrypoint = self
            .instance
            .get_func(self.store.as_context_mut(), func_name)
            .unwrap_or_else(|| unreachable!("wasmtime: missing entrypoint: {}", func_name));
        let mut buffer = Vec::<Val>::default();
        for (i, value) in params.iter().enumerate() {
            let value = match value {
                Value::I32(value) => Val::I32(*value),
                Value::I64(value) => Val::I64(*value),
                Value::F32(value) => Val::F32(value.to_bits()),
                Value::F64(value) => Val::F64(value.to_bits()),
                #[cfg(feature = "e2e")]
                Value::FuncRef(value) => Val::FuncRef(None),
                #[cfg(feature = "e2e")]
                Value::ExternRef(value) => {
                    let func_idx = value.0;
                    if func_idx == 0 {
                        Val::ExternRef(None)
                    } else {
                        Val::ExternRef(wasmtime::ExternRef::new(&mut self.store, func_idx).ok())
                    }
                }
                // this should never happen because rWasm rejects such binaries during compilation
                #[allow(unreachable_patterns)]
                _ => unreachable!("wasmtime: not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        buffer.extend(std::iter::repeat(Val::I32(0)).take(result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
        let exec_result = entrypoint
            .call(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_anyhow_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    Ok(())
                } else {
                    Err(trap_code)
                }
            });
        if exec_result.is_err() {
            return exec_result;
        }
        for (i, x) in mapped_result.iter().cloned().enumerate() {
            result[i] = match x {
                Val::I32(value) => Value::I32(value),
                Val::I64(value) => Value::I64(value),
                Val::F32(value) => Value::F32(F32::from_bits(value)),
                Val::F64(value) => Value::F64(F64::from_bits(value)),
                #[cfg(feature = "e2e")]
                Val::FuncRef(value) => Value::FuncRef(crate::FuncRef::new(0)),
                #[cfg(feature = "e2e")]
                Val::ExternRef(value) => {
                    let value: Option<&u32> = value
                        .and_then(|ext_ref| ext_ref.data(&mut self.store).ok().flatten())
                        .and_then(|v| v.downcast_ref());
                    Value::ExternRef(crate::ExternRef::new(value.map(|v| *v).unwrap_or_default()))
                }
                _ => unreachable!("wasmtime: not supported type: {:?}", x),
            };
        }
        exec_result
    }

    pub fn resume(
        &mut self,
        interruption_result: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        unimplemented!("wasmtime: resume is not implemented yet");
    }
}

impl<T> crate::StoreTr<T> for WasmtimeExecutor<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let instance = self.instance.clone();
        let global_memory = instance
            .get_export(self.store.as_context_mut(), "memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .read(self.store.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let instance = self.instance.clone();
        let global_memory = instance
            .get_export(self.store.as_context_mut(), "memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .write(self.store.as_context_mut(), offset, &buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.store.data_mut().data
    }

    fn data(&self) -> &T {
        &self.store.data().data
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        if let Ok(remaining_fuel) = self.store.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            self.store
                .set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("wasmtime: fuel mode is disabled in wasmtime"));
        } else if let Some(fuel) = self.store.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        if let Ok(fuel) = self.store.get_fuel() {
            Some(fuel)
        } else if let Some(fuel) = self.store.data().fuel.as_ref() {
            Some(*fuel)
        } else {
            None
        }
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        let has_fuel_enabled = self.store.get_fuel().is_ok();
        if has_fuel_enabled {
            self.store.set_fuel(new_fuel_limit).unwrap();
        } else {
            self.store.data_mut().fuel = Some(new_fuel_limit)
        }
    }
}

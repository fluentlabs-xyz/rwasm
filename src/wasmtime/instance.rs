use crate::{
    checked_memory_range_end,
    wasmtime::{types::map_wasmtime_error, wasmtime_import_linker, WrappedContext},
    ImportLinker, SyscallHandler, TrapCode, Value, F32, F64, N_BYTES_PER_MEMORY_PAGE,
    N_DEFAULT_MAX_MEMORY_PAGES, N_MAX_ALLOWED_MEMORY_PAGES,
};
use std::sync::Arc;
use wasmtime::{AsContext, AsContextMut, Extern, StoreContext, StoreContextMut};

pub struct WasmtimeExecutor<T: 'static> {
    pub linker: wasmtime::Linker<WrappedContext<T>>,
    pub store: wasmtime::Store<WrappedContext<T>>,
    pub instance_pre: wasmtime::InstancePre<WrappedContext<T>>,
    pub instance: wasmtime::Instance,
    /// The instance whose exports are currently cached in `functions` and in the store's
    /// memory handle. Compared against `instance` before every use, so swapping `instance`
    /// directly still resolves the right exports.
    cached_instance: wasmtime::Instance,
    /// Exported functions of `cached_instance`, resolved once so calls don't look them up by
    /// name. Entry points are few, so a linear scan beats hashing the name.
    functions: Vec<(Box<str>, wasmtime::Func)>,
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
    fn exported_memory(&mut self) -> Result<wasmtime::Memory, TrapCode> {
        self.ensure_exports_current();
        self.store.data().memory.ok_or(TrapCode::MemoryOutOfBounds)
    }

    /// Re-resolves the cached exports when `instance` was replaced since the last use.
    fn ensure_exports_current(&mut self) {
        if self.cached_instance != self.instance {
            self.refresh_exports();
        }
    }

    /// Resolves the exported functions and the exported memory of `instance` once.
    fn refresh_exports(&mut self) {
        self.functions.clear();
        let mut memory = None;
        for export in self.instance.exports(&mut self.store) {
            let name = export.name();
            match export.into_extern() {
                Extern::Func(func) => self.functions.push((name.into(), func)),
                Extern::Memory(exported_memory) if name == "memory" => {
                    memory = Some(exported_memory)
                }
                _ => {}
            }
        }
        self.store.data_mut().memory = memory;
        self.cached_instance = self.instance;
    }

    /// Creates an executor by instantiating an already-compiled Wasmtime module.
    ///
    /// # Panics
    ///
    /// Panics if linking or instantiation fails. This is deliberate fail-fast on API misuse: a
    /// module produced by this crate's own compile path has already been validated against the
    /// import linker, and start sections (the one way a valid module can trap during
    /// instantiation) are rejected by default at compile time. So a failure here means the caller
    /// paired a module with the wrong import linker or bypassed compilation/validation, and we'd
    /// rather crash loudly than continue with a half-linked instance. Use [`Self::try_new`] to
    /// get the error instead.
    pub fn new(
        module: wasmtime::Module,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> Self {
        Self::try_new(
            module,
            import_linker,
            data,
            syscall_handler,
            fuel_limit,
            max_allowed_memory_pages,
        )
        .unwrap_or_else(|err| panic!("wasmtime: can't instantiate module: {}", err))
    }

    /// Creates an executor by instantiating an already-compiled Wasmtime module, returning the
    /// linking or instantiation error instead of panicking.
    pub fn try_new(
        module: wasmtime::Module,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> wasmtime::Result<Self> {
        let memory_pages = max_allowed_memory_pages
            .unwrap_or(N_DEFAULT_MAX_MEMORY_PAGES)
            .min(N_MAX_ALLOWED_MEMORY_PAGES);
        let memory_size_limit = (memory_pages as usize)
            .checked_mul(N_BYTES_PER_MEMORY_PAGE as usize)
            .expect("wasmtime: memory limit is bounded by N_MAX_ALLOWED_MEMORY_PAGES");
        let resource_limiter = wasmtime::StoreLimitsBuilder::new()
            .memory_size(memory_size_limit)
            .build();

        let context = WrappedContext {
            syscall_handler,
            fuel: None,
            fuel_enabled: false,
            memory: None,
            resource_limiter,
            data,
        };
        let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
        store.limiter(|ctx| &mut ctx.resource_limiter);
        let fuel_enabled = store.get_fuel().is_ok();
        store.data_mut().fuel_enabled = fuel_enabled;
        if let Some(fuel) = fuel_limit {
            if fuel_enabled {
                store.set_fuel(fuel)?;
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
        let instance_pre = linker.instantiate_pre(&module)?;
        let instance = instance_pre.instantiate(store.as_context_mut())?;
        let mut executor = Self {
            linker,
            store,
            instance_pre,
            instance,
            cached_instance: instance,
            functions: Vec::new(),
        };
        executor.refresh_exports();
        Ok(executor)
    }

    /// Instantiates `module` in this executor's store with its linker, replacing the current
    /// instance and its cached exports.
    pub fn instantiate(&mut self, module: &wasmtime::Module) -> wasmtime::Result<()> {
        let instance_pre = self.linker.instantiate_pre(module)?;
        let instance = instance_pre.instantiate(self.store.as_context_mut())?;
        self.instance_pre = instance_pre;
        self.instance = instance;
        self.refresh_exports();
        Ok(())
    }

    /// Looks up an exported function in the cached export table.
    fn exported_function(&mut self, func_name: &str) -> Option<wasmtime::Func> {
        self.ensure_exports_current();
        self.functions
            .iter()
            .find(|(name, _)| &**name == func_name)
            .map(|(_, func)| *func)
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
            .exported_function(func_name)
            .ok_or(TrapCode::UnknownExternalFunction)?;
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
        buffer.extend(std::iter::repeat_n(Val::I32(0), result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
        entrypoint
            .call(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_wasmtime_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    Ok(())
                } else {
                    Err(trap_code)
                }
            })?;
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
                    Value::ExternRef(crate::ExternRef::new(value.copied().unwrap_or_default()))
                }
                _ => unreachable!("wasmtime: not supported type: {:?}", x),
            };
        }
        Ok(())
    }

    pub fn resume(
        &mut self,
        interruption_result: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        unimplemented!("wasmtime: resume is not implemented yet");
    }

    pub fn snapshot_memory(&mut self) -> Result<Vec<u8>, TrapCode> {
        let global_memory = self.exported_memory()?;
        let memory_size = global_memory
            .size(self.store.as_context_mut())
            .checked_mul(N_BYTES_PER_MEMORY_PAGE as u64)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        let mut snapshot = vec![0; memory_size as usize];
        global_memory
            .read(self.store.as_context_mut(), 0, &mut snapshot)
            .map_err(|_| TrapCode::MemoryOutOfBounds)?;
        Ok(snapshot)
    }
}

impl<T> crate::StoreTr<T> for WasmtimeExecutor<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self.exported_memory()?;
        global_memory
            .read(self.store.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        let end = checked_memory_range_end(offset, length)?;
        let global_memory = self.exported_memory()?;
        let memory_size = (global_memory.size(self.store.as_context_mut()) as usize)
            .checked_mul(N_BYTES_PER_MEMORY_PAGE as usize)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        if end > memory_size {
            return Err(TrapCode::MemoryOutOfBounds);
        }
        let mut data = vec![0u8; length];
        self.memory_read(offset, &mut data)?;
        Ok(data)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self.exported_memory()?;
        global_memory
            .write(self.store.as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.store.data_mut().data
    }

    fn data(&self) -> &T {
        &self.store.data().data
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        if self.store.data().fuel_enabled {
            let remaining_fuel = self.store.get_fuel().unwrap_or_else(|_| {
                unreachable!("wasmtime: fuel metering was enabled at store creation")
            });
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            self.store.set_fuel(new_fuel).unwrap_or_else(|_| {
                unreachable!("wasmtime: fuel metering was enabled at store creation")
            });
        } else if let Some(fuel) = self.store.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        if self.store.data().fuel_enabled {
            self.store.get_fuel().ok()
        } else {
            self.store.data().fuel
        }
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        if self.store.data().fuel_enabled {
            self.store.set_fuel(new_fuel_limit).unwrap_or_else(|_| {
                unreachable!("wasmtime: fuel metering was enabled at store creation")
            });
        } else {
            self.store.data_mut().fuel = Some(new_fuel_limit)
        }
    }
}

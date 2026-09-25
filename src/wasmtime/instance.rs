use crate::{
    checked_memory_range_end,
    wasmtime::{
        context::{missing_memory_access, RecordingStoreLimits},
        types::map_wasmtime_error,
        wasmtime_import_linker, WasmtimeModule, WrappedContext,
    },
    ImportLinker, SyscallHandler, TrapCode, Value, F32, F64, N_BYTES_PER_MEMORY_PAGE,
    N_DEFAULT_MAX_MEMORY_PAGES, N_MAX_ALLOWED_MEMORY_PAGES, N_MAX_TABLE_SIZE,
};
use rwasm_fuel_policy::SyscallFuelParams;
use smallvec::SmallVec;
use std::{collections::HashMap, sync::Arc};
use wasmtime::{
    AsContext, AsContextMut, Extern, RwasmStackCounters, StoreContext, StoreContextMut, ValRaw,
    ValType,
};

/// Type of an exported function, recorded once so calls can marshal values without `Val`.
struct ExportedFunction {
    name: Box<str>,
    func: wasmtime::Func,
    params: Vec<ValType>,
    results: Vec<ValType>,
    /// Whether every parameter and result is numeric, which the raw call path requires.
    numeric: bool,
}

pub struct WasmtimeExecutor<T: 'static> {
    /// The host functions (and, in the `e2e` build, the spectest globals). Every import a module
    /// does not get a global of its own for resolves against it, see [`Self::imports`].
    pub linker: wasmtime::Linker<WrappedContext<T>>,
    pub store: wasmtime::Store<WrappedContext<T>>,
    /// The import linker `linker` was built from; resolves a module's syscall fuel schedule to
    /// syscall indices when a module is instantiated.
    import_linker: Arc<ImportLinker>,
    /// The live instance. Replaced only through [`Self::instantiate`], which swaps the cached
    /// exports and the store's syscall fuel schedule in the same step: the host trampolines read
    /// that schedule before every syscall, so an instance installed without it would be charged
    /// for the previous module's imports.
    instance: wasmtime::Instance,
    /// Exported functions of `instance`, resolved once so calls don't look them up by name.
    /// Entry points are few, so a linear scan beats hashing the name.
    functions: Vec<ExportedFunction>,
    /// Export the module was compiled for, when the config selected one.
    ///
    /// The strategy layer exposes a single entrypoint, matching the rwasm backend, which has no
    /// way to resolve an arbitrary export name at run time.
    entrypoint_name: Option<Box<str>>,
    /// The run-time cap on the instance memory, in pages, as the store was created with. Each
    /// module's compile-time cap is applied on top of it; see [`Self::store_limits`].
    max_allowed_memory_pages: u32,
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
    /// The live Wasmtime instance; see [`Self::instantiate`] to replace it.
    pub fn instance(&self) -> wasmtime::Instance {
        self.instance
    }

    /// Resolves the exported functions and the exported memory of `instance` once.
    fn refresh_exports(&mut self) {
        let mut functions = Vec::new();
        let mut memory = None;
        for export in self.instance.exports(&mut self.store) {
            let name = export.name();
            match export.into_extern() {
                Extern::Func(func) => functions.push((Box::<str>::from(name), func)),
                // Resolved by kind, not by the literal name "memory": rwasm always uses the
                // module's memory 0, so the host has to reach whichever memory the module exports
                // (multi-memory is rejected by the compiler, so there is at most one).
                Extern::Memory(exported_memory) => {
                    memory.get_or_insert(exported_memory);
                }
                _ => {}
            }
        }
        self.functions = functions
            .into_iter()
            .map(|(name, func)| {
                let ty = func.ty(&self.store);
                let params = ty.params().collect::<Vec<_>>();
                let results = ty.results().collect::<Vec<_>>();
                let numeric = params.iter().chain(&results).all(|ty| {
                    matches!(
                        ty,
                        ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
                    )
                });
                ExportedFunction {
                    name,
                    func,
                    params,
                    results,
                    numeric,
                }
            })
            .collect();
        self.store.data_mut().memory = memory;
    }

    /// Creates an executor by instantiating an already-compiled Wasmtime module.
    ///
    /// # Errors
    ///
    /// Instantiation can fail on input the caller cannot pre-validate, so the failure is reported
    /// with the trap the rwasm strategy raises for the same module rather than as a panic:
    ///
    /// - an import the linker does not provide: [`TrapCode::UnknownExternalFunction`]
    /// - an initial memory or table larger than the store allows:
    ///   [`TrapCode::MemoryOutOfBounds`] / [`TrapCode::TableOutOfBounds`]
    /// - a trapping start function: that function's trap
    /// - any other Wasmtime error: [`TrapCode::IllegalOpcode`]
    ///
    /// Use [`Self::try_new`] to get the underlying Wasmtime error instead.
    pub fn new(
        module: WasmtimeModule,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> Result<Self, TrapCode> {
        Self::try_new(
            module,
            import_linker,
            data,
            syscall_handler,
            fuel_limit,
            max_allowed_memory_pages,
        )
        .map_err(map_wasmtime_error)
    }

    /// Creates an executor by instantiating an already-compiled Wasmtime module, returning the
    /// linking or instantiation error itself.
    ///
    /// The error carries the [`TrapCode`] described on [`Self::new`] as context, so
    /// `downcast_ref::<TrapCode>()` recovers it.
    pub fn try_new(
        module: WasmtimeModule,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> wasmtime::Result<Self> {
        let max_allowed_memory_pages = max_allowed_memory_pages
            .unwrap_or(N_DEFAULT_MAX_MEMORY_PAGES)
            .min(N_MAX_ALLOWED_MEMORY_PAGES);
        let context = WrappedContext {
            syscall_handler,
            fuel: None,
            fuel_enabled: false,
            fuel_unbounded: false,
            memory: None,
            syscall_fuel: Self::resolve_syscall_fuel(&module, &import_linker)?,
            resource_limiter: Self::store_limits(max_allowed_memory_pages, &module),
            data,
        };
        let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
        store.limiter(|ctx| &mut ctx.resource_limiter);
        let fuel_enabled = store.get_fuel().is_ok();
        store.data_mut().fuel_enabled = fuel_enabled;
        if fuel_enabled {
            // A Wasmtime store starts with zero fuel, so leaving it untouched for `None` would
            // trap on the first function entry where the rwasm VM runs unbounded. Fill the store
            // and flag it unbounded instead.
            store.set_fuel(fuel_limit.unwrap_or(u64::MAX))?;
            store.data_mut().fuel_unbounded = fuel_limit.is_none();
        } else {
            store.data_mut().fuel = fuel_limit;
        }
        #[allow(unused_mut)]
        let mut linker = wasmtime_import_linker(module.engine(), &import_linker)?;
        #[cfg(feature = "e2e")]
        {
            Self::link_spectest_globals(&mut linker, &mut store);
        }
        let imports = Self::imports(&linker, &mut store, &module)?;
        let instance = Self::instantiate_in(module.module(), &imports, &mut store)?;
        let mut executor = Self {
            linker,
            store,
            import_linker,
            instance,
            functions: Vec::new(),
            entrypoint_name: None,
            max_allowed_memory_pages,
        };
        executor.refresh_exports();
        Ok(executor)
    }

    /// Records the export name the module was compiled for.
    pub fn with_entrypoint_name(mut self, entrypoint_name: Option<Box<str>>) -> Self {
        self.entrypoint_name = entrypoint_name;
        self
    }

    /// Instantiates `module` in this executor's store with its linker, replacing the current
    /// instance, its cached exports and its syscall fuel schedule.
    ///
    /// The new fuel schedule applies during initialization. If initialization fails, the
    /// previous schedule is restored without refunding fuel consumed by the failed start.
    /// Errors carry the same [`TrapCode`] context as [`Self::try_new`].
    pub fn instantiate(&mut self, module: &WasmtimeModule) -> wasmtime::Result<()> {
        let syscall_fuel = Self::resolve_syscall_fuel(module, &self.import_linker)?;
        let imports = Self::imports(&self.linker, &mut self.store, module)?;
        let previous_syscall_fuel =
            std::mem::replace(&mut self.store.data_mut().syscall_fuel, syscall_fuel);
        // the replacement brings its own compile-time memory cap
        let previous_resource_limiter = std::mem::replace(
            &mut self.store.data_mut().resource_limiter,
            Self::store_limits(self.max_allowed_memory_pages, module),
        );
        let instance = match Self::instantiate_in(module.module(), &imports, &mut self.store) {
            Ok(instance) => instance,
            Err(err) => {
                self.store.data_mut().syscall_fuel = previous_syscall_fuel;
                self.store.data_mut().resource_limiter = previous_resource_limiter;
                return Err(err);
            }
        };
        self.instance = instance;
        self.refresh_exports();
        Ok(())
    }

    /// The externs `module` instantiates with, one per import in the module's import order.
    ///
    /// A global import of a module compiled with `default_imported_global_value` gets a fresh
    /// global holding that default, in the layout the rwasm compiler gives the global it makes of
    /// the import (see `ModuleParser::process_imports`): the number for a numeric global, its low
    /// limb for a 32-bit one, null for a reference. Every other import resolves to the entry of
    /// `linker` under its name (a host function, or a spectest global of the `e2e` build), which
    /// has to be of the imported kind and type; a missing or mismatched entry is
    /// [`TrapCode::UnknownExternalFunction`], as the linker's own resolution reported it.
    ///
    /// Resolving per import, rather than by defining the globals in a `Linker`, keeps them out of
    /// the persistent linker (a module's global must not replace a host function for the modules
    /// that follow, and a replacement without a default must not inherit one) and lets a module
    /// import the same name as a function and as a global, which is valid Wasm that the rwasm
    /// compiler accepts: a linker has one entry per name and failed to instantiate such a module.
    fn imports(
        linker: &wasmtime::Linker<WrappedContext<T>>,
        store: &mut wasmtime::Store<WrappedContext<T>>,
        module: &WasmtimeModule,
    ) -> wasmtime::Result<Vec<Extern>> {
        use wasmtime::{ExternType, Global, Val};
        let default_value = module.default_imported_global_value();
        let mut imports = Vec::with_capacity(module.module().imports().len());
        for import in module.module().imports() {
            let import_type = import.ty();
            if let (ExternType::Global(global_type), Some(default_value)) =
                (&import_type, default_value)
            {
                let value = match global_type.content().clone() {
                    ValType::I32 => Val::I32(default_value as i32),
                    ValType::I64 => Val::I64(default_value),
                    // a 32-bit global carries its initializer in the low limb of the value, see
                    // `SegmentBuilder::add_global_variable`
                    ValType::F32 => Val::F32(default_value as u32),
                    ValType::F64 => Val::F64(default_value as u64),
                    // a reference global starts null, whatever the default
                    ty if ty.is_funcref() => Val::FuncRef(None),
                    ty if ty.is_externref() => Val::ExternRef(None),
                    ty => {
                        return Err(wasmtime::Error::msg(format!(
                            "wasmtime: unsupported type `{ty}` of the imported global `{}::{}`",
                            import.module(),
                            import.name()
                        )))
                    }
                };
                let global = Global::new(&mut *store, global_type.clone(), value)?;
                imports.push(Extern::Global(global));
                continue;
            }
            let unknown = |err: wasmtime::Error| err.context(TrapCode::UnknownExternalFunction);
            let definition = linker
                .get(&mut *store, import.module(), import.name())
                .map_err(unknown)?;
            if !Self::import_matches(&definition.ty(&*store), &import_type) {
                return Err(unknown(wasmtime::Error::msg(format!(
                    "wasmtime: import `{}::{}` differs in type from the linker's definition",
                    import.module(),
                    import.name()
                ))));
            }
            imports.push(definition);
        }
        Ok(imports)
    }

    /// Whether a linker definition of type `actual` satisfies an import of type `expected`.
    ///
    /// Functions and globals, the kinds the rwasm compiler admits as imports, are checked here;
    /// any other kind is left to Wasmtime's own check at instantiation.
    fn import_matches(actual: &wasmtime::ExternType, expected: &wasmtime::ExternType) -> bool {
        use wasmtime::ExternType;
        match (actual, expected) {
            (ExternType::Func(actual), ExternType::Func(expected)) => actual.matches(expected),
            (ExternType::Global(actual), ExternType::Global(expected)) => {
                actual.mutability() == expected.mutability()
                    && actual.content().matches(expected.content())
            }
            (ExternType::Func(_) | ExternType::Global(_), _)
            | (_, ExternType::Func(_) | ExternType::Global(_)) => false,
            _ => true,
        }
    }

    /// The store limits for running `module` under a run-time cap of `max_allowed_memory_pages`.
    ///
    /// The memory may grow up to the lower of the run-time cap and the module's compile-time cap:
    /// the rwasm VM bounds its memory by the store's cap and every compiled `memory.grow` by the
    /// config's, so a grow past either reports `-1` there. Applying only the run-time cap here
    /// used to let the same module grow further on this backend.
    fn store_limits(
        max_allowed_memory_pages: u32,
        module: &WasmtimeModule,
    ) -> RecordingStoreLimits {
        let memory_pages = max_allowed_memory_pages.min(module.max_allowed_memory_pages());
        let memory_size_limit = (memory_pages as usize)
            .checked_mul(N_BYTES_PER_MEMORY_PAGE as usize)
            .expect("wasmtime: memory limit is bounded by N_MAX_ALLOWED_MEMORY_PAGES");
        // the rwasm VM caps every table at `N_MAX_TABLE_SIZE` elements (`TableEntity::grow_untyped`
        // fails any grow beyond it); apply the same per-table cap here so `table.grow` reports
        // the same failures on both strategies
        RecordingStoreLimits::new(
            wasmtime::StoreLimitsBuilder::new()
                .memory_size(memory_size_limit)
                .table_elements(N_MAX_TABLE_SIZE as usize)
                .build(),
        )
    }

    /// Resolves the module's syscall fuel schedule (by import name) to the syscall indices the
    /// host trampolines are keyed by.
    ///
    /// A policy whose metered parameter does not name an `i32` parameter of the import is
    /// rejected up front with [`TrapCode::BadSignature`], as the rwasm compiler rejects the
    /// same linker entry at compile time; charging it would trap every call instead.
    fn resolve_syscall_fuel(
        module: &WasmtimeModule,
        import_linker: &ImportLinker,
    ) -> wasmtime::Result<HashMap<u32, SyscallFuelParams>> {
        let mut syscall_fuel = HashMap::new();
        for (import_name, entity) in import_linker.iter() {
            let Some(policy) = module.syscall_fuel().get(&import_name) else {
                continue;
            };
            let metered = match policy {
                SyscallFuelParams::None | SyscallFuelParams::Const(_) => None,
                SyscallFuelParams::LinearFuel(params) => Some(params.param_index),
                SyscallFuelParams::QuadraticFuel(params) => Some(params.local_depth),
            };
            if let Some(param_index) = metered {
                let is_i32 = usize::try_from(param_index)
                    .ok()
                    .filter(|index| *index >= 1)
                    .and_then(|index| entity.params.len().checked_sub(index))
                    .and_then(|index| entity.params.get(index))
                    .is_some_and(|ty| *ty == wasmparser::ValType::I32);
                if !is_i32 {
                    return Err(wasmtime::Error::msg(format!(
                        "wasmtime: syscall fuel of import `{import_name}` meters a parameter that is not an i32"
                    ))
                    .context(TrapCode::BadSignature));
                }
            }
            syscall_fuel.insert(entity.sys_func_idx, policy.clone());
        }
        Ok(syscall_fuel)
    }

    /// Instantiates `module` with `imports` in `store`.
    ///
    /// A failure caused by the store's resource limits is tagged with the trap the rwasm
    /// entrypoint prologue raises for the same module, so both strategies report an oversized
    /// initial memory or table identically.
    fn instantiate_in(
        module: &wasmtime::Module,
        imports: &[Extern],
        store: &mut wasmtime::Store<WrappedContext<T>>,
    ) -> wasmtime::Result<wasmtime::Instance> {
        store.data_mut().resource_limiter.reset_denied();
        // The rwasm entrypoint calls a `start` function as a frame of its own, so it runs one
        // frame deep. It runs above the entrypoint's parameters on rwasm, which are unknown at
        // instantiation; they are taken as none.
        store.set_rwasm_stack_counters(RwasmStackCounters {
            call_depth: 1,
            stack_slots: 0,
        });
        match wasmtime::Instance::new(store.as_context_mut(), module, imports) {
            Ok(instance) => Ok(instance),
            Err(err) => {
                // A refused initial memory or table aborts instantiation with a plain error. A
                // refused `memory.grow`/`table.grow` inside the start function, however, is
                // reported to the guest as `-1` and execution goes on; if the function then fails
                // for a reason of its own, the error already carries a trap or a `TrapCode`, and
                // that reason must win over the handled denial.
                let already_classified = err.downcast_ref::<wasmtime::Trap>().is_some()
                    || err.downcast_ref::<TrapCode>().is_some();
                Err(match store.data().resource_limiter.denied() {
                    Some(trap_code) if !already_classified => err.context(trap_code),
                    _ => err,
                })
            }
        }
    }

    /// Looks up an exported function in the cached export table.
    fn exported_function(&self, func_name: &str) -> Option<usize> {
        self.functions
            .iter()
            .position(|function| &*function.name == func_name)
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
        // A module compiled with a named entrypoint is only callable through that name: the rwasm
        // backend cannot resolve another export, so resolving one here would run different code
        // depending on the strategy.
        if let Some(expected) = self.entrypoint_name.as_deref() {
            if expected != func_name {
                return Err(TrapCode::UnknownExternalFunction);
            }
        }
        let index = self
            .exported_function(func_name)
            .ok_or(TrapCode::UnknownExternalFunction)?;
        let function = &self.functions[index];
        // The rwasm entrypoint tail-calls the export the host asked for: it runs as the
        // outermost frame, with nothing on the value stack below its parameters.
        self.store
            .set_rwasm_stack_counters(RwasmStackCounters::default());
        if function.numeric {
            return Self::execute_raw(&mut self.store, function, params, result);
        }
        // The result placeholders are the declared types' zeros. A halted call writes no results
        // and the caller then gets the placeholders, as the rwasm VM reports zeros of the declared
        // types; `i32` placeholders used to report `I32(0)` for a `funcref` result there.
        let placeholders = function
            .results
            .iter()
            .map(|ty| {
                wasmtime::Val::default_for_ty(ty)
                    .expect("wasmtime: every result type of the rwasm language has a zero value")
            })
            .collect::<SmallVec<[wasmtime::Val; 8]>>();
        self.execute_checked(function.func, placeholders, params, result)
    }

    /// Calls a numeric-only export through raw value slots, skipping `Val` marshalling.
    fn execute_raw(
        store: &mut wasmtime::Store<WrappedContext<T>>,
        function: &ExportedFunction,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        // The checked call path reports a signature mismatch as a generic wasmtime error, which
        // `map_wasmtime_error` turns into `IllegalOpcode`; keep that mapping.
        if params.len() != function.params.len() || result.len() != function.results.len() {
            return Err(TrapCode::IllegalOpcode);
        }
        let mut slots = SmallVec::<[ValRaw; 8]>::new();
        for (value, ty) in params.iter().zip(&function.params) {
            slots.push(match (value, ty) {
                (Value::I32(value), ValType::I32) => ValRaw::i32(*value),
                (Value::I64(value), ValType::I64) => ValRaw::i64(*value),
                (Value::F32(value), ValType::F32) => ValRaw::f32(value.to_bits()),
                (Value::F64(value), ValType::F64) => ValRaw::f64(value.to_bits()),
                _ => return Err(TrapCode::IllegalOpcode),
            });
        }
        slots.resize(params.len().max(result.len()), ValRaw::i32(0));
        // SAFETY: `slots` holds one initialized value per parameter, of the types recorded from
        // the function's own type when the export was cached, and has room for every result. The
        // function is numeric only, so no reference types need rooting.
        let halted = match unsafe { function.func.call_unchecked(&mut *store, &mut slots[..]) }
            .map_err(map_wasmtime_error)
        {
            Ok(()) => false,
            Err(TrapCode::ExecutionHalted) => true,
            Err(trap_code) => return Err(trap_code),
        };
        for ((slot, out), ty) in slots.iter().zip(result.iter_mut()).zip(&function.results) {
            // A halted call never wrote its results, so the slots still hold parameter bits;
            // report zeros of the declared types instead of reinterpreting them.
            *out = match ty {
                ValType::I32 => Value::I32(if halted { 0 } else { slot.get_i32() }),
                ValType::I64 => Value::I64(if halted { 0 } else { slot.get_i64() }),
                ValType::F32 => Value::F32(F32::from_bits(if halted { 0 } else { slot.get_f32() })),
                ValType::F64 => Value::F64(F64::from_bits(if halted { 0 } else { slot.get_f64() })),
                _ => unreachable!("wasmtime: raw call path taken for a non-numeric export"),
            };
        }
        Ok(())
    }

    /// Calls an export through wasmtime's checked `Val` interface; needed for reference types.
    ///
    /// An `externref` carries its index across. A `funcref` crosses only as the null reference:
    /// the rwasm VM's function references are code offsets, which have no counterpart in a
    /// Wasmtime `Func`, so a non-null parameter is a type mismatch (`IllegalOpcode`) before the
    /// call runs and a non-null result one after it, with the caller's buffer left as it was.
    /// The compiler admits such an entrypoint only under `allow_func_ref_function_types` (the
    /// spec harness), but a module compiled through `compile_wasmtime_module` gets here without
    /// that policy, so the marshalling is not tied to a build feature.
    fn execute_checked(
        &mut self,
        entrypoint: wasmtime::Func,
        placeholders: SmallVec<[wasmtime::Val; 8]>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        use wasmtime::Val;
        // a wrong result count is the signature mismatch the raw path reports as well
        if result.len() != placeholders.len() {
            return Err(TrapCode::IllegalOpcode);
        }
        let mut buffer = Vec::<Val>::default();
        for value in params {
            let value = match value {
                Value::I32(value) => Val::I32(*value),
                Value::I64(value) => Val::I64(*value),
                Value::F32(value) => Val::F32(value.to_bits()),
                Value::F64(value) => Val::F64(value.to_bits()),
                Value::FuncRef(value) if value.is_null() => Val::FuncRef(None),
                Value::FuncRef(_) => return Err(TrapCode::IllegalOpcode),
                Value::ExternRef(value) => {
                    let func_idx = value.0;
                    if func_idx == 0 {
                        Val::ExternRef(None)
                    } else {
                        Val::ExternRef(wasmtime::ExternRef::new(&mut self.store, func_idx).ok())
                    }
                }
            };
            buffer.push(value);
        }
        buffer.extend(placeholders);
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
        let mut values = SmallVec::<[Value; 8]>::new();
        for x in mapped_result.iter().cloned() {
            values.push(match x {
                Val::I32(value) => Value::I32(value),
                Val::I64(value) => Value::I64(value),
                Val::F32(value) => Value::F32(F32::from_bits(value)),
                Val::F64(value) => Value::F64(F64::from_bits(value)),
                Val::FuncRef(None) => Value::FuncRef(crate::FuncRef::null()),
                Val::FuncRef(Some(_)) => return Err(TrapCode::IllegalOpcode),
                Val::ExternRef(value) => {
                    let value: Option<&u32> = value
                        .and_then(|ext_ref| ext_ref.data(&mut self.store).ok().flatten())
                        .and_then(|v| v.downcast_ref());
                    Value::ExternRef(crate::ExternRef::new(value.copied().unwrap_or_default()))
                }
                _ => unreachable!("wasmtime: not supported type: {:?}", x),
            });
        }
        result.clone_from_slice(&values);
        Ok(())
    }

    /// Interruptions are not supported on the Wasmtime strategy, so there is never an execution
    /// to resume; this always fails with [`TrapCode::IllegalOpcode`], the same error the rwasm
    /// engine reports for a `resume` without an interrupted execution.
    pub fn resume(
        &mut self,
        interruption_result: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        Err(TrapCode::IllegalOpcode)
    }

    pub fn snapshot_memory(&mut self) -> Result<Vec<u8>, TrapCode> {
        // a module without memory snapshots as the zero-page memory the rwasm VM gives it
        let Some(global_memory) = self.store.data().memory else {
            return Ok(Vec::new());
        };
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
        let Some(global_memory) = self.store.data().memory else {
            return missing_memory_access(offset, buffer.len());
        };
        global_memory
            .read(self.store.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        let end = checked_memory_range_end(offset, length)?;
        let Some(global_memory) = self.store.data().memory else {
            return missing_memory_access(offset, length).map(|()| Vec::new());
        };
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
        let Some(global_memory) = self.store.data().memory else {
            return missing_memory_access(offset, buffer.len());
        };
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
        crate::wasmtime::context::try_consume_fuel(&mut self.store, delta)
    }

    fn remaining_fuel(&self) -> Option<u64> {
        crate::wasmtime::context::remaining_fuel(&self.store)
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        crate::wasmtime::context::reset_fuel(&mut self.store, new_fuel_limit)
    }
}

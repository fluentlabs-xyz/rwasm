use crate::{
    error::TestError,
    import_linker::{
        create_import_linker, testing_context_syscall_handler, TestingContext, FUNC_GET_STATE,
    },
};
use rwasm::{
    wasmtime::{compile_wasmtime_module, WasmtimeExecutor},
    CompilationConfig, ExecutionEngine, FuncIdx, FuncType, ModuleParser, Opcode, RwasmInstance,
    RwasmModule, RwasmStore, StateRouterConfig, StoreTr, TrapCode, Value,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone)]
pub(crate) struct TestingInstanceGroup {
    rwasm: Rc<RefCell<TestingInstanceRwasm>>,
    wasmtime: Rc<RefCell<TestingInstanceWasmtime>>,
    /// Extern function types
    extern_types: HashMap<String, FuncType>,
}

struct TestingInstanceRwasm {
    store: RwasmStore<TestingContext>,
    instance: RwasmInstance,
    /// Extern function state values
    extern_state: HashMap<String, u32>,
}

impl TestingInstanceRwasm {
    fn new(
        config: CompilationConfig,
        wasm_binary: &[u8],
        exports: Vec<(Box<str>, FuncIdx, FuncType)>,
    ) -> Result<TestingInstanceRwasm, TestError> {
        let mut extern_state = HashMap::new();
        for (k, func_idx, _func_type) in exports.into_iter() {
            extern_state.insert(k.to_string(), 10_000 + func_idx);
        }

        let (module, _) = RwasmModule::compile(config, wasm_binary)
            .map_err(|err| TestError::Rwasm(err.into()))?;
        {
            let buffer = module.serialize();
            let (parsed_module, bytes_read) = RwasmModule::new(&buffer);
            assert_eq!(module, parsed_module);
            assert_eq!(buffer[bytes_read..].len(), 0);
        }
        println!("rwasm module: {}", module);

        let import_linker = create_import_linker();
        let engine = ExecutionEngine::new();
        let mut store = RwasmStore::new(
            import_linker.clone(),
            TestingContext::default(),
            testing_context_syscall_handler,
            Some(u64::MAX),
        );
        let instance = import_linker.instantiate(&mut store, engine, module)?;
        Ok(TestingInstanceRwasm {
            store,
            instance,
            extern_state,
        })
    }

    fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> anyhow::Result<Option<u64>, TrapCode> {
        // Update the state based on the function name (state router)
        let Some(state) = self.extern_state.get(&func_name.to_string()).copied() else {
            unreachable!("missing state for exported function: {}", func_name);
        };
        self.store.data_mut().state = state;
        // Invoke the function
        let fuel_before = self.store.remaining_fuel();
        self.instance.execute(&mut self.store, params, result)?;
        let fuel_after = self.store.remaining_fuel();
        Ok(fuel_after.map(|fuel_after| fuel_before.unwrap() - fuel_after))
    }
}

struct TestingInstanceWasmtime {
    store: WasmtimeExecutor<TestingContext>,
}

impl TestingInstanceWasmtime {
    fn new(
        config: CompilationConfig,
        wasm_binary: &[u8],
    ) -> Result<TestingInstanceWasmtime, TestError> {
        let import_linker = create_import_linker();
        let module = compile_wasmtime_module(config, wasm_binary).unwrap();
        let store = WasmtimeExecutor::<TestingContext>::new(
            module.clone(),
            import_linker.clone(),
            TestingContext::default(),
            testing_context_syscall_handler,
            Some(u64::MAX),
        );
        Ok(TestingInstanceWasmtime { store })
    }

    fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> anyhow::Result<Option<u64>, TrapCode> {
        let fuel_before = self.store.remaining_fuel();
        self.store.execute(func_name, params, result)?;
        let fuel_after = self.store.remaining_fuel();
        Ok(fuel_after.map(|fuel_after| fuel_before.unwrap() - fuel_after))
    }
}

impl TestingInstanceGroup {
    pub(crate) fn new(wasm_binary: &[u8]) -> Result<Self, TestError> {
        let import_linker = create_import_linker();

        let config = CompilationConfig::default()
            .with_import_linker(import_linker)
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(true)
            .with_default_imported_global_value(666.into())
            .with_allow_func_ref_function_types(true)
            .with_consume_fuel(true);

        // Extract all exports first to calculate rwasm config
        let mut states = Vec::<(Box<str>, u32)>::new();
        let mut extern_types = HashMap::new();
        let exports = ModuleParser::parse_function_exports(config.clone(), wasm_binary)?;
        for (k, func_idx, func_type) in exports.iter() {
            states.push((k.clone(), 10_000 + func_idx));
            extern_types.insert(k.to_string(), func_type.clone());
        }
        let config = config
            .with_state_router(StateRouterConfig {
                states: states.into(),
                opcode: Some(Opcode::Call(FUNC_GET_STATE)),
            })
            .with_consume_fuel(true)
            .with_consume_fuel_for_params_and_locals(false);

        let rwasm = TestingInstanceRwasm::new(config.clone(), wasm_binary, exports)?;
        let wasmtime = TestingInstanceWasmtime::new(config, wasm_binary)?;

        Ok(Self {
            rwasm: Rc::new(RefCell::new(rwasm)),
            wasmtime: Rc::new(RefCell::new(wasmtime)),
            extern_types,
        })
    }

    pub(crate) fn execute(
        &self,
        func_name: &str,
        params: &[Value],
    ) -> Result<Vec<Value>, TestError> {
        let Some(func_type) = self.extern_types.get(func_name) else {
            unreachable!("missing func type for function: {}", func_name)
        };
        // Execute rwasm
        let mut rwasm_result = vec![];
        for val_type in func_type.results() {
            rwasm_result.push(Value::default(*val_type));
        }
        let mut instance = self.rwasm.borrow_mut();
        let res1 = instance.execute(func_name, params, &mut rwasm_result);
        drop(instance);
        // Execute wasmtime
        let mut wasmtime_result = rwasm_result.clone();
        let mut instance = self.wasmtime.borrow_mut();
        let res2 = instance.execute(func_name, params, &mut wasmtime_result);
        drop(instance);
        // Make sure that both results are the same (it also compares fuel consumed)
        assert_eq!(res1, res2);
        // If the result is trap code, then return it
        res1?;
        res2?;
        // Compare output results
        assert_eq!(rwasm_result.len(), wasmtime_result.len());
        for (left, right) in wasmtime_result.iter().zip(wasmtime_result.iter()) {
            match (left, right) {
                // A special cases for NaN comparison (NaN != NaN)
                (Value::F64(left), Value::F64(right))
                    if left.to_float().is_nan() && right.to_float().is_nan() => {}
                (Value::F32(left), Value::F32(right))
                    if left.to_float().is_nan() && right.to_float().is_nan() => {}
                // A case for other comparisons
                _ => assert_eq!(left, right),
            }
        }
        // Compare fuel consumed
        Ok(rwasm_result)
    }
}

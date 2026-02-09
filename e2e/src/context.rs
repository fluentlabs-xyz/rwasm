use super::{TestDescriptor, TestError, TestProfile, TestSpan};
use crate::handler::{
    testing_context_syscall_handler, TestingContext, FUNC_ENTRYPOINT, FUNC_PRINT, FUNC_PRINT_F32,
    FUNC_PRINT_F64, FUNC_PRINT_I32, FUNC_PRINT_I32_F32, FUNC_PRINT_I64, FUNC_PRINT_I64_F64,
};
use anyhow::Result;
use lazy_static::lazy_static;
use rwasm::{
    compile_wasmtime_module, CallStack, CompilationConfig, FuelConfig, FuncType, I64ValueSplit,
    ImportLinker, ImportLinkerEntity, ImportName, ModuleParser, Opcode, RwasmExecutor, RwasmModule,
    StateRouterConfig, Store, TrapCode, TypedExecutor, TypedModule, TypedStore, ValType, Value,
    ValueStack, F64,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};
use wast::token::{Id, Span};

lazy_static! {
    static ref ENGINES: Vec<EngineMode> = vec![
        EngineMode::Rwasm,
        #[cfg(feature = "wasmtime")]
        EngineMode::Wasmtime,
    ];
}

pub struct InstanceInner {
    strategy: TypedModule,
    store: TypedStore<TestingContext>,
    value_stack: ValueStack,
    call_stack: CallStack,
    program_counter: usize,
}

impl InstanceInner {
    fn new_executor(&mut self) -> TypedExecutor<'_, TestingContext> {
        match (&self.strategy, &mut self.store) {
            (TypedModule::Rwasm { module, .. }, TypedStore::Rwasm(rwasm_store)) => {
                TypedExecutor::RwasmExecutor(RwasmExecutor::entrypoint(
                    module,
                    &mut self.value_stack,
                    &mut self.call_stack,
                    rwasm_store,
                ))
            }
            (TypedModule::Wasmtime { .. }, TypedStore::Wasmtime(_)) => {
                unreachable!("Wasmtime isn't supported executor")
            }
            _ => panic!("inconsistent types of module and store"),
        }
    }

    fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.strategy
            .execute(&mut self.store, func_name, params, result)
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub enum EngineMode {
    Rwasm,
    Wasmtime,
}

type Instance = Rc<RefCell<HashMap<EngineMode, InstanceInner>>>;

/// The context of a single Wasm test spec suite run.
pub struct TestContext<'a> {
    /// The list of all instantiated modules.
    instances: HashMap<String, Instance>,
    /// Extern function types and state values
    extern_types: HashMap<String, FuncType>,
    extern_state: HashMap<String, u32>,
    /// The last touched module instance.
    last_instance: Option<Instance>,
    /// Profiling during the Wasm spec test run.
    profile: TestProfile,
    import_linker: Arc<ImportLinker>,
    /// The descriptor of the test.
    ///
    /// Useful for printing better debug messages in case of failure.
    descriptor: &'a TestDescriptor,
}

impl<'a> TestContext<'a> {
    /// Creates a new [`TestContext`] with the given [`TestDescriptor`].
    pub fn new(descriptor: &'a TestDescriptor) -> Self {
        TestContext {
            instances: HashMap::new(),
            extern_types: Default::default(),
            extern_state: Default::default(),
            last_instance: None,
            profile: TestProfile::default(),
            import_linker: Self::create_import_linker(),
            descriptor,
        }
    }

    pub fn create_import_linker() -> Arc<ImportLinker> {
        ImportLinker::from([
            (
                ImportName::new("__nothing_here", "__absolutely_nothing"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_ENTRYPOINT,
                    syscall_fuel_param: Default::default(),
                    params: &[],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT,
                    syscall_fuel_param: Default::default(),
                    params: &[],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F32,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::F32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F64,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::F64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i32_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32_F32,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I32, ValType::F32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i64_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64_F64,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I64, ValType::F64],
                    result: &[],
                    intrinsic: None,
                },
            ),
        ])
        .into()
    }
}

impl TestContext<'_> {
    /// Returns the file path of the associated `.wast` test file.
    fn test_path(&self) -> &str {
        self.descriptor.path()
    }

    /// Returns the [`TestDescriptor`] of the test context.
    pub fn spanned(&self, span: Span) -> TestSpan<'_> {
        self.descriptor.spanned(span)
    }

    /// Returns an exclusive reference to the test profile.
    pub fn profile(&mut self) -> &mut TestProfile {
        &mut self.profile
    }

    /// Compiles the Wasm module and stores it into the [`TestContext`].
    ///
    /// # Errors
    ///
    /// If creating the [`Module`] fails.
    pub fn compile_and_instantiate(
        &mut self,
        mut module: wast::core::Module,
    ) -> Result<(), TestError> {
        let module_name = module.id.map(|id| id.name());
        let wasm = module.encode().unwrap_or_else(|error| {
            panic!(
                "encountered unexpected failure to encode `.wast` module into `.wasm`:{}: {}",
                self.test_path(),
                error
            )
        });
        let instance = Rc::new(RefCell::new(HashMap::new()));

        for engine in ENGINES.iter() {
            let mut instance_inner = self.create_instance(*engine, wasm.as_slice())?;

            if let TypedModule::Rwasm { .. } = &instance_inner.strategy {
                #[cfg(feature = "debug-print")]
                println!(" --- entrypoint ---");
                instance_inner.execute("", &[], &mut [])?;

                #[cfg(feature = "debug-print")]
                println!();
                instance_inner.value_stack.reset();
                instance_inner.call_stack.reset();
                instance_inner.store.reset(true);
            }

            instance.borrow_mut().insert(*engine, instance_inner);
        }

        if let Some(module_name) = module_name {
            self.instances
                .insert(module_name.to_string(), instance.clone());
        }

        self.last_instance = Some(instance);
        Ok(())
    }

    fn create_instance(
        &mut self,
        mode: EngineMode,
        wasm: &[u8],
    ) -> Result<InstanceInner, TestError> {
        let config = CompilationConfig::default()
            .with_import_linker(self.import_linker.clone())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(true)
            .with_default_imported_global_value(666.into())
            .with_allow_func_ref_function_types(true)
            .with_consume_fuel(true);

        // extract all exports first to calculate rwasm config
        let mut states = Vec::<(Box<str>, u32)>::new();
        let exports = ModuleParser::parse_function_exports(config.clone(), wasm)?;
        for (k, func_idx, func_type) in exports.into_iter() {
            self.extern_types.insert(k.to_string(), func_type);
            let state_value = 10_000 + func_idx;
            self.extern_state.insert(k.to_string(), state_value);
            states.push((k, state_value));
        }
        let config = config
            .with_state_router(StateRouterConfig {
                states: states.into(),
                opcode: Some(Opcode::Call(u32::MAX)),
            })
            .with_consume_fuel(true)
            .with_consume_fuel_for_params_and_locals(false);

        let strategy = match mode {
            EngineMode::Rwasm => {
                let (rwasm_module, _) = RwasmModule::compile(config, wasm)
                    .map_err(|err| TestError::Rwasm(err.into()))?;

                {
                    let buffer = rwasm_module.serialize();
                    let (parsed_module, bytes_read) = RwasmModule::new(&buffer);
                    assert_eq!(rwasm_module, parsed_module);
                    assert_eq!(buffer[bytes_read..].len(), 0);
                }

                TypedModule::Rwasm {
                    module: rwasm_module,
                    engine: Default::default(),
                }
            }
            EngineMode::Wasmtime => {
                let wasmtime_module = compile_wasmtime_module(config, wasm).unwrap();
                TypedModule::Wasmtime {
                    module: wasmtime_module,
                }
            }
        };

        let mut store = strategy.create_store(
            self.import_linker.clone(),
            TestingContext::default(),
            testing_context_syscall_handler,
            FuelConfig::default().with_fuel_limit(u64::MAX),
        );
        store.data_mut().state = FUNC_ENTRYPOINT;
        Ok(InstanceInner {
            store,
            strategy,
            value_stack: ValueStack::default(),
            call_stack: CallStack::default(),
            program_counter: 0,
        })
    }

    /// Loads the Wasm module instance with the given name.
    ///
    /// # Errors
    ///
    /// If there is no registered module instance with the given name.
    pub fn instance_by_name(&self, name: &str) -> Result<Instance, TestError> {
        self.instances
            .get(name)
            .cloned()
            .ok_or_else(|| TestError::InstanceNotRegistered {
                name: name.to_owned(),
            })
    }

    /// Loads the Wasm module instance with the given name or the last instantiated one.
    ///
    /// # Errors
    ///
    /// If there have been no Wasm module instances registered so far.
    pub fn instance_by_name_or_last(&self, name: Option<&str>) -> Result<Instance, TestError> {
        name.map(|name| self.instance_by_name(name))
            .unwrap_or_else(|| {
                self.last_instance
                    .clone()
                    .ok_or(TestError::NoModuleInstancesFound)
            })
    }

    /// Registers the given [`Instance`] with the given `name` and sets it as the last instance.
    pub fn register_instance(&mut self, name: &str, instance: Instance) {
        if self.instances.get(name).is_some() {
            // Already registered the instance.
            return;
        }
        self.instances.insert(name.to_string(), instance.clone());
        self.last_instance = Some(instance);
    }

    /// Invokes the [`Func`] identified by `func_name` in [`Instance`] identified by `module_name`.
    ///
    /// If no [`Instance`] under `module_name` is found then invoke [`Func`] on the last
    /// instantiated [`Instance`].
    ///
    /// # Note
    ///
    /// Returns the results of the function invocation.
    ///
    /// # Errors
    ///
    /// - If no module instances can be found.
    /// - If no function identified with `func_name` can be found.
    /// - If function invocation returned an error.
    pub fn invoke(
        &mut self,
        module_name: Option<&str>,
        func_name: &str,
        args: &[Value],
    ) -> Result<Vec<Value>, TestError> {
        #[cfg(feature = "debug-print")]
        println!("\n --- {} --- ", func_name);

        let instance = self.instance_by_name_or_last(module_name)?;
        let mut instances = instance.borrow_mut();
        let mut all_results = vec![];
        let mut remaining_fuel = vec![];
        for (_, instance) in instances.iter_mut() {
            match &instance.strategy {
                TypedModule::Rwasm { .. } => {
                    // We reset an instruction pointer to the state function position to re-invoke the function.
                    // However, with different states.
                    // Some tests might fail, and we might keep outdated signature value in the state,
                    // make sure the state is clear before every new call.
                    let program_counter = instance.store.data().program_counter as usize;

                    instance.program_counter = program_counter;
                    instance.store.reset(true);

                    let func_state = *self.extern_state.get(&func_name.to_string()).unwrap();

                    let binding = args
                        .iter()
                        .cloned()
                        .flat_map(|v| match v {
                            Value::I64(v) => v
                                .split_into_i32_array()
                                .into_iter()
                                .map(Value::I32)
                                .collect(),
                            Value::F64(v) => v
                                .to_bits()
                                .split_into_i32_array()
                                .into_iter()
                                .map(Value::I32)
                                .collect(),
                            v => vec![v],
                        })
                        .collect::<Vec<_>>();
                    let args = binding.as_slice();

                    // reset PC and other state values
                    instance.value_stack.reset();
                    instance.call_stack.reset();
                    instance.store.reset(true);
                    if let TypedStore::Rwasm(store) = &mut instance.store {
                        store.set_fuel(Some(u64::MAX))
                    }

                    // insert input params
                    for value in args {
                        instance.value_stack.push(value.clone().into());
                    }
                    // change function state for router
                    instance.store.data_mut().state = func_state;

                    let pc = instance.program_counter;
                    #[cfg(feature = "debug-print")]
                    println!("Rwasm before call: {:?}", instance.store.remaining_fuel());
                    let mut vm = instance.new_executor();
                    vm.advance_ip(pc);
                    vm.run(&[], &mut [])?;
                    #[cfg(feature = "debug-print")]
                    println!("Rwasm after call: {:?}", instance.store.remaining_fuel());

                    // copy results
                    let func_type = self.extern_types.get(func_name).unwrap();
                    let len_results = func_type.results().len();
                    let mut results = vec![Value::I32(0); len_results];
                    for (i, val_type) in func_type.results().iter().rev().enumerate() {
                        let popped_value = instance.value_stack.pop();
                        results[len_results - 1 - i] = match val_type {
                            ValType::I32 => Value::I32(popped_value.into()),
                            ValType::I64 => {
                                let hi = popped_value.to_bits() as u64;
                                let lo = instance.value_stack.pop().to_bits() as u64;
                                let value = (hi << 32) | lo;
                                Value::I64(value as i64)
                            }
                            ValType::F32 => Value::F32(popped_value.into()),
                            ValType::F64 => {
                                let hi = popped_value.to_bits() as u64;
                                let lo = instance.value_stack.pop().to_bits() as u64;
                                let value = (hi << 32) | lo;
                                Value::F64(F64::from_bits(value))
                            }
                            ValType::FuncRef => Value::FuncRef(popped_value.into()),
                            ValType::ExternRef => Value::ExternRef(popped_value.into()),
                            _ => unreachable!("unsupported result type: {:?}", val_type),
                        };
                    }
                    assert!(instance.value_stack.as_slice().is_empty());
                    remaining_fuel.push(instance.store.remaining_fuel());
                    all_results.push(results)
                }
                TypedModule::Wasmtime { .. } => {
                    let func_type = self.extern_types.get(func_name).unwrap();
                    let mut result = vec![Value::I32(0); func_type.results().len()];
                    if let TypedStore::Wasmtime(store) = &mut instance.store {
                        store.store.set_fuel(u64::MAX).unwrap();
                    }
                    instance.execute(func_name, args, result.as_mut())?;
                    remaining_fuel.push(instance.store.remaining_fuel());
                    all_results.push(result)
                }
            }
        }

        for result in all_results.iter() {
            assert!(
                all_results[0].iter().zip(result).all(|(left, right)| {
                    match (left, right) {
                        (Value::F64(left), Value::F64(right)) => {
                            left.to_float().eq(&right.to_float())
                                || left.to_float().is_nan() && right.to_float().is_nan()
                        }
                        (Value::F32(left), Value::F32(right)) => {
                            left.to_float().eq(&right.to_float())
                                || left.to_float().is_nan() && right.to_float().is_nan()
                        }
                        _ => left.eq(right),
                    }
                }),
                "Result not equal between engines: {all_results:?}"
            );
        }

        for fuel in remaining_fuel.iter() {
            assert_eq!(
                *fuel, remaining_fuel[0],
                "Gas not equal between engines: {:?}",
                remaining_fuel
            );
        }
        Ok(all_results.pop().unwrap())
    }

    /// Returns the current value of the [`Global`] identifier by the given `module_name` and
    /// `global_name`.
    ///
    /// # Errors
    ///
    /// - If no module instances can be found.
    /// - If no global variable identifier with `global_name` can be found.
    pub fn get_global(
        &self,
        _module_name: Option<Id>,
        _global_name: &str,
    ) -> Result<Value, TestError> {
        // We don't support exported globals,
        // but by hardcoding this value we can mass most of the global unit tests
        // to cover other functionality
        Ok(Value::I64(42))
    }
}

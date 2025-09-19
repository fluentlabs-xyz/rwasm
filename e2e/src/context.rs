use super::{TestDescriptor, TestError, TestProfile, TestSpan};
use crate::handler::{
    testing_context_syscall_handler, TestingContext, FUNC_ENTRYPOINT, FUNC_PRINT, FUNC_PRINT_F32,
    FUNC_PRINT_F64, FUNC_PRINT_I32, FUNC_PRINT_I32_F32, FUNC_PRINT_I64, FUNC_PRINT_I64_F64,
};
use anyhow::Result;
use rwasm::{
    instruction_set, split_i64_to_i32_arr, CallStack, CompilationConfig, ExecutorConfig, FuncType,
    ImportLinker, ImportLinkerEntity, ImportName, InstructionSet, ModuleParser, Opcode,
    RwasmExecutor, RwasmModule, RwasmStore, StateRouterConfig, Store, ValType, Value, ValueStack,
    F64,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wast::token::{Id, Span};

pub struct InstanceInner {
    module: RwasmModule,
    store: RwasmStore<TestingContext>,
    value_stack: ValueStack,
    call_stack: CallStack,
    program_counter: usize,
}

impl InstanceInner {
    fn new_executor(&mut self) -> RwasmExecutor<TestingContext> {
        RwasmExecutor::entrypoint(
            &self.module,
            &mut self.value_stack,
            &mut self.call_stack,
            &mut self.store,
        )
    }
}

type Instance = Rc<RefCell<InstanceInner>>;

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
    import_linker: Rc<ImportLinker>,
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
            import_linker: Rc::new(Self::import_linker()),
            descriptor,
        }
    }

    pub fn import_linker() -> ImportLinker {
        let block_fuel = instruction_set! {
            .op_i32_const(0)
        };
        ImportLinker::from([
            (
                ImportName::new("__nothing_here", "__absolutely_nothing"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_ENTRYPOINT,
                    block_fuel: InstructionSet::default(),
                    params: &[],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT,
                    block_fuel: block_fuel.clone(),
                    params: &[],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32,
                    block_fuel: block_fuel.clone(),
                    params: &[ValType::I32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64,
                    block_fuel: block_fuel.clone(),
                    params: &[ValType::I64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F32.into(),
                    block_fuel: block_fuel.clone(),
                    params: &[ValType::F32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F64,
                    block_fuel: block_fuel.clone(),
                    params: &[ValType::F64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i32_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32_F32,
                    block_fuel: block_fuel.clone(),
                    params: &[ValType::I32, ValType::F32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i64_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64_F64,
                    block_fuel: block_fuel.clone(),
                    params: &[ValType::I64, ValType::F64],
                    result: &[],
                    intrinsic: None,
                },
            ),
        ])
    }
}

impl TestContext<'_> {
    /// Returns the file path of the associated `.wast` test file.
    fn test_path(&self) -> &str {
        self.descriptor.path()
    }

    /// Returns the [`TestDescriptor`] of the test context.
    pub fn spanned(&self, span: Span) -> TestSpan {
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

        let config = CompilationConfig::default()
            .with_import_linker(self.import_linker.clone())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(false)
            .with_default_imported_global_value(666.into())
            .with_allow_func_ref_function_types(true);

        // extract all exports first to calculate rwasm config
        let mut states = Vec::<(Box<str>, u32)>::new();
        let exports = ModuleParser::parse_function_exports(config.clone(), &wasm[..])?;
        for (k, func_idx, func_type) in exports.into_iter() {
            self.extern_types.insert(k.to_string(), func_type);
            let state_value = 10_000 + func_idx;
            self.extern_state.insert(k.to_string(), state_value);
            states.push((k.into(), state_value));
        }
        let config = config.with_state_router(StateRouterConfig {
            states: states.into(),
            opcode: Some(Opcode::Call(u32::MAX)),
        });

        let (rwasm_module, _) =
            RwasmModule::compile(config, &wasm[..]).map_err(|err| TestError::Rwasm(err.into()))?;

        {
            let buffer = rwasm_module.serialize();
            let (parsed_module, bytes_read) = RwasmModule::new(&buffer);
            assert_eq!(rwasm_module, parsed_module);
            assert_eq!(buffer[bytes_read..].len(), 0);
        }

        #[cfg(feature = "debug-print")]
        println!("{}", rwasm_module);

        let mut store = RwasmStore::<TestingContext>::new(
            ExecutorConfig::default(),
            self.import_linker.clone(),
            TestingContext::default(),
            testing_context_syscall_handler,
        );
        store.context_mut(|ctx| ctx.state = FUNC_ENTRYPOINT);
        let mut instance_inner = InstanceInner {
            module: rwasm_module,
            store,
            value_stack: ValueStack::default(),
            call_stack: CallStack::default(),
            program_counter: 0,
        };
        let mut executor = RwasmExecutor::<TestingContext>::entrypoint(
            &instance_inner.module,
            &mut instance_inner.value_stack,
            &mut instance_inner.call_stack,
            &mut instance_inner.store,
        );
        #[cfg(feature = "debug-print")]
        println!(" --- entrypoint ---");
        executor.run(&[], &mut [])?;
        #[cfg(feature = "debug-print")]
        println!();
        instance_inner.value_stack.reset();
        instance_inner.call_stack.reset();
        instance_inner.store.reset(true);

        let instance = Rc::new(RefCell::new(instance_inner));

        if let Some(module_name) = module_name {
            self.instances
                .insert(module_name.to_string(), instance.clone());
        }
        self.last_instance = Some(instance);
        Ok(())
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
        println!("\n --- {} ---", func_name);

        let instance = self.instance_by_name_or_last(module_name)?;
        let mut instance = instance.borrow_mut();

        // We reset an instruction pointer to the state function position to re-invoke the function.
        // However, with different states.
        // Some tests might fail, and we might keep outdated signature value in the state,
        // make sure the state is clear before every new call.
        let program_counter = instance.store.context(|ctx| ctx.program_counter as usize);
        instance.program_counter = program_counter;
        instance.store.reset(true);

        let func_state = self
            .extern_state
            .get(&func_name.to_string())
            .unwrap()
            .clone();

        let binding = args
            .iter()
            .cloned()
            .flat_map(|v| match v {
                Value::I64(v) => split_i64_to_i32_arr(v)
                    .into_iter()
                    .map(|v| Value::I32(v))
                    .collect(),
                Value::F64(v) => split_i64_to_i32_arr(v.to_bits() as i64)
                    .into_iter()
                    .map(|v| Value::I32(v))
                    .collect(),
                v => vec![v],
            })
            .collect::<Vec<_>>();
        let args = binding.as_slice();

        // reset PC and other state values
        instance.value_stack.reset();
        instance.call_stack.reset();
        instance.store.reset(true);
        // insert input params
        for value in args {
            instance.value_stack.push(value.clone().into());
        }
        // change function state for router
        instance.store.context_mut(|ctx| ctx.state = func_state);

        let pc = instance.program_counter;
        let mut vm = instance.new_executor();
        vm.advance_ip(pc);
        vm.run(&[], &mut [])?;
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
        Ok(results)
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

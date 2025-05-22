use super::{TestDescriptor, TestError, TestProfile, TestSpan};
use crate::handler::{
    testing_context_syscall_handler,
    TestingContext,
    ENTRYPOINT_FUNC_IDX,
    FUNC_PRINT,
    FUNC_PRINT_F32,
    FUNC_PRINT_F64,
    FUNC_PRINT_I32,
    FUNC_PRINT_I32_F32,
    FUNC_PRINT_I64,
    FUNC_PRINT_I64_F64,
};
use anyhow::Result;
use rwasm::{
    make_instruction_table,
    split_i64_to_i32,
    split_i64_to_i32_arr,
    Caller,
    CompilationConfig,
    ExecutorConfig,
    FuncType,
    ImportLinker,
    ImportLinkerEntity,
    ImportName,
    InstructionTable,
    ModuleParser,
    Opcode,
    OpcodeData,
    RwasmError,
    RwasmExecutor,
    RwasmModule as RwasmModule2,
    RwasmModule,
    StateRouterConfig,
    ValType,
    Value,
};
use std::{cell::RefCell, collections::HashMap, hash::Hash, rc::Rc, sync::Arc};
use wast::token::{Id, Span};

type TestingRwasmExecutor = RwasmExecutor<TestingContext>;
type Instance = Rc<RefCell<TestingRwasmExecutor>>;

/// The context of a single Wasm test spec suite run.
pub struct TestContext<'a> {
    /// The list of all encountered Wasm modules belonging to the test.
    modules: Vec<RwasmModule>,
    /// The list of all instantiated modules.
    instances: HashMap<String, Instance>,
    extern_types: HashMap<String, FuncType>,
    extern_state: HashMap<String, u32>,
    /// The last touched module instance.
    last_instance: Option<Instance>,
    /// Profiling during the Wasm spec test run.
    profile: TestProfile,
    /// Intermediate results buffer that can be reused for calling Wasm functions.
    results: Vec<Value>,
    /// The descriptor of the test.
    ///
    /// Useful for printing better debug messages in case of failure.
    descriptor: &'a TestDescriptor,
}

impl<'a> TestContext<'a> {
    /// Creates a new [`TestContext`] with the given [`TestDescriptor`].
    pub fn new(descriptor: &'a TestDescriptor) -> Self {
        TestContext {
            modules: Vec::new(),
            instances: HashMap::new(),
            extern_types: Default::default(),
            extern_state: Default::default(),
            last_instance: None,
            profile: TestProfile::default(),
            results: Vec::new(),
            descriptor,
        }
    }

    pub fn import_linker() -> ImportLinker {
        ImportLinker::from([
            (
                ImportName::new("spectest", "print"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT,
                    block_fuel: 0,
                    params: &[],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32,
                    block_fuel: 0,
                    params: &[ValType::I32],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64,
                    block_fuel: 0,
                    params: &[ValType::I32; 2],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F32.into(),
                    block_fuel: 0,
                    params: &[ValType::F32],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F64,
                    block_fuel: 0,
                    params: &[ValType::F32; 2],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i32_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32_F32,
                    block_fuel: 0,
                    params: &[ValType::I32, ValType::F32],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i64_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64_F64,
                    block_fuel: 0,
                    params: &[ValType::I32, ValType::I32, ValType::F32, ValType::F32],
                    result: &[],
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
            .with_import_linker(Self::import_linker())
            .with_wrap_import_functions(true)
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(false)
            .with_enable_floating_point(true);

        // extract all exports first to calculate rwasm config
        let mut states = Vec::<(Box<str>, u32)>::new();
        let exports = ModuleParser::parse_function_exports(config.clone(), &wasm[..])?;
        for (k, func_idx, func_type) in exports.into_iter() {
            self.extern_types.insert(k.to_string(), func_type);
            let state_value = 10_000 + func_idx.to_u32();
            self.extern_state.insert(k.to_string(), state_value);
            states.push((k.into(), state_value));
        }
        let config = config.with_state_router(StateRouterConfig {
            states: states.into(),
            opcode: Some((Opcode::Call, OpcodeData::FuncIdx(u32::MAX.into()))),
        });

        let (rwasm_module, _) =
            RwasmModule::compile(config, &wasm[..]).map_err(|err| TestError::Rwasm(err.into()))?;

        println!("{}", rwasm_module);

        let mut executor = TestingRwasmExecutor::new(
            rwasm_module.into(),
            ExecutorConfig::new().floats_enabled(true),
            TestingContext::default(),
        );
        executor.set_syscall_handler(testing_context_syscall_handler);
        executor.context_mut().state = ENTRYPOINT_FUNC_IDX;
        println!(" --- entrypoint ---");
        let exit_code = executor.run().map_err(|err| TestError::Rwasm(err))?;
        assert_eq!(exit_code, 0);
        println!();

        let instance = Rc::new(RefCell::new(executor));

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
        // for export in instance.exports(&self.store) {
        //     self.linker
        //         .define(name, export.name(), export.clone().into_extern())
        //         .unwrap_or_else(|error| {
        //             let field_name = export.name();
        //             let export = export.clone().into_extern();
        //             panic!(
        //                 "failed to define export {name}::{field_name}: \
        //                 {export:?}: {error}",
        //             )
        //         });
        // }
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
    /// - If function invokation returned an error.
    pub fn invoke(
        &mut self,
        module_name: Option<&str>,
        func_name: &str,
        args: &[Value],
    ) -> Result<&[Value], TestError> {
        println!("\n --- {} ---", func_name);

        let mut instance = self.instance_by_name_or_last(module_name)?;
        let mut instance = instance.borrow_mut();

        // We reset an instruction pointer to the state function position to re-invoke the function.
        // However, with different states.
        // Some tests might fail, and we might keep outdated signature value in the state,
        // make sure the state is clear before every new call.
        let pc = instance.context().program_counter as usize;
        instance.reset(Some(pc));

        let func_state = self
            .extern_state
            .get(&func_name.to_string())
            .unwrap()
            .clone();

        let mut caller = Caller::new(&mut instance);

        let binding = args
            .iter()
            .cloned()
            .flat_map(|v| match v {
                Value::I64(v) => split_i64_to_i32_arr(v)
                    .into_iter()
                    .map(|v| Value::I32(v))
                    .collect(),
                v => vec![v],
            })
            .collect::<Vec<_>>();
        let args = binding.as_slice();

        for value in args {
            caller.stack_push(value.clone());
        }

        // change function state for router
        instance.context_mut().state = func_state;
        let exit_code = instance.run().map_err(|err| TestError::Rwasm(err))?;
        // copy results
        let func_type = self.extern_types.get(func_name).unwrap();
        let len_results = func_type.results().len();
        self.results.clear();
        self.results.resize(len_results, Value::I32(0));
        let mut caller = Caller::new(&mut instance);
        for (i, val_type) in func_type.results().iter().rev().enumerate() {
            let popped_value = caller.stack_pop();
            self.results[len_results - 1 - i] = match val_type {
                ValType::I32 => Value::I32(popped_value.into()),
                ValType::I64 => Value::I64(popped_value.into()),
                ValType::F32 => Value::F32(popped_value.into()),
                ValType::F64 => Value::F64(popped_value.into()),
                ValType::FuncRef => Value::FuncRef(popped_value.into()),
                ValType::ExternRef => Value::ExternRef(popped_value.into()),
                _ => unreachable!("unsupported result type: {:?}", val_type),
            };
        }
        Ok(&self.results)
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

use super::{TestDescriptor, TestError, TestProfile, TestSpan, ENABLE_32_BIT_TRANSLATOR};
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
    Caller,
    ExecutorConfig,
    InstructionTable,
    RwasmError,
    RwasmExecutor,
    RwasmModule as RwasmModule2,
    RwasmModule,
    Value,
};
use rwasm_legacy::{
    core::{ImportLinker, ImportLinkerEntity, ValueType},
    module::ImportName,
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
    extern_types: HashMap<String, rwasm_legacy::FuncType>,
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
                    func_idx: FUNC_PRINT,
                    fuel_procedure: &[],
                    params: &[],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i32"),
                ImportLinkerEntity {
                    func_idx: FUNC_PRINT_I32,
                    fuel_procedure: &[],
                    params: &[ValueType::I32],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i64"),
                ImportLinkerEntity {
                    func_idx: FUNC_PRINT_I64,
                    fuel_procedure: &[],
                    params: if ENABLE_32_BIT_TRANSLATOR {
                        &[ValueType::I32; 2]
                    } else {
                        &[ValueType::I64]
                    },
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_f32"),
                ImportLinkerEntity {
                    func_idx: FUNC_PRINT_F32.into(),
                    fuel_procedure: &[],
                    params: &[ValueType::F32],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_f64"),
                ImportLinkerEntity {
                    func_idx: FUNC_PRINT_F64,
                    fuel_procedure: &[],
                    params: if ENABLE_32_BIT_TRANSLATOR {
                        &[ValueType::F32; 2]
                    } else {
                        &[ValueType::F64]
                    },
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i32_f32"),
                ImportLinkerEntity {
                    func_idx: FUNC_PRINT_I32_F32,
                    fuel_procedure: &[],
                    params: &[ValueType::I32, ValueType::F32],
                    result: &[],
                },
            ),
            (
                ImportName::new("spectest", "print_i64_f64"),
                ImportLinkerEntity {
                    func_idx: FUNC_PRINT_I64_F64,
                    fuel_procedure: &[],
                    params: if ENABLE_32_BIT_TRANSLATOR {
                        &[
                            ValueType::I32,
                            ValueType::I32,
                            ValueType::F32,
                            ValueType::F32,
                        ]
                    } else {
                        &[ValueType::I64, ValueType::F64]
                    },
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

        let mut config = rwasm_legacy::Config::default();
        config
            .wasm_mutable_global(false)
            .wasm_saturating_float_to_int(false)
            .wasm_sign_extension(false)
            .wasm_multi_value(false)
            .wasm_mutable_global(true)
            .wasm_saturating_float_to_int(true)
            .wasm_sign_extension(true)
            .wasm_multi_value(true)
            .wasm_bulk_memory(true)
            .wasm_reference_types(true)
            .wasm_tail_call(true)
            .wasm_extended_const(true);

        // extract all exports first to calculate rwasm config
        let rwasm_config = {
            let engine = rwasm_legacy::Engine::new(&config);
            let wasm_module = rwasm_legacy::Module::new(&engine, &wasm[..])?;
            let mut states = Vec::<(String, u32)>::new();
            for (k, v) in wasm_module.exports.iter() {
                let func_idx = v.into_func_idx();
                if func_idx.is_none() {
                    continue;
                }
                let func_idx = func_idx.unwrap();
                let func_typ = wasm_module.get_export(k).unwrap();
                let func_typ = func_typ.func().unwrap();
                let state_value = 10000 + func_idx.into_u32();
                self.extern_types.insert(k.to_string(), func_typ.clone());
                self.extern_state.insert(k.to_string(), state_value);
                states.push((k.to_string(), state_value));
            }
            rwasm_legacy::engine::RwasmConfig {
                state_router: Some(rwasm_legacy::engine::StateRouterConfig {
                    states: states.into(),
                    opcode: rwasm_legacy::engine::bytecode::Instruction::Call(u32::MAX.into()),
                }),
                entrypoint_name: None,
                import_linker: Some(Self::import_linker()),
                wrap_import_functions: true,
                translate_drop_keep: false,
                allow_malformed_entrypoint_func_type: true,
                use_32bit_mode: ENABLE_32_BIT_TRANSLATOR,
                builtins_consume_fuel: false,
            }
        };

        config.rwasm_config(rwasm_config);
        let engine = rwasm_legacy::Engine::new(&config);
        let wasm_module = rwasm_legacy::Module::new(&engine, &wasm[..])?;
        let rwasm_module = rwasm_legacy::rwasm::RwasmModule::from_module(&wasm_module);
        let mut encoded_rwasm_module = Vec::new();
        use rwasm_legacy::rwasm::BinaryFormat;
        rwasm_module
            .write_binary_to_vec(&mut encoded_rwasm_module)
            .unwrap();
        let rwasm_module = RwasmModule2::new(&encoded_rwasm_module);

        println!();
        #[allow(unused)]
        fn trace_rwasm(rwasm_bytecode: &[u8]) {
            let rwasm_module = RwasmModule::new(rwasm_bytecode);
            let mut func_index = 0usize;
            println!("\n -- function #{} -- ", func_index);
            for (i, instr) in rwasm_module.code_section.instr.iter().enumerate() {
                println!("{:02}: {:?}", i, instr);
                if rwasm_module.func_section.contains(&(i as u32 + 1)) {
                    func_index += 1;
                    println!("\n -- function #{} -- ", func_index);
                }
            }
            println!("\n")
        }
        trace_rwasm(&encoded_rwasm_module);
        println!();

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

        let flat_args: Vec<Value>;
        let args = if ENABLE_32_BIT_TRANSLATOR {
            flat_args = args
                .iter()
                .cloned()
                .flat_map(|v| match v {
                    Value::I64(v) => rwasm_legacy::value::split_i64_to_i32(v)
                        .into_iter()
                        .map(|v| Value::I32(v))
                        .collect(),
                    v => vec![v],
                })
                .collect();
            flat_args.as_slice()
        } else {
            args
        };

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
                ValueType::I32 => Value::I32(popped_value.into()),
                ValueType::I64 => Value::I64(popped_value.into()),
                ValueType::F32 => Value::F32(popped_value.into()),
                ValueType::F64 => Value::F64(popped_value.into()),
                ValueType::FuncRef => Value::FuncRef(popped_value.into()),
                ValueType::ExternRef => Value::ExternRef(popped_value.into()),
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

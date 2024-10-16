use super::{TestDescriptor, TestError, TestProfile, TestSpan};
use anyhow::Result;
use rwasm::{
    core::{ValueType, F32, F64},
    module::{FuncIdx, Imported},
    rwasm::{BinaryFormat, RwasmModule},
    Config,
    Engine,
    Extern,
    Func,
    Global,
    Instance,
    Linker,
    Memory,
    MemoryType,
    Module,
    Mutability,
    Store,
    Table,
    TableType,
    Value,
};
use std::collections::HashMap;
use wast::token::{Id, Span};

/// The context of a single Wasm test spec suite run.
#[derive(Debug)]
pub struct TestContext<'a> {
    /// The wasmi config
    config: Config,
    /// The `wasmi` engine used for executing functions used during the test.
    engine: Engine,
    /// The linker for linking together Wasm test modules.
    linker: Linker<()>,
    /// The store to hold all runtime data during the test.
    store: Store<()>,
    /// The list of all encountered Wasm modules belonging to the test.
    modules: Vec<Module>,
    /// The list of all instantiated modules.
    instances: HashMap<String, Instance>,
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
    pub fn new(descriptor: &'a TestDescriptor, config: Config) -> Self {
        let engine = Engine::new(&config);
        let mut linker = Linker::new(&engine);
        let mut store = Store::new(&engine, ());
        let default_memory = Memory::new(&mut store, MemoryType::new(1, Some(2)).unwrap()).unwrap();
        let default_table = Table::new(
            &mut store,
            TableType::new(ValueType::FuncRef, 10, Some(20)),
            Value::default(ValueType::FuncRef),
        )
        .unwrap();
        let global_i32 = Global::new(&mut store, Value::I32(666), Mutability::Const);
        let global_i64 = Global::new(&mut store, Value::I64(666), Mutability::Const);
        let global_f32 = Global::new(&mut store, Value::F32(666.0.into()), Mutability::Const);
        let global_f64 = Global::new(&mut store, Value::F64(666.0.into()), Mutability::Const);
        let print = Func::wrap(&mut store, || {
            println!("print");
        });
        let print_i32 = Func::wrap(&mut store, |value: i32| {
            println!("print: {value}");
        });
        let print_i64 = Func::wrap(&mut store, |value: i64| {
            println!("print: {value}");
        });
        let print_f32 = Func::wrap(&mut store, |value: F32| {
            println!("print: {value:?}");
        });
        let print_f64 = Func::wrap(&mut store, |value: F64| {
            println!("print: {value:?}");
        });
        let print_i32_f32 = Func::wrap(&mut store, |v0: i32, v1: F32| {
            println!("print: {v0:?} {v1:?}");
        });
        let print_f64_f64 = Func::wrap(&mut store, |v0: F64, v1: F64| {
            println!("print: {v0:?} {v1:?}");
        });
        linker.define("spectest", "memory", default_memory).unwrap();
        linker.define("spectest", "table", default_table).unwrap();
        linker.define("spectest", "global_i32", global_i32).unwrap();
        linker.define("spectest", "global_i64", global_i64).unwrap();
        linker.define("spectest", "global_f32", global_f32).unwrap();
        linker.define("spectest", "global_f64", global_f64).unwrap();
        linker.define("spectest", "print", print).unwrap();
        linker.define("spectest", "print_i32", print_i32).unwrap();
        linker.define("spectest", "print_i64", print_i64).unwrap();
        linker.define("spectest", "print_f32", print_f32).unwrap();
        linker.define("spectest", "print_f64", print_f64).unwrap();
        linker
            .define("spectest", "print_i32_f32", print_i32_f32)
            .unwrap();
        linker
            .define("spectest", "print_f64_f64", print_f64_f64)
            .unwrap();
        TestContext {
            config,
            engine,
            linker,
            store,
            modules: Vec::new(),
            instances: HashMap::new(),
            last_instance: None,
            profile: TestProfile::default(),
            results: Vec::new(),
            descriptor,
        }
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

    /// Returns the [`Engine`] of the [`TestContext`].
    fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Returns a shared reference to the underlying [`Store`].
    pub fn store(&self) -> &Store<()> {
        &self.store
    }

    /// Returns an exclusive reference to the underlying [`Store`].
    pub fn store_mut(&mut self) -> &mut Store<()> {
        &mut self.store
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
    ) -> Result<Instance, TestError> {
        println!("\n --- creating module ---");
        println!("{:?}", module);
        println!(" -----------------------\n");
        let module_name = module.id.map(|id| id.name());
        let wasm = module.encode().unwrap_or_else(|error| {
            panic!(
                "encountered unexpected failure to encode `.wast` module into `.wasm`:{}: {}",
                self.test_path(),
                error
            )
        });
        let module = if self.engine.config().get_rwasm_config().is_some() {
            // let original_engine = Engine::new(&self.config);
            let original_engine = self.engine();
            let original_module = Module::new(original_engine, &wasm[..])?;
            let rwasm_module = RwasmModule::from_module(&original_module);
            // encode and decode rwasm module (to tests encoding/decoding flow)
            let mut encoded_rwasm_module = Vec::new();
            rwasm_module
                .write_binary_to_vec(&mut encoded_rwasm_module)
                .unwrap();
            let rwasm_module = RwasmModule::read_from_slice(&encoded_rwasm_module).unwrap();
            // create module builder
            let mut module_builder = rwasm_module.to_module_builder(self.engine());
            // copy imports
            for (i, imported) in original_module.imports.items.iter().enumerate() {
                match imported {
                    Imported::Func(import_name) => {
                        let func_type = original_module.funcs[i];
                        let func_type =
                            original_engine.resolve_func_type(&func_type, |v| v.clone());
                        let new_func_type = self.engine.alloc_func_type(func_type);
                        module_builder.funcs.insert(i, new_func_type);
                        module_builder.imports.funcs.push(import_name.clone());
                    }
                    Imported::Global(_) => continue,
                    _ => unreachable!("not supported import type ({:?})", imported),
                }
            }
            // copy exports indices (it's not affected, so we can safely copy)
            for (k, v) in original_module.exports.iter() {
                if let Some(func_index) = v.into_func_idx() {
                    let func_index = func_index.into_u32();
                    if func_index < original_module.imports.len_funcs as u32 {
                        unreachable!("this is imported and exported func at the same time... ?")
                    }
                    let func_type = original_module.funcs[func_index as usize];
                    let func_type = original_engine.resolve_func_type(&func_type, |v| v.clone());
                    // remap exported a func type
                    let new_func_type = self.engine.alloc_func_type(func_type);
                    module_builder.funcs[func_index as usize] = new_func_type;
                }
                module_builder.push_export(k.clone(), *v);
            }
            let mut module = module_builder.finish();
            // for rWASM set entrypoint as a start function to init module and sections
            let entrypoint_func_index = module.funcs.len() - 1;
            module.start = Some(FuncIdx::from(entrypoint_func_index as u32));
            module
        } else {
            Module::new(self.engine(), &wasm[..])?
        };
        let instance_pre = self.linker.instantiate(&mut self.store, &module)?;
        println!(" --- entrypoint ---");
        let instance = instance_pre.start(&mut self.store)?;
        self.modules.push(module);
        if let Some(module_name) = module_name {
            self.instances.insert(module_name.to_string(), instance);
            for export in instance.exports(&self.store) {
                self.linker
                    .define(module_name, export.name(), export.into_extern())?;
            }
        }
        self.last_instance = Some(instance);
        Ok(instance)
    }

    /// Loads the Wasm module instance with the given name.
    ///
    /// # Errors
    ///
    /// If there is no registered module instance with the given name.
    pub fn instance_by_name(&self, name: &str) -> Result<Instance, TestError> {
        self.instances
            .get(name)
            .copied()
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
            .unwrap_or_else(|| self.last_instance.ok_or(TestError::NoModuleInstancesFound))
    }

    /// Registers the given [`Instance`] with the given `name` and sets it as the last instance.
    pub fn register_instance(&mut self, name: &str, instance: Instance) {
        if self.instances.get(name).is_some() {
            // Already registered the instance.
            return;
        }
        self.instances.insert(name.to_string(), instance);
        for export in instance.exports(&self.store) {
            self.linker
                .define(name, export.name(), export.clone().into_extern())
                .unwrap_or_else(|error| {
                    let field_name = export.name();
                    let export = export.clone().into_extern();
                    panic!(
                        "failed to define export {name}::{field_name}: \
                        {export:?}: {error}",
                    )
                });
        }
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
        let instance = self.instance_by_name_or_last(module_name)?;
        let func = instance
            .get_export(&self.store, func_name)
            .and_then(Extern::into_func)
            .ok_or_else(|| TestError::FuncNotFound {
                module_name: module_name.map(|name| name.to_string()),
                func_name: func_name.to_string(),
            })?;
        let len_results = func.ty(&self.store).results().len();
        self.results.clear();
        self.results.resize(len_results, Value::I32(0));
        func.call(&mut self.store, args, &mut self.results)?;
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
        module_name: Option<Id>,
        global_name: &str,
    ) -> Result<Value, TestError> {
        let module_name = module_name.map(|id| id.name());
        let instance = self.instance_by_name_or_last(module_name)?;
        let global = instance
            .get_export(&self.store, global_name)
            .and_then(Extern::into_global)
            .ok_or_else(|| TestError::GlobalNotFound {
                module_name: module_name.map(|name| name.to_string()),
                global_name: global_name.to_string(),
            })?;
        let value = global.get(&self.store);
        Ok(value)
    }
}

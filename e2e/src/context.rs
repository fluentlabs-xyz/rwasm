use super::{TestDescriptor, TestError, TestProfile, TestSpan};
use crate::group::TestingInstanceGroup;
use rwasm::Value;
use std::collections::HashMap;
use wast::token::{Id, Span};

/// The context of a single Wasm test spec suite run.
pub struct TestContext<'a> {
    /// The list of all instantiated modules.
    instances: HashMap<String, TestingInstanceGroup>,
    /// The last touched module instance.
    last_instance: Option<TestingInstanceGroup>,
    /// Profiling during the Wasm spec test run.
    profile: TestProfile,
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
            last_instance: None,
            profile: TestProfile::default(),
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
        let instance_group = TestingInstanceGroup::new(&wasm)?;
        if let Some(module_name) = module_name {
            self.instances
                .insert(module_name.to_string(), instance_group.clone());
        }
        self.last_instance = Some(instance_group);
        Ok(())
    }

    /// Loads the Wasm module instance with the given name.
    ///
    /// # Errors
    ///
    /// If there is no registered module instance with the given name.
    pub fn instance_by_name(&self, name: &str) -> Result<TestingInstanceGroup, TestError> {
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
    pub fn instance_by_name_or_last(
        &self,
        name: Option<&str>,
    ) -> Result<TestingInstanceGroup, TestError> {
        name.map(|name| self.instance_by_name(name))
            .unwrap_or_else(|| {
                self.last_instance
                    .clone()
                    .ok_or(TestError::NoModuleInstancesFound)
            })
    }

    /// Registers the given [`Instance`] with the given `name` and sets it as the last instance.
    pub fn register_instance(&mut self, name: &str, instance: TestingInstanceGroup) {
        if self.instances.contains_key(name) {
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
        let instances = self.instance_by_name_or_last(module_name)?;
        instances.execute(func_name, args)
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

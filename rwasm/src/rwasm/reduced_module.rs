use crate::{
    core::ImportLinker,
    engine::{DropKeep, RwasmConfig},
    instruction_set,
    module::{FuncIdx, FuncTypeIdx, MemoryIdx, ModuleBuilder, ModuleError},
    rwasm::{
        BinaryFormat,
        BinaryFormatError,
        InstructionSet,
        N_MAX_MEMORY_PAGES,
        N_MAX_RECURSION_DEPTH,
        N_MAX_STACK_HEIGHT,
    },
    Config,
    Engine,
    Error,
    FuelConsumptionMode,
    Module,
    StackLimits,
};
use alloc::{string::ToString, vec::Vec};

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RwasmModuleInstance {
    pub module: RwasmModule,
    pub func_segments: Vec<u32>,
    pub start: usize,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RwasmModule {
    pub code_section: InstructionSet,
    pub memory_section: Vec<u8>,
    pub func_section: Vec<u32>,
    pub element_section: Vec<u32>,
}

impl From<InstructionSet> for RwasmModule {
    fn from(value: InstructionSet) -> Self {
        let code_section_len = value.len() as u32;
        Self {
            code_section: value,
            memory_section: vec![],
            func_section: vec![code_section_len],
            element_section: vec![],
        }
    }
}

impl RwasmModule {
    pub fn default_config(import_linker: Option<ImportLinker>) -> Config {
        let mut config = Config::default();
        config.set_stack_limits(
            StackLimits::new(
                N_MAX_STACK_HEIGHT,
                N_MAX_STACK_HEIGHT,
                N_MAX_RECURSION_DEPTH,
            )
            .unwrap(),
        );
        config.consume_fuel(true);
        config.fuel_consumption_mode(FuelConsumptionMode::Eager);
        config.rwasm_config(RwasmConfig {
            import_linker,
            wrap_import_functions: true,
            ..Default::default()
        });
        config
    }

    pub fn compile(wasm_binary: &[u8], import_linker: Option<ImportLinker>) -> Result<Self, Error> {
        let default_config = Self::default_config(import_linker);
        Self::compile_with_config(wasm_binary, &default_config)
    }

    pub fn compile_with_end_offset(
        wasm_binary: &[u8],
        config: &Config,
    ) -> Result<(Self, usize), Error> {
        assert!(
            config.get_rwasm_config().is_some(),
            "rWASM mode must be enabled in config"
        );
        let engine = Engine::new(&config);
        let (module, end_offset) = Module::parse_with_end_offset(&engine, &wasm_binary)?;
        Ok((Self::from_module(&module), end_offset))
    }

    pub fn compile_with_config(wasm_binary: &[u8], config: &Config) -> Result<Self, Error> {
        let (module, _) = Self::compile_with_end_offset(wasm_binary, config)?;
        Ok(module)
    }

    pub fn new(module: &[u8]) -> Result<Self, BinaryFormatError> {
        Self::read_from_slice(module)
    }

    pub fn new_or_empty(module: &[u8]) -> Result<Self, BinaryFormatError> {
        if module.is_empty() {
            return Ok(Self::empty());
        }
        Self::new(module)
    }

    pub fn empty() -> Self {
        let instruction_set = instruction_set! {
            Return(DropKeep::none())
        };
        Self::from(instruction_set)
    }

    pub fn instantiate(self) -> RwasmModuleInstance {
        let mut func_segments = vec![0u32];
        let mut total_func_len = 0u32;
        for func_len in self.func_section.iter().take(self.func_section.len() - 1) {
            total_func_len += *func_len;
            func_segments.push(total_func_len);
        }
        let source_pc = func_segments
            .last()
            .copied()
            .expect("rwasm: empty function section");
        RwasmModuleInstance {
            module: self,
            start: source_pc as usize,
            func_segments,
        }
    }

    pub fn from_module(module: &Module) -> Self {
        // build code & func sections
        let mut code_section = InstructionSet::new();
        let mut func_section = Vec::new();
        for compiled_func in module.compiled_funcs.iter() {
            let (mut instr_begin, instr_end) = module.engine().instr_ptr(*compiled_func);
            let length_before = code_section.len();
            while instr_begin != instr_end {
                code_section.push(*instr_begin.get());
                instr_begin.add(1);
            }
            let function_length = code_section.len() - length_before;
            func_section.push(function_length as u32);
        }
        // build element section
        let element_section = module.element_segments[0]
            .items
            .items()
            .iter()
            .map(|v| {
                if let Some(value) = v.eval_const() {
                    return value.as_u32();
                }
                v.funcref()
                    .expect("not supported element segment type")
                    .into_u32()
            })
            .collect::<Vec<_>>();
        // build memory section
        let memory_section = Vec::from(&*module.data_segments[0].bytes);
        Self {
            code_section,
            memory_section,
            func_section,
            element_section,
        }
    }

    pub fn bytecode(&self) -> &InstructionSet {
        &self.code_section
    }

    pub fn to_module(&self, engine: &Engine) -> Module {
        let builder = self.to_module_builder(engine);
        builder.finish()
    }

    pub fn to_module_builder<'a>(&'a self, engine: &'a Engine) -> ModuleBuilder {
        let mut builder = ModuleBuilder::new(engine);
        // the main function has empty inputs and outputs
        let empty_func_type = builder.ensure_empty_func_type_exists();
        // push functions and init engine's code map
        builder
            .push_funcs(
                (0..self.func_section.len())
                    .map(|_| Result::<FuncTypeIdx, ModuleError>::Ok(empty_func_type)),
            )
            .expect("failed to push functions");
        let mut func_offset = 0usize;
        for (func_index, func_length) in self.func_section.iter().enumerate() {
            let compiled_func = builder.compiled_funcs[func_index];
            let func_length = *func_length as usize;
            let instr = self.code_section.instr[func_offset..(func_offset + func_length)]
                .into_iter()
                .copied();
            let metas = self.code_section.metas[func_offset..(func_offset + func_length)]
                .into_iter()
                .copied();
            engine.init_func(compiled_func, 0, 0, instr, metas);
            func_offset += func_length;
        }
        // push memory/data and table/elem segments
        builder.push_default_memory(0, Some(N_MAX_MEMORY_PAGES));
        builder.push_rwasm_data_segment(&self.memory_section);
        builder.push_rwasm_tables();
        builder.push_rwasm_elem_segment(&self.element_section);
        builder.push_rwasm_globals();
        // memory and entrypoint must be exported
        builder.push_export("memory".to_string().into_boxed_str(), MemoryIdx::from(0));
        let entrypoint_index = builder.compiled_funcs.len() as u32 - 1;
        builder.push_export(
            "main".to_string().into_boxed_str(),
            FuncIdx::from(entrypoint_index),
        );
        // finalize module
        builder
    }
}

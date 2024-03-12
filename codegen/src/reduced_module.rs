use crate::{
    binary_format::BinaryFormat,
    constants::N_MAX_MEMORY_PAGES,
    instruction_set::InstructionSet,
    BinaryFormatError,
};
use alloc::{collections::BTreeMap, string::ToString, vec::Vec};
use rwasm::{
    module::{FuncIdx, FuncTypeIdx, MemoryIdx, ModuleBuilder, ModuleError},
    Engine,
    FuncType,
    Module,
};

#[derive(Debug, Default, PartialEq)]
pub struct RwasmModule {
    pub(crate) code_section: InstructionSet,
    pub(crate) memory_section: Vec<u8>,
    pub(crate) func_section: Vec<u32>,
    pub(crate) element_section: Vec<u32>,
}

impl RwasmModule {
    pub fn new(module: &[u8]) -> Result<Self, BinaryFormatError> {
        Self::read_from_slice(module)
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
        // main function has empty inputs and outputs
        let mut default_func_types = BTreeMap::new();
        let mut get_func_type_or_create =
            |func_type: FuncType, builder: &mut ModuleBuilder| -> FuncTypeIdx {
                let func_type_idx = default_func_types.get(&func_type);
                let func_type_idx = if let Some(idx) = func_type_idx {
                    *idx
                } else {
                    let idx = default_func_types.len();
                    default_func_types.insert(func_type.clone(), idx);
                    builder.push_func_type(func_type).unwrap();
                    idx
                };
                FuncTypeIdx::from(func_type_idx as u32)
            };
        let empty_func_type = get_func_type_or_create(FuncType::new([], []), &mut builder);
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

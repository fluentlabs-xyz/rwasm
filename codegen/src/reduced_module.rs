use crate::{
    binary_format::BinaryFormat,
    constants::{N_MAX_MEMORY_PAGES, N_MAX_TABLES},
    instruction_set::InstructionSet,
    BinaryFormatError,
};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use rwasm::{
    core::ImportLinker,
    engine::bytecode::Instruction,
    module::{FuncIdx, FuncTypeIdx, MemoryIdx, ModuleBuilder, ModuleError, ModuleResources},
    Engine,
    FuncType,
    Module,
};

#[derive(Debug, Default, Clone, PartialEq)]
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

    pub fn bytecode(&self) -> &InstructionSet {
        &self.code_section
    }

    pub fn to_module(&self, engine: &Engine, import_linker: &ImportLinker) -> Module {
        let builder = self.to_module_builder(engine, import_linker, FuncType::new([], []));
        builder.finish_rwasm()
    }

    pub fn to_module_builder<'a>(
        &'a self,
        engine: &'a Engine,
        import_linker: &ImportLinker,
        func_type: FuncType,
    ) -> ModuleBuilder {
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
        get_func_type_or_create(func_type, &mut builder);

        let mut code_section = self.bytecode().clone();

        // find all used imports and map them
        // TODO: "we can optimize it by adding flag into WASMI VM and support such calls mapping"
        let mut import_mapping = BTreeMap::new();
        for instr in code_section.instr.iter_mut() {
            let host_index = match instr {
                Instruction::Call(func) => func.to_u32(),
                _ => continue,
            };
            let func_index = import_mapping
                .get(&host_index)
                .copied()
                .unwrap_or(import_mapping.len() as u32);
            instr.update_call_index(func_index);
            let import_func = import_linker
                .resolve_by_index(host_index)
                .ok_or_else(|| unreachable!("unknown host index: ({:?})", host_index))
                .unwrap();
            let func_type = import_func.func_type().clone();
            let func_type_idx = get_func_type_or_create(func_type, &mut builder);
            if !import_mapping.contains_key(&host_index) {
                import_mapping.insert(host_index, func_index);
                builder
                    .push_function_import(import_func.import_name().clone(), func_type_idx)
                    .unwrap();
            }
        }
        let import_len = import_mapping.len() as u32;

        // push main functions
        let total_functions = self.func_section.len() + 1;
        let builder_functions = (0..total_functions)
            .map(|_| Result::<FuncTypeIdx, ModuleError>::Ok(FuncTypeIdx::from(0)))
            .collect::<Vec<_>>();
        builder.push_funcs(builder_functions).unwrap();

        // mark headers for missing functions inside binary
        let resources = ModuleResources::new(&builder);
        let compiled_func = resources
            .get_compiled_func(FuncIdx::from(import_len))
            .unwrap();
        let mut instr = code_section.instr.clone();
        if instr.is_empty() {
            instr.push(Instruction::Unreachable);
        }
        let metas = code_section.metas.unwrap();
        engine.init_func(compiled_func, 0, 0, instr, metas);
        for (fn_index, fn_pos) in self.func_section.iter().copied().enumerate() {
            let compiled_func = resources
                .get_compiled_func(FuncIdx::from(import_len + fn_index as u32 + 1))
                .unwrap();
            engine.mark_func(compiled_func, 0, 0, fn_pos as usize);
        }

        // push segments
        builder.push_default_data_segment(&self.memory_section);
        builder.push_default_elem_segment(&self.element_section);

        // allocate default memory
        builder
            .push_default_memory(0, Some(N_MAX_MEMORY_PAGES))
            .unwrap();
        builder
            .push_export("memory".to_string().into_boxed_str(), MemoryIdx::from(0))
            .unwrap();
        // set 0 function as an entrypoint (it goes right after import section)
        let main_index = import_mapping.len() as u32;
        if cfg!(feature = "e2e") {
            builder.set_start(FuncIdx::from(main_index));
        }
        builder
            .push_export(
                "main".to_string().into_boxed_str(),
                FuncIdx::from(main_index),
            )
            .unwrap();
        // push required amount of globals and tables
        let num_globals = self.bytecode().count_globals();
        builder.push_empty_globals(num_globals as usize).unwrap();
        let num_tables = self.bytecode().count_tables();
        builder.push_empty_tables(num_tables, N_MAX_TABLES).unwrap();
        // finalize module
        builder
    }

    pub fn trace(&self) -> String {
        self.code_section.trace()
    }
}

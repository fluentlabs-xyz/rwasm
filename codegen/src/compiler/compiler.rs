use crate::{
    compiler::{
        config::CompilerConfig,
        types::{CompilerError, FuncOrExport, Injection},
    },
    constants::{N_MAX_RECURSION_DEPTH, N_MAX_STACK_HEIGHT},
    InstructionSet,
};
use alloc::vec::Vec;
use rwasm::{
    core::ImportLinker,
    engine::{
        bytecode::{Instruction, TableIdx},
        code_map::InstructionPtr,
        DropKeep,
    },
    module::Imported,
    Config,
    Engine,
    Module,
    StackLimits,
};

pub struct Compiler2<'linker> {
    // input params
    pub(crate) import_linker: Option<&'linker ImportLinker>,
    pub(crate) config: CompilerConfig,
    // parsed wasmi state
    engine: Engine,
    module: Module,
    // translation state
    pub(crate) code_section: InstructionSet,
    function_beginning: Vec<u32>,
    injection_segments: Vec<Injection>,
}

impl<'linker> Compiler2<'linker> {
    pub fn new(wasm_binary: &[u8], config: CompilerConfig) -> Result<Self, CompilerError> {
        Self::new_with_linker(wasm_binary, config, None)
    }

    pub fn new_with_linker(
        wasm_binary: &[u8],
        config: CompilerConfig,
        import_linker: Option<&'linker ImportLinker>,
    ) -> Result<Self, CompilerError> {
        let mut engine_config = Config::default();
        engine_config.set_stack_limits(
            StackLimits::new(
                N_MAX_STACK_HEIGHT,
                N_MAX_STACK_HEIGHT,
                N_MAX_RECURSION_DEPTH,
            )
            .unwrap(),
        );
        engine_config.wasm_bulk_memory(true);
        engine_config.wasm_tail_call(false);
        engine_config.wasm_extended_const(config.extended_const);
        engine_config.consume_fuel(config.fuel_consume);
        engine_config.rwasm_mode(true);
        let engine = Engine::new(&engine_config);
        let module =
            Module::new(&engine, wasm_binary).map_err(|e| CompilerError::ModuleError(e))?;
        Ok(Compiler2 {
            engine,
            module,
            code_section: InstructionSet::new(),
            function_beginning: Vec::new(),
            import_linker,
            injection_segments: vec![],
            config,
        })
    }

    #[cfg(feature = "std")]
    pub fn trace_bytecode(&self) {
        let import_len = self.module.imports.len_funcs;
        for fn_index in 0..self.module.funcs.len() {
            if fn_index != 0 && fn_index - 1 < import_len {
                println!("# imported func {}", fn_index);
            } else {
                println!("# func {}", fn_index);
            }
            let func_body = self.module.compiled_funcs.get(fn_index).unwrap();
            for instr in self.engine.instr_vec(*func_body) {
                println!("{:?}", instr);
            }
        }
        println!()
    }

    pub fn finalize(self) -> (Engine, Module) {
        (self.engine, self.module)
    }

    pub fn config(&self) -> &CompilerConfig {
        &self.config
    }

    fn translate_import_func(&mut self, import_fn_index: u32) -> Result<(), CompilerError> {
        let beginning_offset = self.code_section.len();
        let (import_index, fuel_cost, _, _) = self.resolve_host_call(import_fn_index)?;
        if self.engine.config().get_consume_fuel() {
            self.code_section.op_consume_fuel(fuel_cost);
        }
        self.code_section.op_call(import_index);
        self.code_section.op_return();
        assert_eq!(
            self.function_beginning.len(),
            import_fn_index as usize,
            "incorrect function order"
        );
        self.function_beginning.push(beginning_offset);
        Ok(())
    }

    fn translate_function(&mut self, fn_index: u32) -> Result<(), CompilerError> {
        let import_len = self.module.imports.len_funcs;
        // don't translate import functions because we can't translate them
        if fn_index < import_len as u32 {
            return Ok(());
        }
        let beginning_offset = self.code_section.len();
        let func_body = self
            .module
            .compiled_funcs
            .get(fn_index as usize - import_len)
            .ok_or(CompilerError::MissingFunction)?;
        // reserve stack for locals
        let len_locals = self.engine.num_locals(*func_body);
        (0..len_locals).for_each(|_| {
            self.code_section.op_i32_const(0);
        });
        // translate instructions
        let (mut instr_ptr, instr_end) = self.engine.instr_ptr(*func_body);
        while instr_ptr != instr_end {
            self.translate_opcode(&mut instr_ptr, 0)?;
        }
        // remember function offset in the mapping
        assert_eq!(
            self.function_beginning.len(),
            fn_index as usize,
            "incorrect function order"
        );
        self.function_beginning.push(beginning_offset);
        Ok(())
    }

    fn translate_opcode(
        &mut self,
        instr_ptr: &mut InstructionPtr,
        _return_ptr_offset: usize,
    ) -> Result<(), CompilerError> {
        use Instruction as WI;
        let injection_begin = self.code_section.len();
        let mut opcode_count_origin = 1;

        match *instr_ptr.get() {
            // WI::BrAdjust(branch_offset) => {
            //     opcode_count_origin += 1;
            //     Self::extract_drop_keep(instr_ptr).translate(&mut self.code_section)?;
            //     self.code_section.op_br(branch_offset);
            //     self.code_section.op_return();
            // }
            // WI::BrAdjustIfNez(branch_offset) => {
            //     opcode_count_origin += 1;
            //     let br_if_offset = self.code_section.len();
            //     self.code_section.op_br_if_eqz(0);
            //     Self::extract_drop_keep(instr_ptr).translate(&mut self.code_section)?;
            //     let drop_keep_len = self.code_section.len() - br_if_offset + 1;
            //     self.code_section
            //         .get_mut(br_if_offset as usize)
            //         .unwrap()
            //         .update_branch_offset(BranchOffset::from(1 + drop_keep_len as i32));
            //     let mut branch_offset = branch_offset.to_i32();
            //     if branch_offset < 0 {
            //         branch_offset -= 3;
            //     }
            //     self.code_section.op_br(branch_offset);
            //     self.code_section.op_return();
            // }
            WI::ReturnCallInternal(_) | WI::ReturnCall(_) | WI::ReturnCallIndirect(_) => {
                unreachable!("not supported tail call")
            }
            WI::BrTable(branch_targets) => {
                opcode_count_origin += branch_targets.to_usize() * 2;
                self.code_section.op_br_table(branch_targets);
                for _ in 0..branch_targets.to_usize() {
                    instr_ptr.add(1);
                    let opcode1 = *instr_ptr.get();
                    instr_ptr.add(1);
                    let opcode2 = *instr_ptr.get();
                    match (opcode1, opcode2) {
                        (Instruction::BrAdjust(_), Instruction::Return(drop_keep)) => {
                            assert!(
                                drop_keep.drop() == 0 && drop_keep.keep() == 0,
                                "drop keep must be empty for BrTable targets"
                            );
                        }
                        (Instruction::Return(_), Instruction::Return(drop_keep)) => {
                            assert!(
                                drop_keep.drop() == 0 && drop_keep.keep() == 0,
                                "drop keep must be empty for BrTable targets"
                            );
                        }
                        _ => unreachable!("not possible opcode in the BrTable branch"),
                    }
                    self.code_section.push(opcode1);
                    self.code_section.push(opcode2);
                }
            }
            // WI::Return(drop_keep) => {
            //     drop_keep.translate(&mut self.code_section)?;
            //     self.code_section.op_return();
            // }
            // WI::ReturnIfNez(drop_keep) => {
            //     let br_if_offset = self.code_section.len();
            //     self.code_section.op_br_if_eqz(0);
            //     drop_keep.translate(&mut self.code_section)?;
            //     let drop_keep_len = self.code_section.len() - br_if_offset;
            //     self.code_section
            //         .get_mut(br_if_offset as usize)
            //         .unwrap()
            //         .update_branch_offset(BranchOffset::from(1 + drop_keep_len as i32));
            //     self.code_section.op_return();
            // }
            WI::Call(func_idx) => {
                self.code_section.op_call_internal(func_idx.to_u32());
            }
            WI::CallInternal(func_idx) => {
                let import_len = self.module.imports.len_funcs as u32;
                self.code_section
                    .op_call_internal(func_idx.to_u32() + import_len);
            }
            // WI::ConstRef(const_ref) => {
            //     let resolved_const = self.engine.resolve_const(const_ref).unwrap();
            //     self.code_section.op_i64_const(resolved_const);
            // }
            // WI::MemoryInit(data_segment_idx) => {
            //     self.code_section.op_memory_init(data_segment_idx);
            // }
            // WI::TableInit(elem_segment_idx) => {
            //     let table = Self::extract_table(instr_ptr);
            //     self.code_section.op_table_init(table, elem_segment_idx);
            // }
            // WI::MemoryGrow => {
            //     assert!(!self.module.memories.is_empty(), "memory must be provided");
            //     let max_pages = self.module.memories[0]
            //         .maximum_pages()
            //         .unwrap_or(Pages::max())
            //         .into_inner();
            //     self.code_section.op_local_get(1);
            //     self.code_section.op_memory_size();
            //     self.code_section.op_i32_add();
            //     self.code_section.op_i32_const(max_pages);
            //     self.code_section.op_i32_gt_s();
            //     self.code_section.op_br_if_eqz(4);
            //     self.code_section.op_drop();
            //     self.code_section.op_i32_const(u32::MAX);
            //     self.code_section.op_br(2);
            //     self.code_section.op_memory_grow();
            // }
            // WI::TableGrow(idx) => {
            //     let max_size = self.module.tables[idx.to_u32() as usize]
            //         .maximum()
            //         .unwrap_or(N_MAX_TABLES);
            //     self.code_section.op_local_get(1);
            //     self.code_section.op_table_size(idx);
            //     self.code_section.op_i32_add();
            //     self.code_section.op_i32_const(max_size);
            //     self.code_section.op_i32_gt_s();
            //     self.code_section.op_br_if_eqz(4);
            //     self.code_section.op_drop();
            //     self.code_section.op_drop();
            //     self.code_section.op_i32_const(u32::MAX);
            //     self.code_section.op_br(2);
            //     self.code_section.op_table_grow(idx);
            // }
            _ => {
                self.code_section.push(*instr_ptr.get());
            }
        };
        let injection_end = self.code_section.len();
        if injection_end - injection_begin > opcode_count_origin as u32 {
            self.injection_segments.push(Injection {
                begin: injection_begin as i32,
                end: injection_end as i32,
                origin_len: opcode_count_origin as i32,
            });
        }

        instr_ptr.add(1);
        Ok(())
    }

    fn resolve_host_call(
        &mut self,
        fn_index: u32,
    ) -> Result<(u32, u32, usize, usize), CompilerError> {
        let imports = self
            .module
            .imports
            .items
            .iter()
            .filter(|import| matches!(import, Imported::Func(_)))
            .collect::<Vec<_>>();
        if fn_index >= imports.len() as u32 {
            return Err(CompilerError::NotSupportedImport);
        }
        let imported = &imports[fn_index as usize];
        let import_name = match imported {
            Imported::Func(import_name) => import_name,
            _ => return Err(CompilerError::NotSupportedImport),
        };
        let (index, fuel_cost) = self
            .import_linker
            .ok_or(CompilerError::UnknownImport(import_name.clone()))?
            .index_mapping()
            .get(import_name)
            .copied()
            .ok_or(CompilerError::UnknownImport(import_name.clone()))?;
        let import_func = self
            .import_linker
            .ok_or(CompilerError::UnknownImport(import_name.clone()))?
            .resolve_by_index(index)
            .ok_or(CompilerError::UnknownImport(import_name.clone()))?;
        let (len_input, len_output) = import_func.func_type().len_params();
        Ok((index, fuel_cost, len_input, len_output))
    }

    fn extract_drop_keep(instr_ptr: &mut InstructionPtr) -> DropKeep {
        instr_ptr.add(1);
        let next_instr = instr_ptr.get();
        match next_instr {
            Instruction::Return(drop_keep) => *drop_keep,
            _ => unreachable!("incorrect instr after break adjust ({:?})", *next_instr),
        }
    }

    fn extract_table(instr_ptr: &mut InstructionPtr) -> TableIdx {
        instr_ptr.add(1);
        let next_instr = instr_ptr.get();
        match next_instr {
            Instruction::TableGet(table_idx) => *table_idx,
            _ => unreachable!("incorrect instr after break adjust ({:?})", *next_instr),
        }
    }

    fn translate_router(&mut self, main_index: FuncOrExport) -> Result<(), CompilerError> {
        // translate router into separate instruction set
        let router_opcodes = self.create_router(main_index)?;
        // inject main function call with return
        self.code_section.extend(&router_opcodes);
        self.code_section.op_return();
        self.code_section.op_unreachable();
        Ok(())
    }

    fn create_router(&mut self, main_index: FuncOrExport) -> Result<InstructionSet, CompilerError> {
        let mut router_opcodes = InstructionSet::default();
        let func_index = self.resolve_func_index(&main_index)?.unwrap_or_default();
        if let Some(input_code) = &self.config.input_code {
            router_opcodes.extend(&input_code);
        }
        match main_index {
            FuncOrExport::Export(_) | FuncOrExport::Func(_) => {
                router_opcodes.op_call_internal(func_index);
            }
            FuncOrExport::Custom(router) => {
                router_opcodes.extend(&router);
            }
            _ => unreachable!("not supported main function"),
        }
        if let Some(output_code) = &self.config.output_code {
            router_opcodes.extend(&output_code);
        }
        Ok(router_opcodes)
    }

    pub fn resolve_func_index(&self, export: &FuncOrExport) -> Result<Option<u32>, CompilerError> {
        match export {
            FuncOrExport::Export(name) => Some(self.resolve_export_index(name)).transpose(),
            FuncOrExport::Func(index) => Ok(Some(*index)),
            _ => Ok(None),
        }
    }

    fn resolve_export_index(&self, name: &str) -> Result<u32, CompilerError> {
        let export_index = self
            .module
            .exports
            .get(name)
            .ok_or(CompilerError::MissingEntrypoint)?
            .into_func_idx()
            .ok_or(CompilerError::MissingEntrypoint)?;
        Ok(export_index)
    }
}

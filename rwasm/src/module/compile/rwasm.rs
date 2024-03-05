use crate::{
    core::{UntypedValue, ValueType},
    engine::{
        bytecode::Instruction,
        CompiledFunc,
        DropKeep,
        FuncBuilder,
        FuncTranslatorAllocations,
    },
    errors::ModuleError,
    module::{
        error::RwasmBuilderError,
        ConstExpr,
        DataSegment,
        DataSegmentKind,
        ElementSegmentKind,
        FuncIdx,
        ModuleResources,
        ReusableAllocations,
    },
};

pub struct RwasmTranslator<'parser> {
    /// The interface to incrementally build up the `wasmi` bytecode function.
    func_builder: FuncBuilder<'parser>,
    /// Module resources
    res: ModuleResources<'parser>,
}

impl<'parser> RwasmTranslator<'parser> {
    pub fn new(
        func: FuncIdx,
        compiled_func: CompiledFunc,
        res: ModuleResources<'parser>,
        allocations: FuncTranslatorAllocations,
    ) -> Self {
        let func_builder = FuncBuilder::new(func, compiled_func, res, None, allocations);
        Self { func_builder, res }
    }

    pub fn translate_entrypoint(mut self) -> Result<ReusableAllocations, ModuleError> {
        self.translate_entrypoint_internal()
            .map_err(|err| ModuleError::Rwasm(err))?;
        let allocations = self
            .func_builder
            .finish(None)
            .map_err(Into::<ModuleError>::into)?;
        Ok(allocations)
    }

    pub fn translate_import_func(
        mut self,
        import_func_index: u32,
    ) -> Result<(ReusableAllocations, u32), ModuleError> {
        let sys_index = self
            .translate_import_func_internal(import_func_index)
            .map_err(|err| ModuleError::Rwasm(err))?;
        let allocations = self
            .func_builder
            .finish(None)
            .map_err(Into::<ModuleError>::into)?;
        Ok((allocations, sys_index))
    }

    fn translate_entrypoint_internal(&mut self) -> Result<(), RwasmBuilderError> {
        // first we must translate all sections, this is an entrypoint
        self.translate_sections()?;
        // translate router for main index
        self.translate_router("main")?;
        // translate import functions
        // for i in 0..self.res.res.imports.len_funcs() {
        //     self.translate_import_func(i as u32)?;
        // }
        // push unreachable in the end (indication of the entrypoint end)
        self.func_builder
            .translator
            .alloc
            .inst_builder
            .push_inst(Instruction::Unreachable);
        Ok(())
    }

    fn translate_router(&mut self, entrypoint_name: &'static str) -> Result<(), RwasmBuilderError> {
        // translate router into separate instruction set
        let instr_builder = &mut self.func_builder.translator.alloc.inst_builder;
        let export_index = self
            .res
            .res
            .exports
            .get(entrypoint_name)
            .ok_or(RwasmBuilderError::MissingEntrypoint)?
            .into_func_idx()
            .ok_or(RwasmBuilderError::MissingEntrypoint)?;
        // we do plus one to skip entrypoint section
        instr_builder.push_inst(Instruction::CallInternal(export_index.into()));
        instr_builder.push_inst(Instruction::Return(DropKeep::none()));
        Ok(())
    }

    fn translate_import_func_internal(
        &mut self,
        import_fn_index: u32,
    ) -> Result<u32, RwasmBuilderError> {
        let (import_index, fuel_cost) = self.resolve_host_call(import_fn_index)?;
        let instr_builder = &mut self.func_builder.translator.alloc.inst_builder;
        if self.res.engine().config().get_consume_fuel() {
            instr_builder.push_inst(Instruction::ConsumeFuel(fuel_cost.into()));
        }
        // instr_builder.push_inst(Instruction::Call((import_fn_index + 1).into()));
        instr_builder.push_inst(Instruction::Call(import_index.into()));
        instr_builder.push_inst(Instruction::Return(DropKeep::none()));
        Ok(import_index)
    }

    fn resolve_host_call(&mut self, fn_index: u32) -> Result<(u32, u32), RwasmBuilderError> {
        let imports = self.res.res.imports.funcs.iter().collect::<Vec<_>>();
        if fn_index >= imports.len() as u32 {
            return Err(RwasmBuilderError::NotSupportedImport);
        }
        let import_name = imports[fn_index as usize];
        let import_linker = self.res.res.engine().config().get_import_linker();
        let (index, fuel_cost) = import_linker
            .ok_or(RwasmBuilderError::UnknownImport(import_name.clone()))?
            .index_mapping()
            .get(import_name)
            .copied()
            .ok_or(RwasmBuilderError::UnknownImport(import_name.clone()))?;
        Ok((index, fuel_cost))
    }

    pub fn translate_sections(&mut self) -> Result<(), RwasmBuilderError> {
        // translate global section (replaces with set/get global opcodes)
        self.translate_globals()?;
        // translate table section (replace with grow/set table opcodes)
        self.translate_tables()?;
        self.translate_elements()?;
        // translate memory section (replace with grow/load memory opcodes)
        self.translate_memories()?;
        self.translate_data()?;
        Ok(())
    }

    pub fn translate_globals(&mut self) -> Result<(), RwasmBuilderError> {
        for i in 0..self.res.res.globals.len() {
            self.translate_global(i as u32)?;
        }
        Ok(())
    }

    pub fn translate_global(&mut self, global_index: u32) -> Result<(), RwasmBuilderError> {
        let instr_builder = &mut self.func_builder.translator.alloc.inst_builder;
        let globals = &self.res.res.globals;
        assert!(global_index < globals.len() as u32);
        // if global index less than global num then its imported global, and we have special call
        // index to translate such calls
        let len_globals = self.res.res.imports.len_globals();
        if global_index < len_globals as u32 {
            todo!("exported globals are not supported yet");
            // let global_start_index = self
            //     .config
            //     .global_start_index
            //     .ok_or(RwasmBuilderError::ExportedGlobalsAreDisabled)?;
            // self.code_section.op_call(global_start_index + global_index);
            // self.code_section.op_global_set(global_index);
            // return Ok(());
        }
        let global_inits = &self.res.res.globals_init;
        assert!(global_index as usize - len_globals < global_inits.len());
        let global_expr = &global_inits[global_index as usize - len_globals];
        if let Some(value) = global_expr.eval_const() {
            instr_builder.push_inst(Instruction::I64Const(value));
        } else if let Some(value) = global_expr.funcref() {
            instr_builder.push_inst(Instruction::RefFunc(value.into_u32().into()));
        } else if let Some(index) = global_expr.global() {
            instr_builder.push_inst(Instruction::GlobalGet(index.into()));
        } else {
            let value = Self::translate_const_expr(global_expr)?;
            instr_builder.push_inst(Instruction::I64Const(value));
        }
        instr_builder.push_inst(Instruction::GlobalSet(global_index.into()));
        Ok(())
    }

    pub fn translate_tables(&mut self) -> Result<(), RwasmBuilderError> {
        let instr_builder = &mut self.func_builder.translator.alloc.inst_builder;
        for (table_index, table) in self.res.res.tables.iter().enumerate() {
            // don't use ref_func here due to the entrypoint section
            instr_builder.push_inst(Instruction::I32Const(0.into()));
            if table_index < self.res.res.imports.len_tables() {
                todo!("imported tables are not supported yet")
            }
            instr_builder.push_inst(Instruction::I64Const(table.minimum().into()));
            instr_builder.push_inst(Instruction::TableGrow((table_index as u32).into()));
            instr_builder.push_inst(Instruction::Drop);
        }
        Ok(())
    }

    pub fn translate_elements(&mut self) -> Result<(), RwasmBuilderError> {
        let (rwasm_builder, instr_builder) = (
            &mut self.func_builder.translator.alloc.rwasm_builder,
            &mut self.func_builder.translator.alloc.inst_builder,
        );
        for (i, e) in self.res.res.element_segments.iter().enumerate() {
            if e.ty() != ValueType::FuncRef {
                return Err(RwasmBuilderError::OnlyFuncRefAllowed);
            }
            match &e.kind() {
                ElementSegmentKind::Passive => {
                    let into_inter = e.items.exprs.into_iter().map(|v| {
                        v.funcref()
                            .expect("only funcref type is allowed to sections")
                            .into_u32()
                    });
                    rwasm_builder.add_passive_elements((i as u32).into(), into_inter);
                }
                ElementSegmentKind::Active(aes) => {
                    let dest_offset = Self::translate_const_expr(aes.offset())?;
                    let into_inter = e.items.exprs.into_iter().map(|v| {
                        v.funcref()
                            .expect("only funcref type is allowed to sections")
                            .into_u32()
                    });
                    rwasm_builder.add_active_elements(
                        instr_builder,
                        dest_offset.as_u32(),
                        aes.table_index().into_u32().into(),
                        into_inter,
                    );
                }
                ElementSegmentKind::Declared => return Ok(()),
            };
        }
        Ok(())
    }

    fn translate_memories(&mut self) -> Result<(), RwasmBuilderError> {
        let (rwasm_builder, instr_builder) = (
            &mut self.func_builder.translator.alloc.rwasm_builder,
            &mut self.func_builder.translator.alloc.inst_builder,
        );
        for memory in self.res.res.memories.iter() {
            rwasm_builder.add_memory_pages(instr_builder, memory.initial_pages().into_inner());
        }
        Ok(())
    }

    pub fn translate_data(&mut self) -> Result<(), RwasmBuilderError> {
        let (rwasm_builder, instr_builder) = (
            &mut self.func_builder.translator.alloc.rwasm_builder,
            &mut self.func_builder.translator.alloc.inst_builder,
        );
        for (idx, memory) in self.res.res.data_segments.iter().enumerate() {
            let (offset, bytes, is_active) = Self::read_memory_segment(memory)?;
            if is_active {
                rwasm_builder.add_default_memory(instr_builder, offset.as_u32(), bytes);
            } else {
                rwasm_builder.add_passive_memory((idx as u32).into(), bytes);
            }
        }
        Ok(())
    }

    fn read_memory_segment(
        memory: &DataSegment,
    ) -> Result<(UntypedValue, &[u8], bool), RwasmBuilderError> {
        match memory.kind() {
            DataSegmentKind::Active(seg) => {
                assert_eq!(
                    seg.memory_index().into_u32(),
                    0,
                    "memory index can't be zero"
                );
                let data_offset = Self::translate_const_expr(seg.offset())?;
                return Ok((data_offset, memory.bytes(), true));
            }
            DataSegmentKind::Passive => Ok((0.into(), memory.bytes(), false)),
        }
    }

    pub fn translate_const_expr(const_expr: &ConstExpr) -> Result<UntypedValue, RwasmBuilderError> {
        return if cfg!(feature = "e2e") {
            let init_value = const_expr
                .eval_with_context(|_| crate::Value::I32(666), |_| crate::FuncRef::default())
                .ok_or(RwasmBuilderError::NotSupportedGlobalExpr)?;
            Ok(init_value)
        } else {
            let init_value = const_expr
                .eval_const()
                .ok_or(RwasmBuilderError::NotSupportedGlobalExpr)?;
            Ok(init_value)
        };
    }
}

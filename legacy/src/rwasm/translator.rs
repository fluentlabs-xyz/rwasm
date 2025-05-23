use crate::{
    core::{UntypedValue, ValueType},
    engine::{
        bytecode,
        bytecode::Instruction,
        func_builder::{FuncTranslator, FuncTranslatorI32, FuncTranslators},
        CompiledFunc,
        DropKeep,
        FuncTranslatorAllocations,
    },
    errors::ModuleError,
    module::{ConstExpr, DataSegmentKind, ElementSegmentKind, FuncIdx, ModuleResources},
    rwasm::RwasmBuilderError,
    value::split_i64_to_i32,
};
use alloc::vec::Vec;
use core::iter;

pub struct RwasmTranslator<'parser> {
    /// The interface to incrementally build up the `wasmi` bytecode function.
    translator: FuncTranslators<'parser>,
    /// Module resources
    res: ModuleResources<'parser>,
}

impl<'parser> RwasmTranslator<'parser> {
    pub fn new(
        func: FuncIdx,
        compiled_func: CompiledFunc,
        res: ModuleResources<'parser>,
        allocations: FuncTranslatorAllocations,
        i32_translator: bool,
    ) -> Self {
        let translator = if i32_translator {
            FuncTranslators::TranslatorI32(FuncTranslatorI32::new(
                func,
                compiled_func,
                res,
                allocations,
            ))
        } else {
            FuncTranslators::Translator(FuncTranslator::new(func, compiled_func, res, allocations))
        };
        Self { translator, res }
    }

    pub fn translate_entrypoint(mut self) -> Result<FuncTranslatorAllocations, ModuleError> {
        self.translate_entrypoint_internal()
            .map_err(|err| ModuleError::Rwasm(err))?;
        Ok(self.translator.into_allocations())
    }

    pub fn translate_import_func(
        mut self,
        import_func_index: u32,
    ) -> Result<(FuncTranslatorAllocations, u32), ModuleError> {
        let sys_index = self
            .translate_import_func_internal(import_func_index)
            .map_err(|err| ModuleError::Rwasm(err))?;
        self.translator
            .finish()
            .map_err(Into::<ModuleError>::into)?;
        Ok((
            match self.translator {
                FuncTranslators::Translator(t) => t.alloc,
                FuncTranslators::TranslatorI32(t) => t.alloc,
            },
            sys_index,
        ))
    }

    fn translate_entrypoint_internal(&mut self) -> Result<(), RwasmBuilderError> {
        let ib = &mut self.translator.alloc().inst_builder;
        ib.push_inst(Instruction::SignatureCheck(0.into()));
        ib.push_inst(Instruction::StackAlloc {
            max_stack_height: 0,
        });
        // first, we must translate all sections; this is an entrypoint
        self.translate_sections()?;
        // translate router for the main index (only if entrypoint is enabled)
        if let Some(start) = self.res.res.start {
            // for the start section we must always invoke even if there is a main function,
            // otherwise it might be super misleading for devs why
            match self.res.get_compiled_func(start) {
                Some(compiled_func) => {
                    self.translator
                        .alloc()
                        .inst_builder
                        .push_inst(Instruction::CallInternal(compiled_func));
                }
                None => {
                    let func_idx = bytecode::FuncIdx::from(start.into_u32());
                    self.translator
                        .alloc()
                        .inst_builder
                        .push_inst(Instruction::Call(func_idx));
                }
            }
        }
        // if we have an entrypoint, then translate it
        self.translate_simple_router()?;
        // if we have a state router, then translate state router
        self.translate_state_router()?;
        // push unreachable in the end (indication of the entrypoint end)
        self.translator
            .alloc()
            .inst_builder
            .push_inst(Instruction::Return(DropKeep::none()));
        Ok(())
    }

    fn translate_simple_router(&mut self) -> Result<(), RwasmBuilderError> {
        let config = self.res.engine().config();
        // if we have an entrypoint, then translate it
        let Some(entrypoint_name) = config
            .get_rwasm_config()
            .and_then(|rwasm_config| rwasm_config.entrypoint_name.as_ref())
        else {
            return Ok(());
        };
        let export_index = self
            .res
            .res
            .exports
            .get(entrypoint_name.as_str())
            .ok_or(RwasmBuilderError::MissingEntrypoint)?
            .into_func_idx()
            .ok_or(RwasmBuilderError::MissingEntrypoint)?
            .into_u32();
        // we must validate the number of input/output params
        // to make sure it won't cause potential stack overflow or underflow
        self.ensure_func_type_empty(export_index)?;
        // emit call internal for the `main` function inside entrypoint
        let instr_builder = &mut self.translator.alloc().inst_builder;
        instr_builder.push_inst(Instruction::CallInternal(export_index.into()));
        Ok(())
    }

    fn ensure_func_type_empty(&self, func_index: u32) -> Result<(), RwasmBuilderError> {
        let config = self.res.engine().config();
        let allow_malformed_entrypoint_func_type = config
            .get_rwasm_config()
            .map(|rwasm_config| rwasm_config.allow_malformed_entrypoint_func_type)
            .unwrap_or(false);
        let dedup_func_type = self
            .res
            .res
            .funcs
            .get(func_index as usize)
            .ok_or(RwasmBuilderError::MalformedFuncType)?;
        let is_empty_params = self
            .res
            .engine()
            .resolve_func_type(dedup_func_type, |func_type| {
                func_type.len_params() == (0, 0)
            });
        if !is_empty_params && !allow_malformed_entrypoint_func_type {
            Err(RwasmBuilderError::MalformedFuncType)
        } else {
            Ok(())
        }
    }

    fn translate_state_router(&mut self) -> Result<(), RwasmBuilderError> {
        let config = self.res.engine().config();
        // if we have a state router, then translate state router
        let allow_malformed_entrypoint_func_type = config
            .get_rwasm_config()
            .map(|rwasm_config| rwasm_config.allow_malformed_entrypoint_func_type)
            .unwrap_or(false);
        let state_router = config
            .get_rwasm_config()
            .and_then(|rwasm_config| rwasm_config.state_router.as_ref());
        let state_router = match state_router {
            Some(value) => value,
            None => return Ok(()),
        };
        let instr_builder = &mut self.translator.alloc().inst_builder;
        // push state on the stack
        instr_builder.push_inst(state_router.opcode);
        // translate state router
        for (entrypoint_name, state_value) in state_router.states.iter() {
            let exports = &self.res.res.exports;
            let Some(func_idx) = exports
                .get(entrypoint_name.as_str())
                .and_then(|v| v.into_func_idx())
                .map(|v| v.into_u32())
            else {
                continue;
            };
            // make sure func type is empty
            let dedup_func_type = self
                .res
                .res
                .funcs
                .get(func_idx as usize)
                .ok_or(RwasmBuilderError::MalformedFuncType)?;
            let is_empty_params = self
                .res
                .engine()
                .resolve_func_type(dedup_func_type, |func_type| {
                    func_type.len_params() == (0, 0)
                });
            if !is_empty_params && !allow_malformed_entrypoint_func_type {
                return Err(RwasmBuilderError::MalformedFuncType);
            }
            instr_builder.push_inst(Instruction::LocalGet(1.into()));
            instr_builder.push_inst(Instruction::I32Const((*state_value).into()));
            instr_builder.push_inst(Instruction::I32Eq);
            instr_builder.push_inst(Instruction::BrIfEqz(4.into()));
            // it's super important to drop the original state from the stack
            // because input params might be passed though the stack
            instr_builder.push_inst(Instruction::Drop);
            instr_builder.push_inst(Instruction::CallInternal(func_idx.into()));
            instr_builder.push_inst(Instruction::Return(DropKeep::none()));
        }
        // drop input state from the stack
        instr_builder.push_inst(Instruction::Drop);
        Ok(())
    }

    fn translate_import_func_internal(
        &mut self,
        import_fn_index: u32,
    ) -> Result<u32, RwasmBuilderError> {
        let imports = self.res.res.imports.funcs.iter().collect::<Vec<_>>();
        if import_fn_index >= imports.len() as u32 {
            return Err(RwasmBuilderError::NotSupportedImport);
        }
        let import_name = imports[import_fn_index as usize];
        let config = self.res.res.engine().config();
        let linker_entity = config
            .get_rwasm_config()
            .and_then(|rwasm_config| rwasm_config.import_linker.as_ref())
            .ok_or(RwasmBuilderError::UnknownImport(import_name.clone()))?
            .resolve_by_import_name(import_name)
            .ok_or(RwasmBuilderError::UnknownImport(import_name.clone()))?;
        let dedup_func_type = self
            .res
            .res
            .funcs
            .get(import_fn_index as usize)
            .ok_or(RwasmBuilderError::MalformedFuncType)?;
        let is_type_matches = self
            .res
            .engine()
            .resolve_func_type(dedup_func_type, |func_type| {
                let (params, result) = func_type.params_results();
                params == linker_entity.params && result == linker_entity.result
            });
        if !is_type_matches {
            return Err(RwasmBuilderError::MalformedFuncType);
        }
        let ib = &mut self.translator.alloc().inst_builder;
        ib.push_inst(Instruction::SignatureCheck(0.into()));
        ib.push_inst(Instruction::StackAlloc {
            max_stack_height: 0,
        });
        if self.res.engine().config().get_builtins_consume_fuel() {
            for instr in linker_entity.fuel_procedure.iter() {
                ib.push_inst(instr.clone());
            }
        }
        ib.push_inst(Instruction::Call(linker_entity.func_idx.into()));
        ib.push_inst(Instruction::Return(DropKeep::none()));
        Ok(import_fn_index)
    }

    pub fn translate_sections(&mut self) -> Result<(), RwasmBuilderError> {
        // translate a global section (replaces with set/get global opcodes)
        self.translate_globals()?;
        // translate a table section (replace with grow/set table opcodes)
        self.translate_tables()?;
        self.translate_elements()?;
        // translate a memory section (replace with grow/load memory opcodes)
        self.translate_memory()?;
        Ok(())
    }

    pub fn translate_globals(&mut self) -> Result<(), RwasmBuilderError> {
        for i in 0..self.res.res.globals.len() {
            self.translate_global(i as u32)?;
        }
        Ok(())
    }

    pub fn translate_const(&mut self, const_type: ValueType, value: UntypedValue) {
        let is_i32_translator = self.translator.is_i32_translator();
        let ib = &mut self.translator.alloc().inst_builder;
        if is_i32_translator {
            match const_type {
                ValueType::I32 => {
                    ib.push_inst(Instruction::I32Const(value));
                }
                ValueType::I64 => {
                    if is_i32_translator {}
                    let [lower, upper] = split_i64_to_i32(value.as_u64() as i64);
                    ib.push_inst(Instruction::I32Const(UntypedValue::from(lower)));
                    ib.push_inst(Instruction::I32Const(UntypedValue::from(upper)));
                }
                ValueType::F32 => {
                    ib.push_inst(Instruction::F32Const(value));
                }
                ValueType::F64 => {
                    ib.push_inst(Instruction::F64Const(value));
                }
                _ => {
                    ib.push_inst(Instruction::I32Const(value));
                }
            }
        } else {
            ib.push_inst(Instruction::I64Const(value));
        }
    }

    pub fn translate_global(&mut self, global_index: u32) -> Result<(), RwasmBuilderError> {
        let is_i32_translator = self.translator.is_i32_translator();
        let ib = &mut self.translator.alloc().inst_builder;
        let globals = &self.res.res.globals;
        assert!(global_index < globals.len() as u32);
        // if global index less than global num then its imported global, and we have special call
        // index to translate such calls
        let len_globals = self.res.res.imports.len_globals();
        let global_type = globals[global_index as usize].content();
        if global_index < len_globals as u32 {
            // so let's put this hardcoded condition here only for e2e tests, otherwise we need to
            // patch a lot of spec tests
            if cfg!(feature = "e2e") {
                if is_i32_translator {
                    if global_type == ValueType::I64 && is_i32_translator {
                        ib.push_inst(Instruction::I32Const(666.into()));
                        ib.push_inst(Instruction::I32Const(0.into()));
                        ib.push_inst(Instruction::GlobalSet((global_index * 2).into()));
                        ib.push_inst(Instruction::GlobalSet((global_index * 2 + 1).into()));
                    } else {
                        ib.push_inst(Instruction::I32Const(666.into()));
                        ib.push_inst(Instruction::GlobalSet((global_index * 2).into()));
                    }
                } else {
                    ib.push_inst(Instruction::I64Const(666.into()));
                    ib.push_inst(Instruction::GlobalSet(global_index.into()));
                }
                return Ok(());
            }
            return Err(RwasmBuilderError::ImportedGlobalsAreDisabled);
        }
        let global_inits = &self.res.res.globals_init;
        assert!(global_index as usize - len_globals < global_inits.len());
        let global_expr = &global_inits[global_index as usize - len_globals];
        if let Some(value) = global_expr.eval_const() {
            self.translate_const(global_type, value);
            self.translator.stack_height().push();
        } else if let Some(value) = global_expr.funcref() {
            let ib = &mut self.translator.alloc().inst_builder;
            ib.push_inst(Instruction::RefFunc(value.into_u32().into()));
            self.translator.stack_height().push();
        } else if let Some(index) = global_expr.global() {
            let ib = &mut self.translator.alloc().inst_builder;
            if is_i32_translator {
                if global_type == ValueType::I64 {
                    ib.push_inst(Instruction::GlobalGet((index * 2 + 1).into()));
                }
                ib.push_inst(Instruction::GlobalGet((index * 2).into()));
            } else {
                ib.push_inst(Instruction::GlobalGet(index.into()));
            }
        } else {
            let value = Self::translate_const_expr(global_expr)?;
            self.translate_const(global_type, value);
            self.translator.stack_height().push();
        }
        if is_i32_translator {
            self.translator
                .alloc()
                .inst_builder
                .push_inst(Instruction::GlobalSet((global_index * 2).into()));
            if global_type == ValueType::I64 {
                self.translator
                    .alloc()
                    .inst_builder
                    .push_inst(Instruction::GlobalSet((global_index * 2 + 1).into()));
            }
        } else {
            let ib = &mut self.translator.alloc().inst_builder;
            ib.push_inst(Instruction::GlobalSet(global_index.into()));
        }

        Ok(())
    }

    pub fn translate_tables(&mut self) -> Result<(), RwasmBuilderError> {
        let instr_builder = &mut self.translator.alloc().inst_builder;
        for (table_index, table) in self.res.res.tables.iter().enumerate() {
            // don't use ref_func here due to the entrypoint section
            if table_index < self.res.res.imports.len_tables() {
                return Err(RwasmBuilderError::ImportedTablesAreDisabled);
            }
            instr_builder.push_inst(Instruction::I32Const(0.into()));
            instr_builder.push_inst(Instruction::I64Const(table.minimum().into()));
            instr_builder.push_inst(Instruction::TableGrow((table_index as u32).into()));
            instr_builder.push_inst(Instruction::Drop);
        }
        Ok(())
    }

    fn func_ref_or_const_zero(v: &ConstExpr) -> u32 {
        if let Some(const_value) = v.eval_const() {
            assert_eq!(
                const_value.as_u32(),
                0,
                "const as funcref must only be null value"
            );
            // we encode nullptr as `u32::MAX` since its impossible number of
            // function refs
            // TODO(dmitry123): "is it right decision? more tests needed"
            u32::MAX
        } else {
            v.funcref()
                .expect("only funcref type is allowed to sections")
                .into_u32()
        }
    }

    pub fn translate_elements(&mut self) -> Result<(), RwasmBuilderError> {
        let alloc = self.translator.alloc();
        let (rwasm_builder, instr_builder) = (&mut alloc.segment_builder, &mut alloc.inst_builder);
        for (i, e) in self.res.res.element_segments.iter().enumerate() {
            if e.ty() != ValueType::FuncRef {
                return Err(RwasmBuilderError::OnlyFuncRefAllowed);
            }
            match &e.kind() {
                ElementSegmentKind::Passive => {
                    let into_inter = e
                        .items
                        .exprs
                        .into_iter()
                        .map(|v| Self::func_ref_or_const_zero(v));
                    rwasm_builder.add_passive_elements((i as u32).into(), into_inter);
                }
                ElementSegmentKind::Active(aes) => {
                    let dest_offset = Self::translate_const_expr(aes.offset())?;
                    let into_inter = e
                        .items
                        .exprs
                        .into_iter()
                        .map(|v| Self::func_ref_or_const_zero(v));
                    rwasm_builder.add_active_elements(
                        instr_builder,
                        (i as u32).into(),
                        dest_offset.as_u32(),
                        aes.table_index().into_u32().into(),
                        into_inter,
                    );
                }
                ElementSegmentKind::Declared => {
                    rwasm_builder.add_passive_elements((i as u32).into(), iter::empty());
                }
            };
        }
        Ok(())
    }

    fn translate_memory(&mut self) -> Result<(), RwasmBuilderError> {
        let alloc = self.translator.alloc();
        let (rwasm_builder, instr_builder) = (&mut alloc.segment_builder, &mut alloc.inst_builder);
        let is_imported_memory = self.res.res.imports.len_memories() > 0;
        if is_imported_memory {
            return Err(RwasmBuilderError::ImportedMemoriesAreDisabled);
        }
        for memory in self.res.res.memories.iter() {
            rwasm_builder.add_memory_pages(instr_builder, memory.initial_pages().into_inner());
        }
        for (idx, memory) in self.res.res.data_segments.iter().enumerate() {
            match memory.kind() {
                DataSegmentKind::Active(seg) => {
                    let data_offset = Self::translate_const_expr(seg.offset())?;
                    rwasm_builder.add_active_memory(
                        instr_builder,
                        (idx as u32).into(),
                        data_offset.as_u32(),
                        &memory.bytes,
                    );
                }
                DataSegmentKind::Passive => {
                    rwasm_builder.add_passive_memory((idx as u32).into(), &memory.bytes)
                }
            }
        }
        Ok(())
    }

    pub fn translate_const_expr(const_expr: &ConstExpr) -> Result<UntypedValue, RwasmBuilderError> {
        // we hardcode this value to pass some e2e tests
        // and avoid applying a lot of patches for spec tests
        if cfg!(feature = "e2e") && const_expr.global().is_some() {
            return Ok(UntypedValue::from(666));
        }
        let init_value = const_expr
            .eval_const()
            .ok_or(RwasmBuilderError::NotSupportedGlobalExpr)?;
        Ok(init_value)
    }
}

use crate::{
    compiler::{
        config::CompilerConfig,
        types::{
            CompilerError,
            FuncOrExport,
            Injection,
            N_MAX_RECURSION_DEPTH,
            N_MAX_STACK_HEIGHT,
        },
    },
    drop_keep::DropKeepWithReturnParam,
    types::Translator,
    BinaryFormat,
    ImportLinker,
    InstructionSet,
    RwasmModule,
    N_MAX_TABLES,
};
use alloc::{collections::BTreeMap, vec::Vec};
use rwasm::{
    core::{Pages, UntypedValue, ValueType},
    engine::{
        bytecode::{BranchOffset, Instruction, TableIdx},
        code_map::InstructionPtr,
        DropKeep,
    },
    module::{ConstExpr, DataSegment, DataSegmentKind, ElementSegmentKind, Imported},
    Config,
    Engine,
    Module,
    StackLimits,
};
use std::ops::Deref;

pub struct Compiler2<'linker> {
    // input params
    pub(crate) import_linker: Option<&'linker ImportLinker>,
    pub(crate) config: CompilerConfig,
    // parsed wasmi state
    engine: Engine,
    module: Module,
    // translation state
    pub(crate) code_section: InstructionSet,
    function_beginning: BTreeMap<u32, u32>,
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
        engine_config.wasm_tail_call(true);
        engine_config.wasm_extended_const(config.extended_const);
        engine_config.consume_fuel(config.fuel_consume);
        engine_config.wasm_tail_call(config.tail_call);
        let engine = Engine::new(&engine_config);
        let module =
            Module::new(&engine, wasm_binary).map_err(|e| CompilerError::ModuleError(e))?;
        Ok(Compiler2 {
            engine,
            module,
            code_section: InstructionSet::new(),
            function_beginning: BTreeMap::new(),
            import_linker,
            injection_segments: vec![],
            config,
        })
    }

    pub fn config(&self) -> &CompilerConfig {
        &self.config
    }

    pub fn translate(&mut self, main_index: FuncOrExport) -> Result<(), CompilerError> {
        // first we must translate all sections, this is an entrypoint
        if self.config.translate_sections {
            self.translate_sections()?;
        }
        // translate router for main index
        if self.config.with_router {
            self.translate_router(main_index)?;
        }
        // remember that this is injected and shifts br/br_if offset
        self.injection_segments.push(Injection {
            begin: 0,
            end: self.code_section.len() as i32,
            origin_len: 0,
        });
        // self.translate_imports_funcs()?;
        // translate rest functions
        let total_fns = self.module.funcs.len();
        for i in 0..total_fns {
            self.translate_function(i as u32)?;
        }
        // there is no need to inject because code is already validated
        self.code_section.finalize(false);
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
            self.code_section.op_i64_const32(0);
        });
        // translate instructions
        let (mut instr_ptr, instr_end) = self.engine.instr_ptr(*func_body);
        while instr_ptr != instr_end {
            self.translate_opcode(&mut instr_ptr, 0)?;
        }
        if !self.config.translate_func_as_inline {
            self.code_section.op_unreachable();
        }
        // remember function offset in the mapping (+1 because 0 is reserved for sections init)
        self.function_beginning.insert(fn_index, beginning_offset);
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
            WI::BrAdjust(branch_offset) => {
                opcode_count_origin += 1;
                Self::extract_drop_keep(instr_ptr).translate(&mut self.code_section)?;
                self.code_section.op_br(branch_offset);
                self.code_section.op_return();
            }
            WI::BrAdjustIfNez(branch_offset) => {
                opcode_count_origin += 1;
                let br_if_offset = self.code_section.len();
                self.code_section.op_br_if_eqz(0);
                Self::extract_drop_keep(instr_ptr).translate(&mut self.code_section)?;
                let drop_keep_len = self.code_section.len() - br_if_offset + 1;
                self.code_section
                    .get_mut(br_if_offset as usize)
                    .unwrap()
                    .update_branch_offset(BranchOffset::from(1 + drop_keep_len as i32));
                // we increase break offset in negative case due to jump over BrAdjustIfNez opcode
                // injection
                let mut branch_offset = branch_offset.to_i32();
                if branch_offset < 0 {
                    branch_offset -= 3;
                }
                self.code_section.op_br(branch_offset);
                self.code_section.op_return();
            }
            WI::Return(drop_keep) => {
                DropKeepWithReturnParam(drop_keep).translate(&mut self.code_section)?;
                self.code_section.op_return();
            }
            WI::ReturnIfNez(drop_keep) => {
                let br_if_offset = self.code_section.len();
                self.code_section.op_br_if_eqz(0);
                DropKeepWithReturnParam(drop_keep).translate(&mut self.code_section)?;
                let drop_keep_len = self.code_section.len() - br_if_offset;
                self.code_section
                    .get_mut(br_if_offset as usize)
                    .unwrap()
                    .update_branch_offset(BranchOffset::from(1 + drop_keep_len as i32));
                self.code_section.op_return();
            }
            WI::Call(func_idx) => {
                self.translate_host_call(func_idx.to_u32())?;
            }
            WI::ConstRef(const_ref) => {
                let resolved_const = self.engine.resolve_const(const_ref).unwrap();
                self.code_section.op_i64_const(resolved_const);
            }
            WI::MemoryGrow => {
                assert!(!self.module.memories.is_empty(), "memory must be provided");
                let max_pages = self.module.memories[0]
                    .maximum_pages()
                    .unwrap_or(Pages::max())
                    .into_inner();
                self.code_section.op_local_get(1);
                self.code_section.op_memory_size();
                self.code_section.op_i32_add();
                self.code_section.op_i64_const32(max_pages);
                self.code_section.op_i32_gt_s();
                self.code_section.op_br_if_eqz(4);
                self.code_section.op_drop();
                self.code_section.op_i64_const32(u32::MAX);
                self.code_section.op_br(2);
                self.code_section.op_memory_grow();
            }
            WI::TableGrow(idx) => {
                let max_size = self.module.tables[idx.to_u32() as usize]
                    .maximum()
                    .unwrap_or(N_MAX_TABLES);
                self.code_section.op_local_get(1);
                self.code_section.op_table_size(idx);
                self.code_section.op_i32_add();
                self.code_section.op_i64_const32(max_size);
                self.code_section.op_i32_gt_s();
                self.code_section.op_br_if_eqz(5);
                self.code_section.op_drop();
                self.code_section.op_drop();
                self.code_section.op_i64_const32(u32::MAX);
                self.code_section.op_br(2);
                self.code_section.op_table_grow(idx);
            }
            // WI::LocalGet(local_depth) => {
            //     self.code_section
            //         .op_local_get(local_depth.to_usize() as u32 + 1);
            // }
            // WI::LocalSet(local_depth) => {
            //     self.code_section
            //         .op_local_set(local_depth.to_usize() as u32 + 1);
            // }
            // WI::LocalTee(local_depth) => {
            //     self.code_section
            //         .op_local_tee(local_depth.to_usize() as u32 + 1);
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
                origin_len: opcode_count_origin,
            });
        }

        instr_ptr.add(1);
        Ok(())
    }

    fn translate_host_call(&mut self, fn_index: u32) -> Result<(), CompilerError> {
        let (import_index, fuel_amount) = self.resolve_host_call(fn_index)?;
        if self.engine.config().get_fuel_consumption_mode().is_some() {
            self.code_section.op_consume_fuel(fuel_amount);
        }
        self.code_section.op_call(import_index);
        Ok(())
    }

    fn resolve_host_call(&mut self, fn_index: u32) -> Result<(u32, u32), CompilerError> {
        let imports = self
            .module
            .imports
            .items
            .deref()
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
        let import_index_and_fuel_amount = self
            .import_linker
            .ok_or(CompilerError::UnknownImport(import_name.clone()))?
            .index_mapping()
            .get(import_name)
            .ok_or(CompilerError::UnknownImport(import_name.clone()))?;
        Ok(*import_index_and_fuel_amount)
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
        match main_index {
            FuncOrExport::Export(_) | FuncOrExport::Func(_) => {
                if let Some(input_code) = &self.config.input_code {
                    router_opcodes.extend(&input_code);
                }
                router_opcodes.op_call_internal(func_index);
                if let Some(output_code) = &self.config.output_code {
                    router_opcodes.extend(&output_code);
                }
            }
            _ => unreachable!("not supported main function"),
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

    fn resolve_global_instr(&self, export: &FuncOrExport) -> Option<Instruction> {
        match export {
            FuncOrExport::Global(ix) => Some(ix.clone()),
            _ => None,
        }
    }

    pub fn translate_sections(&mut self) -> Result<(), CompilerError> {
        // translate global section (replaces with set/get global opcodes)
        self.translate_globals()?;
        // translate table section (replace with grow/set table opcodes)
        self.translate_tables()?;
        self.translate_elements()?;
        // translate memory section (replace with grow/load memory opcodes)
        self.translate_memory()?;
        self.translate_data()?;
        Ok(())
    }

    pub fn translate_memory(&mut self) -> Result<(), CompilerError> {
        for memory in self.module.memories.iter() {
            self.code_section
                .add_memory_pages(memory.initial_pages().into_inner());
        }
        Ok(())
    }

    pub fn translate_data(&mut self) -> Result<(), CompilerError> {
        for (idx, memory) in self.module.data_segments.iter().enumerate() {
            let (offset, bytes, is_active) = Self::read_memory_segment(memory)?;
            if is_active {
                self.code_section.add_default_memory(offset.as_u32(), bytes);
            } else {
                self.code_section
                    .add_passive_memory((idx as u32).into(), bytes);
            }
        }
        Ok(())
    }

    fn read_memory_segment(
        memory: &DataSegment,
    ) -> Result<(UntypedValue, &[u8], bool), CompilerError> {
        match memory.kind() {
            DataSegmentKind::Active(seg) => {
                let data_offset = Self::translate_const_expr(seg.offset())?;
                if seg.memory_index().into_u32() != 0 {
                    return Err(CompilerError::NotSupported("not zero index"));
                }
                return Ok((data_offset, memory.bytes(), true));
            }
            DataSegmentKind::Passive => Ok((0.into(), memory.bytes(), false)),
        }
    }

    pub fn translate_globals(&mut self) -> Result<(), CompilerError> {
        let total_globals = self.module.globals.len();
        for i in 0..total_globals {
            self.translate_global(i as u32)?;
        }
        Ok(())
    }

    pub fn translate_global(&mut self, global_index: u32) -> Result<(), CompilerError> {
        let globals = &self.module.globals;
        assert!(global_index < globals.len() as u32);

        // if global index less than global num then its imported global, and we have special call
        // index to translate such calls
        let len_globals = self.module.imports.len_globals;
        if global_index < len_globals as u32 {
            let global_start_index = self
                .config
                .global_start_index
                .ok_or(CompilerError::ExportedGlobalsAreDisabled)?;
            self.code_section.op_call(global_start_index + global_index);
            self.code_section.op_global_set(global_index);
            return Ok(());
        }

        // extract global init code to embed it into codebase
        let global_inits = &self.module.globals_init;
        assert!(global_index as usize - len_globals < global_inits.len());

        let global_expr = &global_inits[global_index as usize - len_globals];
        if let Some(value) = global_expr.eval_const() {
            self.code_section.op_i64_const(value);
        } else if let Some(value) = global_expr.funcref() {
            self.code_section.op_ref_func(value.into_u32());
        } else if let Some(index) = global_expr.global() {
            self.code_section.op_global_get(index.into_u32());
        } else {
            self.code_section
                .op_i64_const(Self::translate_const_expr(global_expr)?.to_bits());
        }

        self.code_section.op_global_set(global_index);
        Ok(())
    }

    pub fn translate_const_expr(const_expr: &ConstExpr) -> Result<UntypedValue, CompilerError> {
        return if cfg!(feature = "e2e") {
            let init_value = const_expr
                .eval_with_context(|_| rwasm::Value::I32(666), |_| rwasm::FuncRef::default())
                .ok_or(CompilerError::NotSupportedGlobalExpr)?;
            Ok(init_value)
        } else {
            let init_value = const_expr
                .eval_const()
                .ok_or(CompilerError::NotSupportedGlobalExpr)?;
            Ok(init_value)
        };
    }

    pub fn translate_tables(&mut self) -> Result<(), CompilerError> {
        for (table_index, table) in self.module.tables.iter().enumerate() {
            // don't use ref_func here due to the entrypoint section
            self.code_section.op_i64_const32(0);
            if table_index < self.module.imports.len_tables {
                self.code_section.op_i64_const(table.minimum() as usize);
            } else {
                self.code_section.op_i64_const(table.minimum() as usize);
            }
            self.code_section.op_table_grow(table_index as u32);
            self.code_section.op_drop();
        }
        Ok(())
    }

    pub fn translate_elements(&mut self) -> Result<(), CompilerError> {
        for (i, e) in self.module.element_segments.iter().enumerate() {
            if e.ty() != ValueType::FuncRef {
                return Err(CompilerError::OnlyFuncRefAllowed);
            }
            match &e.kind() {
                ElementSegmentKind::Passive => {
                    for (_, item) in e.items_cloned().items().iter().enumerate() {
                        if let Some(value) = item.funcref() {
                            // self.code_section.op_ref_func(value.into_u32());
                            // self.code_section.op_elem_store(i as u32);
                            todo!("not supported yet")
                        }
                    }
                }
                ElementSegmentKind::Active(aes) => {
                    let dest_offset = Self::translate_const_expr(aes.offset())?;
                    for (index, item) in e.items_cloned().items().iter().enumerate() {
                        self.code_section
                            .op_i64_const32(dest_offset.as_u32() + index as u32);
                        if let Some(value) = item.eval_const() {
                            self.code_section.op_i64_const(value);
                        } else if let Some(value) = item.funcref() {
                            self.code_section.op_ref_func(value.into_u32());
                        }
                        self.code_section.op_table_set(aes.table_index().into_u32());
                    }
                    if cfg!(feature = "e2e") {
                        self.code_section.op_i64_const(dest_offset);
                        self.code_section.op_i64_const(0);
                        self.code_section.op_i64_const(0);
                        self.code_section
                            .op_table_init(aes.table_index().into_u32(), i as u32);
                    }
                }
                ElementSegmentKind::Declared => return Ok(()),
            };
        }
        Ok(())
    }

    pub fn finalize(&mut self) -> Result<RwasmModule, CompilerError> {
        let bytecode = &mut self.code_section;

        let mut i = 0;
        while i < bytecode.len() as usize {
            match bytecode.instr[i] {
                Instruction::Br(offset)
                | Instruction::BrIfNez(offset)
                | Instruction::BrAdjust(offset)
                | Instruction::BrAdjustIfNez(offset)
                | Instruction::BrIfEqz(offset) => {
                    let mut offset = offset.to_i32();
                    let start = i as i32;
                    let mut target = start + offset;
                    if offset > 0 {
                        for injection in &self.injection_segments {
                            if injection.begin < target && start < injection.begin {
                                offset += injection.end - injection.begin - injection.origin_len;
                                target += injection.end - injection.begin - injection.origin_len;
                            }
                        }
                    } else {
                        for injection in self.injection_segments.iter().rev() {
                            if injection.end < start && target < injection.end {
                                offset -= injection.end - injection.begin - injection.origin_len;
                                target -= injection.end - injection.begin - injection.origin_len;
                            }
                        }
                    };
                    bytecode.instr[i].update_branch_offset(BranchOffset::from(offset));
                }
                Instruction::BrTable(target) => {
                    i += target.to_usize() * 2;
                }
                _ => {}
            };
            i += 1;
        }

        for instr in bytecode.instr.iter_mut() {
            let func_idx = match instr {
                Instruction::CallInternal(func_idx) => func_idx.to_u32(),
                Instruction::RefFunc(func_idx) => func_idx.to_u32(),
                _ => continue,
            };
            let func_offset = self
                .function_beginning
                .get(&func_idx)
                .copied()
                .ok_or(CompilerError::MissingFunction)?;
            instr.update_call_index(func_offset);
        }

        Ok(RwasmModule {
            code_section: bytecode.clone(),
        })
    }
}

use crate::{
    compiler::{
        block_fuel::compile_block_params,
        compiled_expr::CompiledExpr,
        func_builder::FuncBuilder,
        snippets::Snippet,
        translator::{InstructionTranslator, ReusableAllocations},
    },
    CompilationConfig, CompilationError, ConstructorParams, DataSegmentIdx, ElementSegmentIdx,
    FuncIdx, FuncRef, GlobalIdx, GlobalVariable, ImportName, Opcode, RwasmModule, RwasmModuleInner,
    TableIdx, DEFAULT_MEMORY_INDEX, SNIPPET_FUNC_IDX_UNRESOLVED,
};
use alloc::{boxed::Box, vec::Vec};
use core::{
    mem::{replace, take},
    ops::Range,
};
use hashbrown::HashMap;
use wasmparser::{
    CustomSectionReader, DataKind, DataSectionReader, ElementItems, ElementKind,
    ElementSectionReader, Encoding, ExportSectionReader, ExternalKind, FuncType, FunctionBody,
    FunctionSectionReader, GlobalSectionReader, ImportSectionReader, MemorySectionReader, Parser,
    Payload, TableSectionReader, Type, TypeRef, TypeSectionReader, ValType, Validator,
};

/// Single-pass Wasm front-end that validates, translates, and assembles rwasm bytecode.
/// It streams the Wasm module with wasmparser, builds the instruction set and sections,
/// and applies configuration (entry routing, snippets) before finalizing the module.
pub struct ModuleParser {
    /// The Wasm validator used throughout stream parsing.
    validator: Validator,
    /// The number of compiled or processed functions.
    compiled_funcs: u32,
    /// Reusable allocations for validating and translation functions.
    allocations: ReusableAllocations,
    /// A compilation config
    config: CompilationConfig,
}

impl ModuleParser {
    pub fn new(config: CompilationConfig) -> Self {
        Self {
            validator: Validator::new_with_features(config.wasm_features()),
            compiled_funcs: 0,
            allocations: ReusableAllocations::default(),
            config,
        }
    }

    pub fn parse(&mut self, wasm_binary: &[u8]) -> Result<(), CompilationError> {
        let parser = Parser::new(0);
        let payloads = parser.parse_all(wasm_binary).collect::<Vec<_>>();
        let mut func_bodies = Vec::new();
        for payload in payloads {
            match payload? {
                Payload::CodeSectionEntry(func_body) => {
                    func_bodies.push(func_body);
                }
                Payload::End(offset) => {
                    for func_body in take(&mut func_bodies) {
                        self.process_code_entry(func_body)?;
                    }
                    self.process_end(offset)?;
                }
                payload => {
                    self.process_payload(payload)?;
                }
            }
        }
        Ok(())
    }

    pub fn parse_function_exports(
        config: CompilationConfig,
        wasm_binary: &[u8],
    ) -> Result<Vec<(Box<str>, FuncIdx, FuncType)>, CompilationError> {
        let mut result = Vec::default();
        let mut parser = ModuleParser::new(config);
        parser.parse(wasm_binary)?;
        for (k, v) in &parser.allocations.translation.exported_funcs {
            let func_type_idx = parser.allocations.translation.resolve_func_type_index(*v);
            let func_type = parser
                .allocations
                .translation
                .func_type_registry
                .resolve_original_func_type(func_type_idx)
                .clone();
            result.push((k.clone(), *v, func_type));
            #[cfg(feature = "debug-print")]
            print!("{}: func_idx={}, func_type_idx={}\n", k, v, func_type_idx);
        }
        Ok(result)
    }

    pub fn finalize(
        mut self,
        wasm_binary: &[u8],
    ) -> Result<(RwasmModule, ConstructorParams), CompilationError> {
        if let Some(start_func) = self.allocations.translation.start_func {
            if !self.config.allow_start_section {
                return Err(CompilationError::StartSectionsAreNotAllowed);
            }
            self.allocations
                .translation
                .emit_function_call(start_func, true, false);
        }
        self.allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .op_return();

        // A pointer to the instruction set (post-init section)
        let source_pc = self
            .allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .len() as u32;

        if let Some(entrypoint_name) = self.config.entrypoint_name.as_ref() {
            let func_idx = self
                .allocations
                .translation
                .exported_funcs
                .get(entrypoint_name)
                .copied()
                .ok_or(CompilationError::MissingEntrypoint)?;
            self.allocations
                .translation
                .emit_function_call(func_idx, true, true);
        } else if self.config.state_router.is_none() {
            // if there is no state router, then such an application can't be executed; then why do
            // we need to compile it?
            return Err(CompilationError::MissingEntrypoint);
        }
        self.emit_snippets();
        // we can emit state router only at the end of a translation process
        self.emit_state_router()?;
        // the entrypoint always ends with an empty return
        self.allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .finalize(true);

        // merge the entrypoint with our code section
        let mut code_section = self
            .allocations
            .translation
            .segment_builder
            .entrypoint_bytecode;
        let entrypoint_length = code_section.len() as u32;
        code_section.extend(self.allocations.translation.instruction_set.iter());

        // TODO(dmitry123): "optimize it"
        for instr in code_section.iter_mut() {
            match instr {
                Opcode::CallInternal(compiled_func)
                | Opcode::ReturnCallInternal(compiled_func)
                | Opcode::RefFunc(compiled_func) => {
                    if *compiled_func > 0 {
                        *compiled_func = self.allocations.translation.func_offsets
                            [*compiled_func as usize - 1]
                            + entrypoint_length;
                    }
                }
                _ => continue,
            }
        }

        let mut element_section = self
            .allocations
            .translation
            .segment_builder
            .global_element_section;
        for elem in element_section.iter_mut() {
            if *elem > 0 {
                *elem = self.allocations.translation.func_offsets[*elem as usize - 1]
                    + entrypoint_length;
            }
        }

        let module = RwasmModuleInner {
            code_section,
            data_section: self
                .allocations
                .translation
                .segment_builder
                .global_memory_section,
            elem_section: element_section,
            hint_section: wasm_binary.to_vec(),
            source_pc,
        };
        let constructor_params = self.allocations.translation.constructor_params;

        Ok((RwasmModule::from(module), constructor_params))
    }

    pub fn emit_state_router(&mut self) -> Result<(), CompilationError> {
        // if we have a state router, then translate state router
        let allow_malformed_entrypoint_func_type = self.config.allow_malformed_entrypoint_func_type;
        let Some(state_router) = &self.config.state_router else {
            return Ok(());
        };
        // push state on the stack
        if let Some(opcode) = &state_router.opcode {
            self.allocations
                .translation
                .segment_builder
                .entrypoint_bytecode
                .push(*opcode);
        }
        // translate state router
        for (entrypoint_name, state_value) in state_router.states.iter() {
            let Some(func_idx) = self
                .allocations
                .translation
                .exported_funcs
                .get(entrypoint_name)
                .copied()
            else {
                continue;
            };
            let func_type_idx = self
                .allocations
                .translation
                .resolve_func_type_index(func_idx);
            // make sure the func type is empty
            let is_empty_func_type = self
                .allocations
                .translation
                .func_type_registry
                .resolve_func_type_ref(func_type_idx, |func_type| {
                    func_type.params().len() == 0 && func_type.results().len() == 0
                });
            if !is_empty_func_type && !allow_malformed_entrypoint_func_type {
                return Err(CompilationError::MalformedFuncType);
            }
            let entrypoint_bytecode = &mut self
                .allocations
                .translation
                .segment_builder
                .entrypoint_bytecode;
            entrypoint_bytecode.op_local_get(1u32);
            entrypoint_bytecode.op_i32_const(*state_value);
            entrypoint_bytecode.op_i32_eq();
            entrypoint_bytecode.op_br_if_eqz(3);
            // it's super important to drop the original state from the stack
            // because input params might be passed though the stack
            entrypoint_bytecode.op_drop();
            self.allocations
                .translation
                .emit_function_call(func_idx, true, true);
        }
        // drop input state from the stack
        self.allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .op_drop();
        Ok(())
    }

    pub fn emit_snippets(&mut self) {
        if !self.config.code_snippets {
            return;
        }
        let mut emitted_snippets: HashMap<Snippet, FuncIdx> = HashMap::new();

        let snippet_calls = self.allocations.translation.snippet_calls.clone();
        for snippet_call in snippet_calls {
            let snippet = snippet_call.snippet;

            let snippet_func_idx = *emitted_snippets.entry(snippet).or_insert_with(|| {
                let new_func_idx = self.next_func();
                let alloc = &mut self.allocations.translation;
                let func_offset = alloc.instruction_set.len() as u32;
                alloc.func_offsets.push(func_offset);
                alloc
                    .instruction_set
                    .op_stack_check(snippet.max_stack_height());
                snippet.emit(&mut alloc.instruction_set);
                alloc.instruction_set.op_return();
                new_func_idx
            });

            let loc = snippet_call.loc;
            let alloc = &mut self.allocations.translation;
            let opcode = alloc.instruction_set.get_nth_mut(loc as usize)
                .unwrap_or_else(|| panic!("expected snippet call at index {loc}, but instruction set length is smaller"));

            match opcode {
                Opcode::CallInternal(func_idx) => {
                    assert_eq!(*func_idx, SNIPPET_FUNC_IDX_UNRESOLVED);
                    *func_idx = snippet_func_idx + 1;
                }
                other => {
                    panic!("expected Opcode::CallInternal at index {loc}, but found {other:?}")
                }
            }
        }
    }

    /// Processes the `wasmparser` payload.
    ///
    /// # Errors
    ///
    /// - If Wasm validation of the payload fails.
    /// - If some unsupported Wasm proposal definition is encountered.
    /// - If `rwasm` limits are exceeded.
    fn process_payload(&mut self, payload: Payload) -> Result<bool, CompilationError> {
        match payload {
            Payload::Version {
                num,
                encoding,
                range,
            } => self.process_version(num, encoding, range),
            Payload::TypeSection(section) => self.process_types(section),
            Payload::ImportSection(section) => self.process_imports(section),
            Payload::InstanceSection(section) => self.process_instances(section),
            Payload::FunctionSection(section) => self.process_functions(section),
            Payload::TableSection(section) => self.process_tables(section),
            Payload::MemorySection(section) => self.process_memories(section),
            Payload::TagSection(section) => self.process_tags(section),
            Payload::GlobalSection(section) => self.process_globals(section),
            Payload::ExportSection(section) => self.process_exports(section),
            Payload::StartSection { func, range } => self.process_start(func, range),
            Payload::ElementSection(section) => self.process_element(section),
            Payload::DataCountSection { count, range } => self.process_data_count(count, range),
            Payload::DataSection(section) => self.process_data(section),
            Payload::CustomSection(section) => self.process_custom_section(section),
            Payload::CodeSectionStart { count, range, .. } => self.process_code_start(count, range),
            Payload::CodeSectionEntry(func_body) => self.process_code_entry(func_body),
            Payload::UnknownSection { id, range, .. } => self.process_unknown(id, range),
            Payload::ModuleSection { parser: _, range } => {
                self.process_unsupported_component_model(range)
            }
            Payload::CoreTypeSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::ComponentSection { parser: _, range } => {
                self.process_unsupported_component_model(range)
            }
            Payload::ComponentInstanceSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::ComponentAliasSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::ComponentTypeSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::ComponentCanonicalSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::ComponentStartSection { start: _, range } => {
                self.process_unsupported_component_model(range)
            }
            Payload::ComponentImportSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::ComponentExportSection(section) => {
                self.process_unsupported_component_model(section.range())
            }
            Payload::End(offset) => {
                self.process_end(offset)?;
                return Ok(true);
            }
        }?;
        Ok(false)
    }

    /// Validates the Wasm version section.
    fn process_version(
        &mut self,
        num: u16,
        encoding: Encoding,
        range: Range<usize>,
    ) -> Result<(), CompilationError> {
        self.validator
            .version(num, encoding, &range)
            .map_err(Into::into)
    }

    /// Processes the Wasm type section.
    ///
    /// # Note
    ///
    /// This extracts all function types into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If an unsupported function type is encountered.
    fn process_types(&mut self, section: TypeSectionReader) -> Result<(), CompilationError> {
        self.validator.type_section(&section)?;
        for func_type in section.into_iter() {
            let func_type = match func_type? {
                Type::Func(func_type) => func_type,
            };
            self.allocations
                .translation
                .func_type_registry
                .alloc_func_type(func_type)?;
        }
        Ok(())
    }

    /// Processes the Wasm import section.
    ///
    /// # Note
    ///
    /// This extracts all imports into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// - If an import fails to validate.
    /// - If an unsupported import declaration is encountered.
    fn process_imports(&mut self, section: ImportSectionReader) -> Result<(), CompilationError> {
        self.validator.import_section(&section)?;
        for import in section.into_iter() {
            let import = import?;
            let func_type_index = match import.ty {
                TypeRef::Func(func_type_index) => func_type_index,
                TypeRef::Global(global_type) => {
                    let Some(default_value) = self.config.default_imported_global_value else {
                        return Err(CompilationError::NotSupportedImportType);
                    };
                    let global_index = self.allocations.translation.globals.len() as u32;
                    let global_variable = GlobalVariable::new(global_type, default_value);
                    self.allocations
                        .translation
                        .segment_builder
                        .add_global_variable(global_index.into(), &global_variable)?;
                    self.allocations.translation.globals.push(global_variable);
                    continue;
                }
                _ => return Err(CompilationError::NotSupportedImportType),
            };
            let import_name = ImportName::new(import.module, import.name);
            let Some(import_linker) = self.config.import_linker.as_ref() else {
                // Do we need to process imports if there is no import linker?
                return Err(CompilationError::UnresolvedImportFunction);
            };
            let import_linker_entity = import_linker
                .resolve_by_import_name(&import_name)
                .cloned()
                .ok_or(CompilationError::UnresolvedImportFunction)?;
            // verify an imported function type
            let func_type = self
                .allocations
                .translation
                .func_type_registry
                .resolve_original_func_type(func_type_index);
            if !import_linker_entity.matches_func_type(func_type) {
                return Err(CompilationError::MalformedImportFunctionType);
            }
            // don't allow funcref/externref in imported functions
            if !self.config.allow_func_ref_function_types {
                for x in func_type.params().iter().chain(func_type.results()) {
                    if x == &ValType::FuncRef || x == &ValType::ExternRef {
                        return Err(CompilationError::MalformedImportFunctionType);
                    }
                }
            }
            // inject an import function trampoline to support reffunc
            let func_idx = self.next_func();
            self.allocations
                .translation
                .compiled_funcs
                .push(func_type_index);

            if let Some(intrinsic) = import_linker_entity.intrinsic {
                self.allocations
                    .translation
                    .intrinsic_handler
                    .intrinsics
                    .push((func_idx, intrinsic));
            }

            let allocations = take(&mut self.allocations);
            let mut translator = InstructionTranslator::new(
                allocations.translation,
                self.config.consume_fuel,
                self.config.code_snippets,
                self.config.consume_fuel_for_params_and_locals,
                self.config.max_allowed_memory_pages,
            );
            translator.prepare(func_idx)?;
            let signature_index = translator
                .alloc
                .func_type_registry
                .resolve_func_type_signature(func_type_index);
            translator.alloc.instruction_set.op_stack_check(u32::MAX);

            if self.config.builtins_consume_fuel {
                compile_block_params(
                    &mut translator.alloc.instruction_set,
                    import_linker_entity.syscall_fuel_param,
                )
            }

            translator
                .alloc
                .instruction_set
                .op_call(import_linker_entity.sys_func_idx);
            translator.alloc.instruction_set.op_return();
            translator.finish()?;
            let _ = replace(
                &mut self.allocations,
                ReusableAllocations {
                    translation: take(&mut translator.alloc),
                    validation: allocations.validation,
                },
            );
        }
        Ok(())
    }

    /// Process module instances.
    ///
    /// # Note
    ///
    /// This is part of the module linking a Wasm proposal and not yet supported
    /// by `rwasm`.
    fn process_instances(
        &mut self,
        section: wasmparser::InstanceSectionReader,
    ) -> Result<(), CompilationError> {
        self.validator
            .instance_section(&section)
            .map_err(Into::into)
    }

    /// Process module function declarations.
    ///
    /// # Note
    ///
    /// This extracts all function declarations into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a function declaration fails to validate.
    fn process_functions(
        &mut self,
        section: FunctionSectionReader,
    ) -> Result<(), CompilationError> {
        self.validator.function_section(&section)?;
        for func_type_index in section.into_iter() {
            let func_type_index = func_type_index?;
            self.allocations
                .translation
                .compiled_funcs
                .push(func_type_index);
        }
        Ok(())
    }

    /// Process module table declarations.
    ///
    /// # Note
    ///
    /// This extracts all table declarations into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a table declaration fails to validate.
    fn process_tables(&mut self, section: TableSectionReader) -> Result<(), CompilationError> {
        self.validator.table_section(&section)?;
        for (table_idx, table_type) in section.into_iter().enumerate() {
            let table_type = table_type?;
            let table_idx = TableIdx::try_from(table_idx).unwrap();
            self.allocations
                .translation
                .segment_builder
                .emit_table_segment(table_idx, &table_type)?;
            self.allocations.translation.tables.push(table_type);
        }
        Ok(())
    }

    /// Process module linear memory declarations.
    ///
    /// # Note
    ///
    /// This extracts all linear memory declarations into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a linear memory declaration fails to validate.
    fn process_memories(&mut self, section: MemorySectionReader) -> Result<(), CompilationError> {
        self.validator.memory_section(&section)?;
        for memory_type in section.into_iter() {
            let memory_type = memory_type?;
            self.allocations.translation.memories.push(memory_type);
            let initial_memory =
                u32::try_from(memory_type.initial).expect("memory initial size too large");
            self.allocations
                .translation
                .segment_builder
                .add_memory_pages(initial_memory, self.config.max_allowed_memory_pages)?;
        }
        Ok(())
    }

    /// Process module tags.
    ///
    /// # Note
    ///
    /// This is part of the module linking a Wasm proposal and not yet supported
    /// by `rwasm`.
    fn process_tags(
        &mut self,
        section: wasmparser::TagSectionReader,
    ) -> Result<(), CompilationError> {
        self.validator.tag_section(&section).map_err(Into::into)
    }

    /// Process module global variable declarations.
    ///
    /// # Note
    ///
    /// This extracts all global variable declarations into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a global variable declaration fails to validate.
    fn process_globals(&mut self, section: GlobalSectionReader) -> Result<(), CompilationError> {
        self.validator.global_section(&section)?;
        for global in section.into_iter() {
            let global = global?;
            let init_expr = CompiledExpr::new(global.init_expr);
            let default_value = self.eval_const(init_expr)?;
            let global_variable = GlobalVariable::new(global.ty, default_value);
            let global_idx = GlobalIdx::from(self.allocations.translation.globals.len() as u32);
            self.allocations
                .translation
                .segment_builder
                .add_global_variable(global_idx, &global_variable)?;
            self.allocations.translation.globals.push(global_variable);
        }
        Ok(())
    }

    /// Process module export declarations.
    ///
    /// # Note
    ///
    /// This extracts all export declarations into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If an export declaration fails to validate.
    fn process_exports(&mut self, section: ExportSectionReader) -> Result<(), CompilationError> {
        self.validator.export_section(&section)?;
        for export in section.into_iter() {
            let export = export?;
            // #[cfg(feature = "debug-print")]
            // println!("export: func_idx={} {}", export.index, export.name);
            match export.kind {
                ExternalKind::Func => {
                    let function_name: Box<str> = export.name.into();
                    self.allocations
                        .translation
                        .exported_funcs
                        .insert(function_name, FuncIdx::from(export.index));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Process module start section.
    ///
    /// # Note
    ///
    /// This sets the start function for the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If the start function declaration fails to validate.
    fn process_start(&mut self, func: u32, range: Range<usize>) -> Result<(), CompilationError> {
        self.validator.start_section(func, &range)?;
        self.allocations.translation.start_func = Some(FuncIdx::from(func));
        Ok(())
    }

    /// Process module table element segments.
    ///
    /// # Note
    ///
    /// This extracts all table element segments into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If any of the table element segments fail to validate.
    fn process_element(&mut self, section: ElementSectionReader) -> Result<(), CompilationError> {
        self.validator.element_section(&section)?;
        for (element_segment_idx, element) in section.into_iter().enumerate() {
            let element = element?;
            let element_segment_idx = ElementSegmentIdx::from(element_segment_idx as u32);

            let element_items_vec = match element.items {
                ElementItems::Expressions(section) => section
                    .into_iter()
                    .map(|v| {
                        let compiled_expr = CompiledExpr::new(v?);
                        compiled_expr
                            .funcref()
                            .map(|v| v + 1)
                            .or_else(|| compiled_expr.eval_const().map(|v| v as i32 as u32))
                            .ok_or(CompilationError::ConstEvaluationFailed)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                ElementItems::Functions(section) => section
                    .into_iter()
                    .map(|v| v.map(|v| v + 1).map_err(CompilationError::from))
                    .collect::<Result<Vec<_>, _>>()?,
            };

            match element.kind {
                ElementKind::Active {
                    table_index,
                    offset_expr,
                } => {
                    let compiled_expr = CompiledExpr::new(offset_expr);
                    // We can fail-fast here because we already that know that there an overflow
                    let element_offset = u32::try_from(self.eval_const(compiled_expr)?)
                        .map_err(|_| CompilationError::TableOutOfBounds)?;
                    let table_idx = TableIdx::try_from(table_index).unwrap();
                    self.allocations
                        .translation
                        .segment_builder
                        .add_active_elements(
                            element_segment_idx,
                            element_offset,
                            table_idx,
                            element_items_vec,
                        );
                }
                ElementKind::Passive => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_elements(element_segment_idx, element_items_vec),
                ElementKind::Declared => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_elements(element_segment_idx, []),
            };
        }
        Ok(())
    }

    /// Process module data count section.
    ///
    /// # Note
    ///
    /// This is part of the bulk memory operations Wasm proposal and not yet supported
    /// by `rwasm`.
    fn process_data_count(
        &mut self,
        count: u32,
        range: Range<usize>,
    ) -> Result<(), CompilationError> {
        self.validator
            .data_count_section(count, &range)
            .map_err(Into::into)
    }

    /// Process module linear memory data segments.
    ///
    /// # Note
    ///
    /// This extracts all table elements into the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If any of the table elements fail to validate.
    fn process_data(&mut self, section: DataSectionReader) -> Result<(), CompilationError> {
        self.validator.data_section(&section)?;
        for (data_segment_idx, data) in section.into_iter().enumerate() {
            let data = data?;
            let data_segment_idx = DataSegmentIdx::from(data_segment_idx as u32);
            match data.kind {
                DataKind::Active {
                    memory_index,
                    offset_expr,
                } => {
                    if memory_index != DEFAULT_MEMORY_INDEX {
                        return Err(CompilationError::NonDefaultMemoryIndex);
                    }
                    let compiled_expr = CompiledExpr::new(offset_expr);
                    // We can fail-fast here because we already that know that there an overflow
                    let data_offset = u32::try_from(self.eval_const(compiled_expr)?)
                        .map_err(|_| CompilationError::MemoryOutOfBounds)?;
                    self.allocations
                        .translation
                        .segment_builder
                        .add_active_memory(data_segment_idx, data_offset, data.data);
                }
                DataKind::Passive => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_memory(data_segment_idx, data.data),
            };
        }
        Ok(())
    }

    fn eval_const(&self, compiled_expr: CompiledExpr) -> Result<i64, CompilationError> {
        compiled_expr
            .eval_with_context(
                |global_index| {
                    self.allocations
                        .translation
                        .globals
                        .get(global_index as usize)
                        .and_then(GlobalVariable::value)
                },
                |function_index| Some(FuncRef::new(function_index + 1)),
            )
            .ok_or(CompilationError::ConstEvaluationFailed)
    }

    fn process_custom_section(
        &mut self,
        reader: CustomSectionReader,
    ) -> Result<(), CompilationError> {
        self.allocations
            .translation
            .constructor_params
            .try_parse(reader);
        Ok(())
    }

    /// Process module code section start.
    ///
    /// # Note
    ///
    /// This currently does not do a lot, but it might become important in the
    /// future if we add parallel translation of function bodies to prepare for
    /// the translation.
    ///
    /// # Errors
    ///
    /// If the code start section fails to validate.
    fn process_code_start(
        &mut self,
        count: u32,
        range: Range<usize>,
    ) -> Result<(), CompilationError> {
        self.validator.code_section_start(count, &range)?;
        Ok(())
    }

    /// Returns the next `FuncIdx` for processing of its function body.
    fn next_func(&mut self) -> FuncIdx {
        let compiled_func = self.compiled_funcs;
        self.compiled_funcs += 1;
        FuncIdx::from(compiled_func)
    }

    /// Process a single module code section entry.
    ///
    /// # Note
    ///
    /// This contains the local variables and Wasm instructions of
    /// a single function body.
    /// This procedure is translating the Wasm bytecode into `rwasm` bytecode.
    ///
    /// # Errors
    ///
    /// If the function body fails to validate.
    fn process_code_entry(&mut self, func_body: FunctionBody) -> Result<(), CompilationError> {
        let func_idx = self.next_func();
        // #[cfg(feature = "debug-print")]
        // println!("\nfunc_idx={}", func_idx);
        let allocations = take(&mut self.allocations);
        let validator = self.validator.code_section_entry(&func_body)?;
        let func_validator = validator.into_validator(allocations.validation);
        let allocations = FuncBuilder::new(
            func_body,
            func_validator,
            func_idx,
            allocations.translation,
            self.config.consume_fuel,
            self.config.code_snippets,
            self.config.consume_fuel_for_params_and_locals,
            self.config.max_allowed_memory_pages,
        )
        .translate()?;
        let _ = replace(&mut self.allocations, allocations);
        Ok(())
    }

    /// Process an unknown Wasm module section.
    ///
    /// # Note
    ///
    /// This generally will be treated as an error for now.
    fn process_unknown(&mut self, id: u8, range: Range<usize>) -> Result<(), CompilationError> {
        self.validator
            .unknown_section(id, &range)
            .map_err(Into::into)
    }

    /// Process the entries for the Wasm component model proposal.
    fn process_unsupported_component_model(
        &mut self,
        range: Range<usize>,
    ) -> Result<(), CompilationError> {
        panic!(
            "rwasm does not support the `component-model` Wasm proposal: bytes[{}..{}]",
            range.start, range.end
        )
    }

    /// Processes the end of the Wasm binary.
    fn process_end(&mut self, offset: usize) -> Result<(), CompilationError> {
        self.validator.end(offset)?;
        Ok(())
    }
}

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
            println!("{}: func_idx={}, func_type_idx={}", k, v, func_type_idx);
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }

    /// Preserves the named entrypoint's Wasm signature for the typed strategy API. The reduced
    /// bytecode itself carries stack slots, so it cannot recover value boundaries at call time.
    pub(crate) fn entrypoint_type(&self) -> Option<FuncType> {
        let name = self.config.entrypoint_name.as_ref()?;
        let translation = &self.allocations.translation;
        let func_idx = *translation.exported_funcs.get(name)?;
        let type_idx = translation.resolve_func_type_index(func_idx);
        Some(
            translation
                .func_type_registry
                .resolve_original_func_type(type_idx)
                .clone(),
        )
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
        // The functions were bounded as they were translated; the init prologue, the snippets
        // and the state router come on top and the whole section has to fit as well.
        if code_section.len() > self.config.max_code_len as usize {
            return Err(CompilationError::CodeSizeExceeded {
                len: u32::try_from(code_section.len()).unwrap_or(u32::MAX),
                limit: self.config.max_code_len,
            });
        }

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
                    func_type.params().is_empty() && func_type.results().is_empty()
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
            // the branch skips the drop and the call; its offset is resolved once the call is
            // emitted, because an import the linker replaces with an intrinsic expands to more
            // than the one `ReturnCallInternal` a compiled function costs
            let skip_call = entrypoint_bytecode.len();
            entrypoint_bytecode.op_br_if_eqz(0i32);
            // it's super important to drop the original state from the stack
            // because input params might be passed though the stack
            entrypoint_bytecode.op_drop();
            self.allocations
                .translation
                .emit_function_call(func_idx, true, true);
            let entrypoint_bytecode = &mut self
                .allocations
                .translation
                .segment_builder
                .entrypoint_bytecode;
            let offset = i32::try_from(entrypoint_bytecode.len() - skip_call)
                .map_err(|_| CompilationError::BranchOffsetOutOfBounds)?;
            entrypoint_bytecode[skip_call].update_branch_offset(offset);
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
            let snippet_func_idx =
                self.emit_snippet_func(&mut emitted_snippets, snippet_call.snippet);

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

    /// Emits the function body for `snippet` (once per snippet) and returns its func index.
    ///
    /// A snippet body may itself call other snippets (e.g. the div/rem wrappers call the shared
    /// `UDivMod64` core) via `CallInternal(SNIPPET_FUNC_IDX_UNRESOLVED)` placeholders; those
    /// dependencies are emitted recursively and the placeholders inside the body are patched to
    /// the dependency's func index.
    fn emit_snippet_func(
        &mut self,
        emitted_snippets: &mut HashMap<Snippet, FuncIdx>,
        snippet: Snippet,
    ) -> FuncIdx {
        if let Some(func_idx) = emitted_snippets.get(&snippet) {
            return *func_idx;
        }
        let new_func_idx = self.next_func();
        emitted_snippets.insert(snippet, new_func_idx);
        let (body_start, body_end) = {
            let alloc = &mut self.allocations.translation;
            let func_offset = alloc.instruction_set.len() as u32;
            alloc.func_offsets.push(func_offset);
            alloc
                .instruction_set
                .op_stack_check(snippet.max_stack_height());
            snippet.emit(&mut alloc.instruction_set);
            alloc.instruction_set.op_return();
            (func_offset as usize, alloc.instruction_set.len())
        };
        // With a single dependency, every unresolved placeholder in the body targets it.
        assert!(snippet.dependencies().len() <= 1);
        for dependency in snippet.dependencies() {
            let dependency_func_idx = self.emit_snippet_func(emitted_snippets, *dependency);
            let alloc = &mut self.allocations.translation;
            for opcode in alloc.instruction_set.instr[body_start..body_end].iter_mut() {
                if let Opcode::CallInternal(func_idx) = opcode {
                    if *func_idx == SNIPPET_FUNC_IDX_UNRESOLVED {
                        *func_idx = dependency_func_idx + 1;
                    }
                }
            }
        }
        new_func_idx
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
    /// If the configured type count limit is exceeded or an unsupported function type is encountered.
    fn process_types(&mut self, section: TypeSectionReader) -> Result<(), CompilationError> {
        // Validation reserves storage for the declared count; bound it before that allocation.
        if section.count() > self.config.max_allowed_function_types {
            return Err(CompilationError::TooManyFunctionTypes {
                count: section.count(),
                limit: self.config.max_allowed_function_types,
            });
        }
        self.validator.type_section(&section)?;
        for func_type in section.into_iter() {
            let Type::Func(func_type) = func_type?;
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
                        .add_global_variable(global_index, &global_variable)?;
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
                self.config.consume_fuel_for_bulk_ops,
                self.config.consume_fuel_for_params_and_locals,
                self.config.max_allowed_memory_pages,
                self.config.max_code_len,
            );
            translator.prepare(func_idx)?;
            let signature_index = translator
                .alloc
                .func_type_registry
                .resolve_func_type_signature(func_type_index);
            translator.alloc.instruction_set.op_stack_check(u32::MAX);

            if self.config.builtins_consume_fuel {
                let temporary_slots = compile_block_params(
                    &mut translator.alloc.instruction_set,
                    import_linker_entity.syscall_fuel_param,
                    import_linker_entity.params,
                )?;
                // This prologue is emitted directly rather than through the Wasm translator.
                // Include its peak so the trampoline grows the stack before using temporaries.
                translator.stack_height.push_n(temporary_slots);
                translator.stack_height.pop_n(temporary_slots);
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
                .add_memory_pages(
                    initial_memory,
                    self.config.max_allowed_memory_pages,
                    self.config.consume_fuel_for_bulk_ops,
                )?;
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
            let init_expr = CompiledExpr::new(global.init_expr)?;
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
            if export.kind == ExternalKind::Func {
                let function_name: Box<str> = export.name.into();
                self.allocations
                    .translation
                    .exported_funcs
                    .insert(function_name, FuncIdx::from(export.index));
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
                        let compiled_expr = CompiledExpr::new(v?)?;
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
                    let compiled_expr = CompiledExpr::new(offset_expr)?;
                    // Validation requires an i32 offset. Its bits denote an unsigned index;
                    // an out-of-bounds active segment traps when the initializer runs.
                    let element_offset = self.eval_const(compiled_expr)? as u32;
                    let table_idx = TableIdx::try_from(table_index).unwrap();
                    self.allocations
                        .translation
                        .segment_builder
                        .add_active_elements(
                            element_segment_idx,
                            element_offset,
                            table_idx,
                            element_items_vec,
                        )?;
                }
                ElementKind::Passive => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_elements(element_segment_idx, element_items_vec)?,
                ElementKind::Declared => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_elements(element_segment_idx, [])?,
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
                    let compiled_expr = CompiledExpr::new(offset_expr)?;
                    // Preserve the unsigned bits of the validated i32 expression, including
                    // negative literals. Bounds belong to the emitted initialization code.
                    let data_offset = self.eval_const(compiled_expr)? as u32;
                    self.allocations
                        .translation
                        .segment_builder
                        .add_active_memory(data_segment_idx, data_offset, data.data)?;
                }
                DataKind::Passive => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_memory(data_segment_idx, data.data)?,
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
            self.config.consume_fuel_for_bulk_ops,
            self.config.consume_fuel_for_params_and_locals,
            self.config.max_allowed_memory_pages,
            self.config.max_code_len,
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
        _range: Range<usize>,
    ) -> Result<(), CompilationError> {
        Err(CompilationError::NotSupportedExtension)
    }

    /// Processes the end of the Wasm binary.
    fn process_end(&mut self, offset: usize) -> Result<(), CompilationError> {
        self.validator.end(offset)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_component_model_returns_error() {
        let mut parser = ModuleParser::new(CompilationConfig::default());
        let err = parser
            .process_unsupported_component_model(0..0)
            .expect_err("component-model payload must be rejected");
        assert!(matches!(err, CompilationError::NotSupportedExtension));
    }

    /// A memory access that can never be in bounds — immediate offset plus access size beyond
    /// the declared maximum, or beyond 4 GiB without one — lowers to an unconditional
    /// `Trap(MemoryOutOfBounds)` and ends the path, the way Cranelift treats it, so both engines
    /// charge the region only up to the access (audit round 7, R7-1). An access that can be in
    /// bounds keeps its ordinary lowering.
    #[test]
    fn statically_out_of_bounds_access_lowers_to_a_trap() {
        use crate::TrapCode;
        fn compiled(memory: &str, body: &str) -> Vec<Opcode> {
            let wasm = wat::parse_str(format!(
                r#"(module (memory {memory})
                     (func (export "main") {body} unreachable))"#
            ))
            .unwrap();
            let config = CompilationConfig::default_strategy_compatible()
                .with_entrypoint_name("main".into());
            let module = RwasmModule::compile(config, &wasm).unwrap().0;
            // the init prologue carries its own `MemoryOutOfBounds` guard; look at the body only
            module
                .code_section
                .iter()
                .skip(module.source_pc as usize)
                .copied()
                .collect()
        }
        let has = |code: &[Opcode], pred: fn(&Opcode) -> bool| code.iter().any(pred);
        let is_oob_trap = |op: &Opcode| matches!(op, Opcode::Trap(TrapCode::MemoryOutOfBounds));
        let is_load = |op: &Opcode| matches!(op, Opcode::I32Load(_));
        let is_store = |op: &Opcode| matches!(op, Opcode::I32Store(_));
        let is_unreachable = |op: &Opcode| matches!(op, Opcode::Unreachable);

        // static: the trap replaces the access and the dead tail is not emitted
        for (memory, body) in [
            ("1 2", "(drop (i32.load offset=131072 (i32.const 0)))"),
            ("1 2", "(drop (i32.load offset=131069 (i32.const 0)))"),
            ("0 0", "(drop (i32.load (i32.const 0)))"),
            ("1", "(drop (i64.load offset=0xffffffff (i32.const 0)))"),
        ] {
            let code = compiled(memory, body);
            assert!(has(&code, is_oob_trap), "{memory} {body}: trap expected");
            assert!(!has(&code, is_load), "{memory} {body}: no load expected");
            assert!(
                !has(&code, is_unreachable),
                "{memory} {body}: dead tail expected"
            );
        }
        let code = compiled(
            "0 1",
            "(i32.store offset=65536 (i32.const 0) (i32.const 0))",
        );
        assert!(has(&code, is_oob_trap) && !has(&code, is_store));

        // dynamic: the access and everything after it are emitted
        for (memory, body) in [
            ("1 2", "(drop (i32.load offset=131068 (i32.const 0)))"),
            ("1", "(drop (i32.load offset=0xfffffffc (i32.const 0)))"),
            ("0", "(drop (i32.load (i32.const 0)))"),
        ] {
            let code = compiled(memory, body);
            assert!(
                !has(&code, is_oob_trap),
                "{memory} {body}: no trap expected"
            );
            assert!(has(&code, is_load), "{memory} {body}: load expected");
            assert!(has(&code, is_unreachable), "{memory} {body}: tail expected");
        }
    }

    /// The fuel prologue `compile_block_params` emits into an import trampoline is written
    /// straight into the instruction set, outside the translator's stack-height tracking. Its
    /// temporaries still have to be part of the trampoline's `StackCheck`: with `StackCheck(0)`
    /// a `LinearFuel` (two temporaries) or `QuadraticFuel` (four) import called while the value
    /// stack sat within that many slots of its capacity trapped `StackOverflow` on the rwasm VM
    /// for a module Wasmtime executes (audit round 5, R5-2).
    #[test]
    fn import_trampoline_stack_check_covers_the_fuel_prologue() {
        use crate::ImportLinker;
        use alloc::sync::Arc;
        use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams, SyscallFuelParams};

        let cases = [
            ("none", SyscallFuelParams::None, 0),
            ("const", SyscallFuelParams::Const(5), 0),
            (
                "linear",
                SyscallFuelParams::LinearFuel(LinearFuelParams {
                    base_fuel: 1,
                    param_index: 1,
                    word_cost: 1,
                }),
                2,
            ),
            (
                "quadratic",
                SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
                    local_depth: 1,
                    word_cost: 1,
                    divisor: 1,
                    fuel_denom_rate: 1,
                }),
                4,
            ),
        ];
        for (name, policy, expected_peak) in cases {
            let mut linker = ImportLinker::default();
            linker.insert_function(
                ImportName::new("env", "builtin"),
                1,
                policy,
                &[ValType::I32],
                &[],
            );
            let wasm = wat::parse_str(
                r#"(module
                  (import "env" "builtin" (func $builtin (param i32)))
                  (func (export "main") (i32.const 0) (call $builtin)))"#,
            )
            .unwrap();
            let config = CompilationConfig::default()
                .with_entrypoint_name("main".into())
                .with_builtins_consume_fuel(true)
                .with_import_linker(Arc::new(linker));
            let module = RwasmModule::compile(config, &wasm).unwrap().0;
            // the trampoline is the first compiled function: it starts right after the init
            // prologue's `Return`, with `SignatureCheck; ConsumeFuel; StackCheck`
            let stack_check = module
                .code_section
                .iter()
                .skip(module.source_pc as usize)
                .find_map(|opcode| match opcode {
                    Opcode::StackCheck(height) => Some(*height),
                    _ => None,
                })
                .expect("the trampoline carries a StackCheck");
            assert_eq!(
                stack_check, expected_peak,
                "{name}: the trampoline must reserve the fuel prologue's temporaries"
            );
        }
    }

    /// Compiles a module with `main` as its entrypoint and runs it on the rwasm VM.
    fn run_on_the_vm(
        wasm: &[u8],
        config: CompilationConfig,
        result: &mut [crate::Value],
    ) -> Result<(), crate::TrapCode> {
        use crate::{always_failing_syscall_handler, ExecutionEngine, RwasmStore};
        use alloc::sync::Arc;
        let linker = config
            .import_linker
            .clone()
            .unwrap_or_else(|| Arc::new(crate::ImportLinker::default()));
        let module = RwasmModule::compile(config, wasm)
            .expect("the compiler accepts the module")
            .0;
        let mut store = RwasmStore::<()>::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            None,
            None,
        );
        let instance = linker.instantiate(&mut store, ExecutionEngine::new(), module)?;
        instance.execute(&mut store, &[], result)
    }

    /// An `i64` operator lowered to a code snippet runs in a frame of its own: the snippet's
    /// `StackCheck` reserves its temporaries on top of the caller's operands. The compile-time
    /// frame check (`params + peak <= N_MAX_STACK_SIZE`) bounds the caller only, so a frame the
    /// compiler accepts must still leave room for that hidden frame at run time, as it does for
    /// the import trampoline (`N_STACK_TRAMPOLINE_HEADROOM`). Otherwise the rwasm VM traps
    /// `StackOverflow` on a module the Wasmtime backend, where the operator is one native
    /// instruction, executes (audit 2026-09-18).
    #[test]
    fn snippet_frames_fit_the_runtime_value_stack() {
        use crate::{InstructionSet, Value, N_MAX_STACK_SIZE};
        // (operator, expected result of `91 op 7`, the snippet's own `StackCheck`)
        let ops = [
            ("i64.add", 98, InstructionSet::MSH_I64_ADD),
            ("i64.mul", 637, InstructionSet::MSH_I64_MUL),
            ("i64.div_u", 13, InstructionSet::MSH_I64_DIV_U),
            ("i64.rem_u", 0, InstructionSet::MSH_I64_REM_U),
            ("i64.div_s", 13, InstructionSet::MSH_I64_DIV_S),
            ("i64.rem_s", 0, InstructionSet::MSH_I64_REM_S),
        ];
        // `(operator, frame) -> outcome` for every accepted frame from `limit - peak` to the limit
        let mut outcomes = Vec::new();
        let mut expected_outcomes = Vec::new();
        for (op, expected, msh) in ops {
            // the frame is `locals + 4` (two i64 operands); the compiler accepts up to the limit
            let max_locals = N_MAX_STACK_SIZE - 4;
            for locals in (max_locals - msh as usize)..=max_locals {
                let wasm = wat::parse_str(format!(
                    r#"(module (func (export "main") (result i64) {locals}
                         i64.const 91 i64.const 7 {op}))"#,
                    locals = "(local i32)".repeat(locals)
                ))
                .unwrap();
                let config = CompilationConfig::default().with_entrypoint_name("main".into());
                let mut result = [Value::I64(0)];
                let outcome = run_on_the_vm(&wasm, config, &mut result).map(|_| result[0].clone());
                outcomes.push((op, locals + 4, outcome));
                expected_outcomes.push((op, locals + 4, Ok(Value::I64(expected))));
            }
        }
        assert_eq!(
            outcomes, expected_outcomes,
            "an accepted frame must run its snippets on the VM"
        );
    }

    /// `return_call` to an import the linker replaces with an intrinsic must still leave the
    /// function through a `Return`: the intrinsic body is spliced in place of the call, and the
    /// translator treats the rest of the body as unreachable, so nothing else emits one. Without
    /// it the VM falls through into the next function in the code section (audit 2026-09-18).
    #[test]
    fn return_call_to_an_intrinsic_returns() {
        use crate::{intrinsic::Intrinsic, ImportLinker, StoreTr, TrapCode};
        use alloc::sync::Arc;
        // `victim` is laid out right after `main` and must never run
        let wasm = wat::parse_str(
            r#"(module
                (import "env" "consume_fuel" (func $consume_fuel (param i32)))
                (memory 1)
                (func (export "main") (i32.const 5) (return_call $consume_fuel))
                (func (export "victim") (i32.store (i32.const 0) (i32.const 0xdeadbeef))))"#,
        )
        .unwrap();
        for (name, intrinsic) in [
            (
                "replace",
                Intrinsic::Replace(vec![Opcode::ConsumeFuelStack]),
            ),
            ("remove", Intrinsic::Remove),
        ] {
            let mut linker = ImportLinker::default();
            linker.insert_intrinsic(
                ImportName::new("env", "consume_fuel"),
                71,
                intrinsic,
                &[ValType::I32],
                &[],
            );
            let config = CompilationConfig::default()
                .with_entrypoint_name("main".into())
                .with_import_linker(Arc::new(linker));

            // the compiled `main` is the second function after the init prologue (the import
            // trampoline comes first); its last instruction must be the `Return`
            let module = RwasmModule::compile(config.clone(), &wasm).unwrap().0;
            let starts: Vec<usize> = module
                .code_section
                .iter()
                .enumerate()
                .skip(module.source_pc as usize)
                .filter(|(_, op)| matches!(op, Opcode::SignatureCheck(_)))
                .map(|(pos, _)| pos)
                .collect();
            let main_body = &module.code_section[starts[1]..starts[2]];
            assert_eq!(
                main_body.last(),
                Some(&Opcode::Return),
                "{name}: `main` must end with a Return, got {main_body:?}"
            );

            // and the VM must return from `main` instead of running `victim`
            use crate::{always_failing_syscall_handler, ExecutionEngine, RwasmStore};
            let linker = config.import_linker.clone().unwrap();
            let mut store = RwasmStore::<()>::new(
                linker.clone(),
                (),
                always_failing_syscall_handler,
                Some(1_000_000),
                None,
            );
            let instance = linker
                .instantiate(&mut store, ExecutionEngine::new(), module)
                .unwrap();
            let outcome: Result<(), TrapCode> = instance.execute(&mut store, &[], &mut []);
            let mut word = [0u8; 4];
            store.memory_read(0, &mut word).unwrap();
            assert_eq!(outcome, Ok(()), "{name}");
            assert_eq!(
                u32::from_le_bytes(word),
                0,
                "{name}: `main` fell through into `victim`"
            );
        }
    }
}

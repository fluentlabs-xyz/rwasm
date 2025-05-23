use crate::{
    compiler::{func_builder::FuncBuilder, translator::ReusableAllocations},
    CompilationConfig,
    CompilationError,
    CompiledExpr,
    DataSegmentIdx,
    DropKeep,
    ElementSegmentIdx,
    FuncIdx,
    FuncTypeIdx,
    GlobalIdx,
    GlobalVariable,
    ImportName,
    Opcode,
    OpcodeData,
    RwasmModule,
    TableIdx,
    UntypedValue,
    DEFAULT_MEMORY_INDEX,
};
use core::ops::Range;
use std::mem::{replace, take};
use wasmparser::{
    BinaryReaderError,
    DataKind,
    DataSectionReader,
    ElementItems,
    ElementKind,
    ElementSectionReader,
    Encoding,
    ExportSectionReader,
    ExternalKind,
    FunctionBody,
    FunctionSectionReader,
    GlobalSectionReader,
    ImportSectionReader,
    MemorySectionReader,
    Parser,
    Payload,
    TableSectionReader,
    Type,
    TypeRef,
    TypeSectionReader,
    Validator,
};

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
            allocations: Default::default(),
            config,
        }
    }

    pub fn parse(&mut self, wasm_binary: &[u8]) -> Result<(), CompilationError> {
        let parser = Parser::new(0);
        let payloads = parser.parse_all(wasm_binary).collect::<Vec<_>>();
        for payload in payloads {
            self.process_payload(payload)?;
        }
        Ok(())
    }

    pub fn finalize(mut self) -> Result<RwasmModule, CompilationError> {
        if let Some(start_func) = self.allocations.translation.start_func {
            self.allocations
                .translation
                .segment_builder
                .add_start_function(start_func.to_u32());
        } else if let Some(entrypoint_name) = self.config.entrypoint_name.as_ref() {
            let func_idx = self
                .allocations
                .translation
                .exported_funcs
                .get(entrypoint_name)
                .ok_or(CompilationError::MissingEntrypoint)?;
            self.allocations
                .translation
                .segment_builder
                .add_start_function(func_idx.to_u32());
        } else if self.config.state_router.is_none() {
            // if there is no state router, then such an application can't be executed; then why do
            // we need to compile it?
            return Err(CompilationError::MissingEntrypoint);
        }
        // we can emit state router only at the end of a translation process
        self.emit_state_router()?;
        // the entrypoint always ends with an empty return
        self.allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .op_return(DropKeep::none());

        // merge the entrypoint with our code section
        let mut code_section = self
            .allocations
            .translation
            .segment_builder
            .entrypoint_bytecode;
        let entrypoint_length = code_section.len() as u32;
        code_section.extend(self.allocations.translation.instruction_set.iter());

        let mut func_section = self.allocations.translation.func_offsets;
        for offset in func_section.iter_mut().skip(1) {
            // make sure each function offset is adjusted by an entrypoint length
            // since entrypoint is always the first function in our bytecode
            *offset += entrypoint_length;
        }

        // we store source pc here to keep compatibility with the oldest version of rWasm
        // where entrypoint was the last function inside function offsets (can be removed later)
        let source_pc = func_section.first().copied().unwrap();
        debug_assert_eq!(source_pc, 0);

        // now we can rewrite all calls internal offset
        for x in code_section.iter_mut() {
            match x {
                (Opcode::CallInternal, OpcodeData::CompiledFunc(compiled_func)) => {
                    // +1 because of the entrypoint
                    *compiled_func += 1;
                    // *compiled_func += func_section.get(*compiled_func as usize + 1).unwrap();
                }
                _ => {}
            }
        }

        Ok(RwasmModule {
            code_section,
            memory_section: self
                .allocations
                .translation
                .segment_builder
                .global_memory_section,
            element_section: self
                .allocations
                .translation
                .segment_builder
                .global_element_section,
            source_pc,
            func_section,
        })
    }

    pub fn emit_state_router(&mut self) -> Result<(), CompilationError> {
        // if we have a state router, then translate state router
        let allow_malformed_entrypoint_func_type = self.config.allow_malformed_entrypoint_func_type;
        let Some(state_router) = &self.config.state_router else {
            return Ok(());
        };
        // push state on the stack
        self.allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .push(state_router.opcode.0, state_router.opcode.1);
        // translate state router
        for (entrypoint_name, state_value) in state_router.states.iter() {
            let func_idx = self
                .allocations
                .translation
                .exported_funcs
                .get(entrypoint_name)
                .copied()
                .ok_or(CompilationError::MissingEntrypoint)?;
            let func_type_idx = self
                .allocations
                .translation
                .resolve_func_type_index(func_idx);
            // make sure the func type is empty
            let is_empty_func_type = self
                .allocations
                .translation
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
            entrypoint_bytecode.op_local_get(1);
            entrypoint_bytecode.op_i32_const(*state_value);
            entrypoint_bytecode.op_i32_eq();
            entrypoint_bytecode.op_br_if_eqz(4);
            // it's super important to drop the original state from the stack
            // because input params might be passed though the stack
            entrypoint_bytecode.op_drop();
            entrypoint_bytecode.op_call_internal(func_idx.to_u32());
            entrypoint_bytecode.op_return(DropKeep::none());
        }
        // drop input state from the stack
        self.allocations
            .translation
            .segment_builder
            .entrypoint_bytecode
            .op_drop();
        Ok(())
    }

    /// Processes the `wasmparser` payload.
    ///
    /// # Errors
    ///
    /// - If Wasm validation of the payload fails.
    /// - If some unsupported Wasm proposal definition is encountered.
    /// - If `rwasm` limits are exceeded.
    fn process_payload(
        &mut self,
        payload: Result<Payload, BinaryReaderError>,
    ) -> Result<bool, CompilationError> {
        match payload? {
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
            Payload::CustomSection { .. } => Ok(()),
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
            self.allocations.translation.func_types.push(func_type);
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
                .func_types
                .get(func_type_index as usize)
                .expect("missing function type");
            if !import_linker_entity.matches_func_type(func_type) {
                return Err(CompilationError::MalformedImportFunctionType);
            }
            // add import to the module
            self.allocations
                .translation
                .imported_funcs
                .push((import_linker_entity, func_type_index as FuncTypeIdx));
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
        for table_type in section.into_iter() {
            let table_type = table_type?;
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
                .add_memory_pages(initial_memory)?;
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
            let global_variable = GlobalVariable {
                global_type: global.ty,
                init_expr,
            };
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
                            .eval_const()
                            .map(UntypedValue::as_u32)
                            .ok_or(CompilationError::ConstEvaluationFailed)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                ElementItems::Functions(section) => section
                    .into_iter()
                    .map(|v| v.map_err(CompilationError::from))
                    .collect::<Result<Vec<_>, _>>()?,
            };

            match element.kind {
                ElementKind::Active {
                    table_index,
                    offset_expr,
                } => {
                    let compiled_expr = CompiledExpr::new(offset_expr);
                    let element_offset = compiled_expr
                        .eval_const()
                        .ok_or(CompilationError::ConstEvaluationFailed)?;
                    let table_index = TableIdx::from(table_index);
                    self.allocations
                        .translation
                        .segment_builder
                        .add_active_elements(
                            element_segment_idx,
                            element_offset,
                            table_index,
                            element_items_vec,
                        );
                }
                ElementKind::Passive | ElementKind::Declared => self
                    .allocations
                    .translation
                    .segment_builder
                    .add_passive_elements(element_segment_idx, element_items_vec),
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
                    let data_offset = compiled_expr
                        .eval_const()
                        .ok_or(CompilationError::ConstEvaluationFailed)?;
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
        // We have to adjust the initial func reference to the first
        // internal function before we process any of the internal functions.
        let len_func_imports = u32::try_from(self.allocations.translation.imported_funcs.len())
            .unwrap_or_else(|_| panic!("too many imported functions"));
        FuncIdx::from(compiled_func + len_func_imports)
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
        let allocations = take(&mut self.allocations);
        let validator = self.validator.code_section_entry(&func_body)?;
        let func_validator = validator.into_validator(allocations.validation);
        let func_builder =
            FuncBuilder::new(func_body, func_validator, func_idx, allocations.translation);
        let allocations = func_builder.translate()?;
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

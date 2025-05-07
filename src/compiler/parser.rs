use crate::{
    compiler::{builder::ModuleBuilder, error::CompilerError, CompilerConfig},
    RwasmModule,
};
use alloc::{boxed::Box, vec::Vec};
use core::{
    mem::{replace, take},
    ops::Range,
};
use wasmparser::{
    DataSectionReader,
    ElementSectionReader,
    Encoding,
    ExportSectionReader,
    FuncValidatorAllocations,
    FunctionBody,
    FunctionSectionReader,
    GlobalSectionReader,
    ImportSectionReader,
    MemorySectionReader,
    Parser as WasmParser,
    Payload,
    TableSectionReader,
    Type,
    TypeSectionReader,
    Validator,
};

/// Parses and validates the given Wasm bytecode stream.
///
/// Returns the compiled and validated Wasm [`Module`] upon success.
/// Uses the given [`Engine`] as the translation target of the process.
///
/// # Errors
///
/// If the Wasm bytecode stream fails to validate.
pub fn parse(config: &CompilerConfig, stream: &[u8]) -> Result<RwasmModule, CompilerError> {
    ModuleParser::new(config).parse(stream)
}

/// Context used to construct a WebAssembly module from a stream of bytes.
pub struct ModuleParser<'engine> {
    config: &'engine CompilerConfig,
    /// The module builder used throughout stream parsing.
    builder: ModuleBuilder<'engine>,
    /// The Wasm validator used throughout stream parsing.
    validator: Validator,
    /// The number of compiled or processed functions.
    compiled_funcs: u32,
    /// Reusable allocations for validating and translation functions.
    allocations: ReusableAllocations,
}

/// Reusable heap allocations for function validation and translation.
#[derive(Default)]
pub struct ReusableAllocations {
    pub translation: FuncTranslatorAllocations,
    pub validation: FuncValidatorAllocations,
}

impl<'a> ModuleParser<'a> {
    /// Creates a new [`ModuleParser`] for the given [`Engine`].
    fn new(config: &'a CompilerConfig) -> Self {
        let validator = Validator::new_with_features(config.wasm_features());
        Self {
            config,
            builder: ModuleBuilder::new(config),
            validator,
            compiled_funcs: 0,
            allocations: ReusableAllocations::default(),
        }
    }

    /// Starts parsing and validating the Wasm bytecode stream.
    ///
    /// Returns the compiled and validated Wasm [`Module`] upon success.
    ///
    /// # Errors
    ///
    /// If the Wasm bytecode stream fails to validate.
    pub fn parse(mut self, stream: &'a [u8]) -> Result<RwasmModule, CompilerError> {
        let mut func_bodies: Vec<FunctionBody> = Vec::new();
        let parser = WasmParser::new(0);
        let payloads = parser.parse_all(stream).collect::<Vec<_>>();
        for payload in payloads {
            let payload = payload?;
            match payload {
                Payload::CodeSectionEntry(func_body) => {
                    func_bodies.push(func_body);
                }
                Payload::End(offset) => {
                    // before processing code entries, we must process an entrypoint
                    let entrypoint_instr_builder = self.process_rwasm_entrypoint()?;
                    for func_body in take(&mut func_bodies) {
                        self.process_code_entry(func_body)?;
                    }
                    // rewrite memory and table sections for rWASM (encoding/decoding simulation)
                    let (mut instr_builder, compiled_func) = entrypoint_instr_builder;
                    instr_builder.finish(self.builder.engine(), compiled_func, 0, 0)?;
                    self.rewrite_sections()?;
                    self.process_end(offset)?;
                }
                Payload::CustomSection(reader) => {
                    self.builder
                        .custom_sections
                        .push(reader.name(), reader.data());
                }
                _ => {
                    self.process_payload(payload)?;
                }
            }
        }
        Ok(self.builder.finish())
    }

    /// Processes the `wasmparser` payload.
    ///
    /// # Errors
    ///
    /// - If Wasm validation of the payload fails.
    /// - If some unsupported Wasm proposal definition is encountered.
    /// - If `wasmi` limits are exceeded.
    fn process_payload(&mut self, payload: Payload<'a>) -> Result<bool, CompilerError> {
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

    /// Processes the end of the Wasm binary.
    fn process_end(&mut self, offset: usize) -> Result<(), CompilerError> {
        self.validator.end(offset)?;
        Ok(())
    }

    /// Validates the Wasm version section.
    fn process_version(
        &mut self,
        num: u16,
        encoding: Encoding,
        range: Range<usize>,
    ) -> Result<(), CompilerError> {
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
    fn process_types(&mut self, section: TypeSectionReader) -> Result<(), CompilerError> {
        self.validator.type_section(&section)?;
        let func_types = section.into_iter().map(|v| {
            v.map(|v| match v {
                Type::Func(func_type) => func_type,
            })
            .map_err(CompilerError::from)
        });
        self.builder.push_func_types(func_types)?;
        self.builder.ensure_empty_func_type_exists();
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
    fn process_imports(&mut self, section: ImportSectionReader) -> Result<(), CompilerError> {
        self.validator.import_section(&section)?;
        let imports = section
            .into_iter()
            .map(|import| import.map_err(CompilerError::from));
        self.builder.push_imports(imports)?;
        Ok(())
    }

    /// Process module instances.
    ///
    /// # Note
    ///
    /// This is part of the module linking Wasm proposal and not yet supported
    /// by `wasmi`.
    fn process_instances(
        &mut self,
        section: wasmparser::InstanceSectionReader,
    ) -> Result<(), CompilerError> {
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
    fn process_functions(&mut self, section: FunctionSectionReader) -> Result<(), CompilerError> {
        self.validator.function_section(&section)?;
        let funcs = section
            .into_iter()
            .map(|func| func.map_err(CompilerError::from));
        self.builder.push_funcs(funcs)?;
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
    fn process_tables(&mut self, section: TableSectionReader) -> Result<(), CompilerError> {
        self.validator.table_section(&section)?;
        let tables = section
            .into_iter()
            .map(|table| table.map_err(CompilerError::from));
        self.builder.push_tables(tables)?;
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
    fn process_memories(&mut self, section: MemorySectionReader) -> Result<(), CompilerError> {
        self.validator.memory_section(&section)?;
        let memories = section
            .into_iter()
            .map(|memory| memory.map_err(CompilerError::from));
        self.builder.push_memories(memories)?;
        Ok(())
    }

    /// Process module tags.
    ///
    /// # Note
    ///
    /// This is part of the module linking Wasm proposal and not yet supported
    /// by `wasmi`.
    fn process_tags(&mut self, section: wasmparser::TagSectionReader) -> Result<(), CompilerError> {
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
    fn process_globals(&mut self, section: GlobalSectionReader) -> Result<(), CompilerError> {
        self.validator.global_section(&section)?;
        let globals = section
            .into_iter()
            .map(|global| global.map_err(CompilerError::from));
        self.builder.push_globals(globals)?;
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
    fn process_exports(&mut self, section: ExportSectionReader) -> Result<(), CompilerError> {
        self.validator.export_section(&section)?;
        let exports = section
            .into_iter()
            .map(|export| export.map_err(CompilerError::from));
        self.builder.push_exports(exports)?;
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
    fn process_start(&mut self, func: u32, range: Range<usize>) -> Result<(), CompilerError> {
        self.validator.start_section(func, &range)?;
        self.builder.set_start(FuncIdx::from(func));
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
    fn process_element(&mut self, section: ElementSectionReader) -> Result<(), CompilerError> {
        self.validator.element_section(&section)?;
        let segments = section.into_iter().map(|segment| {
            segment
                .map(ElementSegment::from)
                .map_err(CompilerError::from)
        });
        self.builder.push_element_segments(segments)?;
        Ok(())
    }

    /// Process module data count section.
    ///
    /// # Note
    ///
    /// This is part of the bulk memory operations Wasm proposal and not yet supported
    /// by `wasmi`.
    fn process_data_count(&mut self, count: u32, range: Range<usize>) -> Result<(), CompilerError> {
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
    fn process_data(&mut self, section: DataSectionReader) -> Result<(), CompilerError> {
        self.validator.data_section(&section)?;
        let segments = section
            .into_iter()
            .map(|segment| segment.map_err(CompilerError::from));
        self.builder.push_data_segments(segments)?;
        Ok(())
    }

    /// Process module code section start.
    ///
    /// # Note
    ///
    /// This currently does not do a lot but it might become important in the
    /// future if we add parallel translation of function bodies to prepare for
    /// the translation.
    ///
    /// # Errors
    ///
    /// If the code start section fails to validate.
    fn process_code_start(&mut self, count: u32, range: Range<usize>) -> Result<(), CompilerError> {
        self.validator.code_section_start(count, &range)?;
        Ok(())
    }

    /// Returns the next `FuncIdx` for processing of its function body.
    fn next_func(&mut self) -> (FuncIdx, CompiledFunc) {
        let index = self.compiled_funcs;
        let compiled_func = self.builder.compiled_funcs[index as usize];
        self.compiled_funcs += 1;
        let func_idx = if self.builder.engine().config().get_rwasm_wrap_import_funcs() {
            // We don't have to adjust the initial func reference because we replace
            // imported functions with compiled func wrapper in rWASM
            FuncIdx::from(index)
        } else {
            // We have to adjust the initial func reference to the first
            // internal function before we process any of the internal functions.
            let len_func_imports = u32::try_from(self.builder.imports.funcs.len())
                .unwrap_or_else(|_| panic!("too many imported functions"));
            FuncIdx::from(index + len_func_imports)
        };
        (func_idx, compiled_func)
    }

    /// Process a single module code section entry.
    ///
    /// # Note
    ///
    /// This contains the local variables and Wasm instructions of
    /// a single function body.
    /// This procedure is translating the Wasm bytecode into `wasmi` bytecode.
    ///
    /// # Errors
    ///
    /// If the function body fails to validate.
    fn process_code_entry(&mut self, func_body: FunctionBody) -> Result<(), CompilerError> {
        let (func, compiled_func) = self.next_func();
        let mut allocations = take(&mut self.allocations);
        let validator = self.validator.code_section_entry(&func_body)?;
        let module_resources = ModuleResources::new(&self.builder);
        allocations = translate(
            func,
            compiled_func,
            func_body.clone(),
            validator.into_validator(allocations.validation),
            module_resources,
            allocations.translation,
            self.builder.engine().config().get_i32_translator(),
        )?;
        let _ = replace(&mut self.allocations, allocations);
        Ok(())
    }

    fn process_rwasm_entrypoint(
        &mut self,
    ) -> Result<(InstructionsBuilder, CompiledFunc), CompilerError> {
        // we must register new compiled func for an entrypoint
        let (func, compiled_func) = self.builder.push_entrypoint();
        let allocations = take(&mut self.allocations);
        // translate entrypoint
        let mut func_allocations = RwasmTranslator::new(
            func,
            compiled_func,
            ModuleResources::new(&self.builder),
            allocations.translation,
            self.builder.engine().config().get_i32_translator(),
        )
        .translate_entrypoint()?;
        let instr_builder = take(&mut func_allocations.inst_builder);
        let _ = replace(
            &mut self.allocations,
            ReusableAllocations {
                translation: func_allocations,
                validation: allocations.validation,
            },
        );
        let has_entrypoint = self
            .builder
            .engine()
            .config()
            .get_rwasm_config()
            .map(|rwasm_config| rwasm_config.entrypoint_name.is_some())
            .unwrap_or_default();
        if has_entrypoint {
            self.builder.rewrite_exports("main".into(), func)?;
        }
        Ok((instr_builder, compiled_func))
    }

    fn rewrite_sections(&mut self) -> Result<(), CompilerError> {
        self.builder.rewrite_tables()?;
        self.builder.rewrite_memory()?;
        Ok(())
    }

    /// Process the entries for the Wasm component model proposal.
    fn process_unsupported_component_model(
        &mut self,
        range: Range<usize>,
    ) -> Result<(), CompilerError> {
        panic!(
            "wasmi does not support the `component-model` Wasm proposal: bytes[{}..{}]",
            range.start, range.end
        )
    }

    /// Process an unknown Wasm module section.
    ///
    /// # Note
    ///
    /// This generally will be treated as an error for now.
    fn process_unknown(&mut self, id: u8, range: Range<usize>) -> Result<(), CompilerError> {
        self.validator
            .unknown_section(id, &range)
            .map_err(Into::into)
    }
}

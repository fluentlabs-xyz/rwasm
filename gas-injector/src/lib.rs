mod cost_model;
#[cfg(test)]
mod tests;

pub use crate::cost_model::{CostModel, GasMeter, ShouldInject};
use wasm_encoder::reencode::{Error, Reencode, RoundtripReencoder};
use wasm_encoder::{
    CodeSection, ElementSection, Encode, ExportSection, ImportSection, InstructionSink, Section,
    SectionId, TypeSection,
};
use wasmparser::{
    BinaryReaderError, Chunk, ElementKind, ElementSectionReader, ExportSectionReader, ExternalKind,
    FunctionBody, ImportSectionReader, Operator, Parser, Payload, TypeSectionReader, ValType,
};

pub const BASE_FUEL_COST: u32 = 1;
pub const ENTITY_FUEL_COST: u32 = 1;
pub const LOAD_FUEL_COST: u32 = 1;
pub const STORE_FUEL_COST: u32 = 1;
pub const CALL_FUEL_COST: u32 = 1;

pub const MEMORY_BYTES_PER_FUEL: u32 = 64;
pub const MEMORY_BYTES_PER_FUEL_LOG2: u32 = 6;
pub const TABLE_ELEMS_PER_FUEL: u32 = 16;
pub const TABLE_ELEMS_PER_FUEL_LOG2: u32 = 4;
pub const LOCALS_PER_FUEL: u32 = 16;
pub const LOCALS_PER_FUEL_LOG2: u32 = 4;
pub const DROP_KEEP_PER_FUEL: u32 = 16;
pub const DROP_KEEP_PER_FUEL_LOG2: u32 = 4;

#[derive(Debug, Clone)]
pub struct GasInjectorConfig {
    charge_gas_func_name: (&'static str, &'static str),
}

pub struct GasInjector<T: CostModel> {
    config: GasInjectorConfig,
    gas_meter: GasMeter<T>,
    charge_gas_func_type_idx: Option<u32>,
    charge_gas_func_idx: Option<u32>,
    remap_func_indices: bool,
    code_section: Option<CodeSection>,
    wasm_output: Vec<u8>,
}

impl<T: CostModel> GasInjector<T> {
    pub fn new(config: GasInjectorConfig, cost_model: T) -> Self {
        Self {
            config,
            gas_meter: GasMeter::new(cost_model),
            charge_gas_func_type_idx: None,
            charge_gas_func_idx: None,
            remap_func_indices: false,
            code_section: None,
            wasm_output: Vec::new(),
        }
    }

    pub fn inject(mut self, wasm_binary: &[u8]) -> Result<Vec<u8>, BinaryReaderError> {
        let mut parser = Parser::new(0);
        let mut data = &wasm_binary[..];
        loop {
            match parser.parse(&data, true)? {
                Chunk::NeedMoreData(_) => {
                    // this is not possible since eof is unreachable
                    unreachable!()
                }
                Chunk::Parsed { consumed, payload } => {
                    let should_break = if let Payload::End(_) = payload {
                        true
                    } else {
                        false
                    };
                    self.process_payload(payload, &data, consumed)?;
                    data = &data[consumed..];
                    if should_break {
                        break;
                    }
                }
            };
        }
        Ok(self.wasm_output)
    }

    fn process_payload(
        &mut self,
        payload: Payload,
        data: &[u8],
        consumed_bytes: usize,
    ) -> Result<(), BinaryReaderError> {
        let section_id = match payload {
            Payload::CodeSectionEntry(_) => SectionId::Code as u8,
            _ => payload.as_section().map(|section| section.0).unwrap_or(0),
        };
        // if we're missing type/import sections, then forcibly inject them
        if section_id > SectionId::Type as u8 && self.charge_gas_func_type_idx.is_none() {
            self.process_type_section(None)?;
        } else if section_id > SectionId::Import as u8 && self.charge_gas_func_idx.is_none() {
            self.process_import_section(None)?;
        }
        // if we've passed a code section, then commit it
        if section_id != SectionId::Code as u8 && self.code_section.is_some() {
            let code_section = self.code_section.take().unwrap();
            self.wasm_output.push(SectionId::Code as u8);
            code_section.encode(&mut self.wasm_output);
        }
        // process the payload
        match payload {
            Payload::TypeSection(reader) => self.process_type_section(Some(reader))?,
            Payload::ImportSection(reader) => self.process_import_section(Some(reader))?,
            Payload::ExportSection(reader) => self.process_export_section(reader)?,
            Payload::ElementSection(reader) => self.process_elem_section(reader)?,
            Payload::CodeSectionStart { .. } => {
                self.code_section = Some(CodeSection::new());
            }
            Payload::CodeSectionEntry(function_body) => {
                self.process_code_section_entry(function_body)?
            }
            Payload::CustomSection(_) => {
                // ignore custom sections
            }
            _ => self.wasm_output.extend(&data[..consumed_bytes]),
        }
        Ok(())
    }

    fn process_type_section(
        &mut self,
        type_section_reader: Option<TypeSectionReader>,
    ) -> Result<(), BinaryReaderError> {
        let mut type_section = TypeSection::new();
        // try to find a type with the same function name
        let mut func_type_idx = 0u32;
        if let Some(type_section_reader) = type_section_reader {
            for group in type_section_reader.into_iter() {
                for sub_type in group?.into_types() {
                    // remember func type index only if it's not assigned yet
                    if self.charge_gas_func_type_idx.is_none()
                        && sub_type.unwrap_func().params() == &[ValType::I32]
                        && sub_type.unwrap_func().results() == &[]
                    {
                        self.charge_gas_func_type_idx = Some(func_type_idx);
                    }
                    func_type_idx += 1;
                    // encode func type
                    let sub_type = sub_type.try_into().unwrap();
                    type_section.ty().subtype(&sub_type);
                }
            }
        }
        // if func type is not found then create the func type
        if self.charge_gas_func_type_idx.is_none() {
            let func_type: wasm_encoder::FuncType =
                wasm_encoder::FuncType::new([wasm_encoder::ValType::I32], []);
            type_section.ty().subtype(&wasm_encoder::SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Func(func_type),
                    shared: false,
                },
            });
            self.charge_gas_func_type_idx = Some(func_type_idx);
        }
        // encode a type section into wasm output
        self.wasm_output.push(type_section.id());
        type_section.encode(&mut self.wasm_output);
        // mark a section as injected
        Ok(())
    }

    fn process_import_section(
        &mut self,
        import_section_reader: Option<ImportSectionReader>,
    ) -> Result<(), BinaryReaderError> {
        let mut import_section = ImportSection::new();
        let mut func_idx = 0u32;
        if let Some(import_section_reader) = import_section_reader {
            for import in import_section_reader.into_iter() {
                let import = import?;
                if import.module == self.config.charge_gas_func_name.0
                    && import.name == self.config.charge_gas_func_name.1
                {
                    self.charge_gas_func_idx = Some(func_idx);
                }
                let entity_type: wasm_encoder::EntityType = import.ty.try_into().unwrap();
                import_section.import(import.module, import.name, entity_type);
                func_idx += 1;
            }
        }
        // since we inject function in the middle, then we'll need to remap all indices
        if self.charge_gas_func_idx.is_none() {
            self.remap_func_indices = true;
            import_section.import(
                self.config.charge_gas_func_name.0,
                self.config.charge_gas_func_name.1,
                wasm_encoder::EntityType::Function(self.charge_gas_func_type_idx.unwrap()),
            );
            self.charge_gas_func_idx = Some(func_idx);
        }
        // encode a type section into wasm output
        self.wasm_output.push(import_section.id());
        import_section.encode(&mut self.wasm_output);
        Ok(())
    }

    fn process_export_section(
        &mut self,
        import_section_reader: ExportSectionReader,
    ) -> Result<(), BinaryReaderError> {
        let mut export_section = ExportSection::new();
        for export in import_section_reader.into_iter() {
            let mut export = export?;
            if export.kind == ExternalKind::Func
                && export.index >= self.charge_gas_func_idx.unwrap()
            {
                export.index += 1;
            }
            export_section.export(
                export.name,
                wasm_encoder::ExportKind::try_from(export.kind).unwrap(),
                export.index,
            );
        }
        // encode a type section into wasm output
        self.wasm_output.push(export_section.id());
        export_section.encode(&mut self.wasm_output);
        Ok(())
    }

    fn process_elem_section(
        &mut self,
        elem_section_reader: ElementSectionReader,
    ) -> Result<(), BinaryReaderError> {
        let mut element_section = ElementSection::new();
        pub fn element_items<'a, T: ?Sized + Reencode>(
            charge_gas_func_idx: u32,
            re_encoder: &mut T,
            items: wasmparser::ElementItems<'a>,
        ) -> Result<wasm_encoder::Elements<'a>, Error<T::Error>> {
            Ok(match items {
                wasmparser::ElementItems::Functions(f) => {
                    let mut funcs = Vec::new();
                    for func in f {
                        let mut func_index = func?;
                        if func_index >= charge_gas_func_idx {
                            func_index += 1;
                        }
                        funcs.push(re_encoder.function_index(func_index)?);
                    }
                    wasm_encoder::Elements::Functions(funcs.into())
                }
                wasmparser::ElementItems::Expressions(ty, e) => {
                    let mut expressions = Vec::new();
                    for expr in e {
                        expressions.push(re_encoder.const_expr(expr?)?);
                    }
                    wasm_encoder::Elements::Expressions(
                        re_encoder.ref_type(ty)?,
                        expressions.into(),
                    )
                }
            })
        }
        for element in elem_section_reader.into_iter() {
            let element = element?;
            let items = element_items(
                self.charge_gas_func_idx.unwrap(),
                &mut RoundtripReencoder,
                element.items,
            )
            .unwrap();
            match element.kind {
                ElementKind::Passive => {
                    element_section.passive(items);
                }
                ElementKind::Active {
                    table_index,
                    offset_expr,
                } => {
                    element_section.active(
                        table_index,
                        &wasm_encoder::ConstExpr::try_from(offset_expr).unwrap(),
                        items,
                    );
                }
                ElementKind::Declared => {
                    element_section.declared(items);
                }
            }
        }
        // encode a type section into wasm output
        self.wasm_output.push(element_section.id());
        element_section.encode(&mut self.wasm_output);
        Ok(())
    }

    fn process_code_section_entry(
        &mut self,
        function_body: FunctionBody,
    ) -> Result<(), BinaryReaderError> {
        let mut func_output = Vec::new();
        // re-encode locals (we don't change them)
        let locals_reader = function_body.get_locals_reader()?;
        locals_reader.get_count().encode(&mut func_output);
        for local in locals_reader.into_iter() {
            let (count, typ) = local?;
            count.encode(&mut func_output);
            let typ: wasm_encoder::ValType = typ.try_into().unwrap();
            typ.encode(&mut func_output);
        }
        // encode code section
        let mut operators_reader = function_body.get_operators_reader()?;
        while !operators_reader.eof() {
            let mut binary_reader = operators_reader.get_binary_reader();
            let offset = operators_reader.original_position();
            let op = operators_reader.read()?;
            if let ShouldInject::InjectCost(consume_gas) = self.gas_meter.charge_gas_for(&op) {
                let mut sink = InstructionSink::new(&mut func_output);
                sink.i32_const(i32::try_from(consume_gas).unwrap());
                sink.call(self.charge_gas_func_idx.unwrap());
            }
            macro_rules! keep_original {
                () => {
                    let op_len = operators_reader.original_position() - offset;
                    func_output.extend_from_slice(binary_reader.read_bytes(op_len)?);
                };
            }
            match op {
                Operator::MemoryGrow { .. } => {
                    keep_original!();
                }
                Operator::Call { mut function_index } if self.remap_func_indices => {
                    if function_index >= self.charge_gas_func_idx.unwrap() {
                        function_index += 1;
                    }
                    InstructionSink::new(&mut func_output).call(function_index);
                }
                Operator::RefFunc { mut function_index } if self.remap_func_indices => {
                    if function_index >= self.charge_gas_func_idx.unwrap() {
                        function_index += 1;
                    }
                    InstructionSink::new(&mut func_output).call(function_index);
                }
                Operator::ReturnCall { mut function_index } if self.remap_func_indices => {
                    if function_index >= self.charge_gas_func_idx.unwrap() {
                        function_index += 1;
                    }
                    InstructionSink::new(&mut func_output).call(function_index);
                }
                _ => {
                    keep_original!();
                }
            }
        }
        // inject func output into a code section
        self.code_section.as_mut().unwrap().raw(&func_output);
        Ok(())
    }
}

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
            match op {
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
                    let op_len = operators_reader.original_position() - offset;
                    func_output.extend_from_slice(binary_reader.read_bytes(op_len)?);
                }
            }
        }
        // inject func output into a code section
        self.code_section.as_mut().unwrap().raw(&func_output);
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ShouldInject {
    InjectCost(u64),
    None,
}

pub trait CostModel {
    fn cost_for(&self, op: &Operator) -> u64;
}

#[derive(Default)]
pub struct DefaultCostModel;

impl CostModel for DefaultCostModel {
    fn cost_for(&self, op: &Operator) -> u64 {
        use Operator::*;
        match op {
            // Control flow may create branches, but is generally inexpensive and
            // free, so don't consume fuel.
            // Note the lack of `if` since some
            // cost is incurred with the conditional check.
            Block { .. } | Loop { .. } | Unreachable | Return | Else | End => 0,

            // Most other ops cost base
            _ => BASE_FUEL_COST as u64,
        }
    }
}

/// GasMeter state (cumulative counter, threshold, cost model)
pub struct GasMeter<T: CostModel> {
    gas_spent: u64,
    model: T,
}

impl<T: CostModel + Default> Default for GasMeter<T> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: CostModel> GasMeter<T> {
    pub fn new(model: T) -> Self {
        Self {
            gas_spent: 0,
            model,
        }
    }

    pub fn charge_gas_for(&mut self, op: &Operator) -> ShouldInject {
        use Operator::*;
        // List of control operators
        let is_control = matches!(
            op,
            Unreachable
                | Block { .. }
                | Loop { .. }
                | If { .. }
                | Else
                | End
                | Br { .. }
                | BrIf { .. }
                | BrTable { .. }
                | Return
                | Call { .. }
                | CallIndirect { .. }
                | ReturnCall { .. }
                | ReturnCallIndirect { .. }
        );
        let cost = self.model.cost_for(op);
        self.gas_spent += cost;
        if is_control && self.gas_spent > 0 {
            let total = self.gas_spent;
            self.gas_spent = 0;
            ShouldInject::InjectCost(total)
        } else {
            ShouldInject::None
        }
    }

    pub fn gas_spent(&self) -> u64 {
        self.gas_spent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_type_injection(wat: &str) -> String {
        let wasm_binary: Vec<u8> = wat::parse_str(wat).unwrap();
        let config = GasInjectorConfig {
            charge_gas_func_name: ("env", "_charge_gas"),
        };
        let gas_injector = GasInjector::new(config, DefaultCostModel::default());
        let new_wasm = gas_injector.inject(&wasm_binary).unwrap();
        let new_wat = wasmprinter::print_bytes(&new_wasm).unwrap();
        println!("{}", new_wat);
        new_wat
    }

    #[test]
    fn test_inject_type_into_module() {
        let new_wat = test_type_injection(
            r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (export "f" (func $f))
  (func $f (;0;) (type 0) (param i32) (result i32)
    local.get 0
  )
)
"#,
        );
        assert_eq!(
            new_wat,
            r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i32)))
  (import "env" "_charge_gas" (func (;0;) (type 1)))
  (export "f" (func 1))
  (func (;1;) (type 0) (param i32) (result i32)
    local.get 0
    i32.const 1
    call 0
  )
)
"#
        );
    }

    #[test]
    fn test_type_already_presented() {
        let new_wat = test_type_injection(
            r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i32)))
  (export "f" (func $f))
  (func $f (;0;) (type 0) (param i32) (result i32)
    local.get 0
  )
)
"#,
        );
        assert_eq!(
            new_wat,
            r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i32)))
  (import "env" "_charge_gas" (func (;0;) (type 1)))
  (export "f" (func 1))
  (func (;1;) (type 0) (param i32) (result i32)
    local.get 0
    i32.const 1
    call 0
  )
)
"#
        );
    }

    #[test]
    fn test_inject_a_second_distinct_type() {
        let new_wat = test_type_injection(
            r#"(module
  (type (func (param i32) (result i32)))
  (func (type 0) (param i32) (result i32)
    local.get 0)
  (func $g (param i64) (result f32)
    f32.const 1.0)
  (export "g" (func $g))
)
"#,
        );
        assert_eq!(
            new_wat,
            r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i64) (result f32)))
  (type (;2;) (func (param i32)))
  (import "env" "_charge_gas" (func (;0;) (type 2)))
  (export "g" (func 2))
  (func (;1;) (type 0) (param i32) (result i32)
    local.get 0
    i32.const 1
    call 0
  )
  (func (;2;) (type 1) (param i64) (result f32)
    f32.const 0x1p+0 (;=1;)
    i32.const 1
    call 0
  )
)
"#
        );
    }
}

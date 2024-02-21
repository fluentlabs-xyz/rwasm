use crate::{BinaryFormat, BinaryFormatError, BinaryFormatReader, BinaryFormatWriter, RwasmModule};
use rwasm::engine::bytecode::Instruction;

/// Rwasm binary version that is equal to 'R' symbol (0x52 in hex)
const RWASM_VERSION: u8 = 0x52;

/// Sections that are presented in Rwasm binary:
/// - code
/// - memory
/// - decl
/// - element
const SECTION_CODE: u8 = 0x01;
const SECTION_MEMORY: u8 = 0x02;
const SECTION_DECL: u8 = 0x03;
const SECTION_ELEMENT: u8 = 0x04;
const SECTION_END: u8 = 0x00;

/// Rwasm module encoding follows EIP-3540 standard but stores rWASM opcodes
/// instead of EVM.
///
/// https://eips.ethereum.org/EIPS/eip-3540
impl<'a> BinaryFormat<'a> for RwasmModule {
    type SelfType = RwasmModule;

    fn encoded_length(&self) -> usize {
        2 + // sig
        1 + // version
        5 + // code section header
        5 + // memory section header
        5 + // decl section header
        5 + // element section header
        1 + // terminator
            self.code_section.encoded_length() + // code section
            self.memory_section.len() + // memory section
            self.decl_section.len() * core::mem::size_of::<u32>() + // decl section
            self.element_section.len() * core::mem::size_of::<u32>() // element section
    }

    fn write_binary(&self, sink: &mut BinaryFormatWriter<'a>) -> Result<usize, BinaryFormatError> {
        // magic prefix (0xef 0x00)
        let mut n = sink.write_u8(0xef)?;
        n += sink.write_u8(0x00)?;
        // version (0x52 = R symbol)
        n += sink.write_u8(RWASM_VERSION)?;
        // code section header
        let code_section_length = self.code_section.encoded_length() as u32;
        n += sink.write_u8(SECTION_CODE)?;
        n += sink.write_u32_le(code_section_length)?;
        // memory section header
        let memory_section_length = self.memory_section.len() as u32;
        n += sink.write_u8(SECTION_MEMORY)?;
        n += sink.write_u32_le(memory_section_length)?;
        // decl section header
        let decl_section_length =
            self.decl_section.len() as u32 * (core::mem::size_of::<u32>() as u32);
        n += sink.write_u8(SECTION_DECL)?;
        n += sink.write_u32_le(decl_section_length)?;
        // element section header
        let element_section_length =
            self.element_section.len() as u32 * (core::mem::size_of::<u32>() as u32);
        n += sink.write_u8(SECTION_ELEMENT)?;
        n += sink.write_u32_le(element_section_length)?;
        // section terminator
        n += sink.write_u8(SECTION_END)?;
        // write code/decl/element sections
        for opcode in self.code_section.instr.iter() {
            n += opcode.write_binary(sink)?;
        }
        n += sink.write_bytes(&self.memory_section)?;
        for x in self.decl_section.iter() {
            n += x.write_binary(sink)?;
        }
        for x in self.element_section.iter() {
            n += x.write_binary(sink)?;
        }
        Ok(n)
    }

    fn read_binary(sink: &mut BinaryFormatReader<'a>) -> Result<Self::SelfType, BinaryFormatError> {
        let mut result = RwasmModule::default();
        // magic prefix (0xef 0x00)
        if sink.read_u8()? != 0xef || sink.read_u8()? != 0x00 {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        // version check
        let version = sink.read_u8()?;
        if version != RWASM_VERSION {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        // code section header
        sink.assert_u8(SECTION_CODE)?;
        let code_section_length = sink.read_u32_le()?;
        // memory section header
        sink.assert_u8(SECTION_MEMORY)?;
        let memory_section_length = sink.read_u32_le()?;
        // decl section header
        sink.assert_u8(SECTION_DECL)?;
        let decl_section_length = sink.read_u32_le()?;
        // element section header
        sink.assert_u8(SECTION_ELEMENT)?;
        let element_section_length = sink.read_u32_le()?;
        // section terminator
        sink.assert_u8(SECTION_END)?;
        // read code section
        let mut code_section_sink = sink.limit_with(code_section_length as usize);
        while !code_section_sink.is_empty() {
            result
                .code_section
                .push(Instruction::read_binary(&mut code_section_sink)?);
        }
        sink.pos += code_section_length as usize;
        // read memory section
        {
            let mut memory_section_sink = sink.limit_with(memory_section_length as usize);
            result
                .memory_section
                .resize(memory_section_length as usize, 0u8);
            memory_section_sink.read_bytes(&mut result.memory_section)?;
            sink.pos += memory_section_length as usize;
        }
        // read decl section
        {
            let mut decl_section_sink = sink.limit_with(decl_section_length as usize);
            while !decl_section_sink.is_empty() {
                result.decl_section.push(decl_section_sink.read_u32_le()?);
            }
            sink.pos += decl_section_length as usize;
        }
        // read element section
        {
            let mut element_section_sink = sink.limit_with(element_section_length as usize);
            while !element_section_sink.is_empty() {
                result
                    .element_section
                    .push(element_section_sink.read_u32_le()?);
            }
            sink.pos += element_section_length as usize;
        }
        // return the final module
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::{instruction_set, BinaryFormat, RwasmModule};

    #[test]
    fn test_module_encoding() {
        let instruction_set = instruction_set! {
            // .add_memory_pages(1)
            // .add_default_memory(0, &[0, 1, 2, 3])
            // .add_default_memory(100, &[4, 5, 6, 7])
            I32Const(100)
            I32Const(20)
            I32Add
            I32Const(3)
            I32Add
            Drop
        };
        // let memory_section = instruction_set.memory_section.clone();
        let module = RwasmModule {
            code_section: instruction_set,
            memory_section: Default::default(),
            decl_section: vec![1, 2, 3],
            element_section: vec![5, 6, 7, 8, 9],
        };
        let mut encoded_data = Vec::new();
        module.write_binary_to_vec(&mut encoded_data).unwrap();
        assert_eq!(module.encoded_length(), encoded_data.len());
        let module2 = RwasmModule::read_from_slice(&encoded_data).unwrap();
        assert_eq!(module, module2);
    }
}

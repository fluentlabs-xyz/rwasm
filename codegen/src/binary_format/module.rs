use crate::{BinaryFormat, BinaryFormatError, BinaryFormatReader, BinaryFormatWriter, RwasmModule};
use rwasm::engine::bytecode::Instruction;

const RWASM_VERSION: u8 = 0x52;
const SECTION_TERMINATOR: u8 = 0x00;
const SECTION_CODE: u8 = 0x01;
const SECTION_MEMORY: u8 = 0x02;

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
        1 + // terminator
            self.code_section.encoded_length() + // code section
            self.code_section.memory_section.len() // memory section
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
        // data section header
        let memory_section_length = self.code_section.memory_section.len() as u32;
        n += sink.write_u8(SECTION_MEMORY)?;
        n += sink.write_u32_le(memory_section_length)?;
        // section terminator
        n += sink.write_u8(SECTION_TERMINATOR)?;
        // write code section
        for opcode in self.code_section.instr.iter() {
            n += opcode.write_binary(sink)?;
        }
        // write data section
        n += sink.write_bytes(&self.code_section.memory_section)?;
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
        if sink.read_u8()? != SECTION_CODE {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        let code_section_length = sink.read_u32_le()?;
        // memory section header
        if sink.read_u8()? != SECTION_MEMORY {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        let memory_section_length = sink.read_u32_le()?;
        // section terminator
        if sink.read_u8()? != SECTION_TERMINATOR {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        // read code section
        let mut code_section_sink = sink.limit_with(code_section_length as usize);
        while !code_section_sink.is_empty() {
            result
                .code_section
                .push(Instruction::read_binary(&mut code_section_sink)?);
        }
        sink.pos += code_section_length as usize;
        // read memory section
        let mut memory_section_sink = sink.limit_with(memory_section_length as usize);
        result
            .code_section
            .memory_section
            .resize(memory_section_length as usize, 0u8);
        memory_section_sink.read_bytes(&mut result.code_section.memory_section)?;
        sink.pos += memory_section_length as usize;
        // return the final module
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::{instruction_set, BinaryFormat, RwasmModule};

    #[test]
    #[ignore]
    fn test_module_encoding() {
        let instruction_set = instruction_set! {
            .add_memory_pages(1)
            .add_default_memory(0, &[0, 1, 2, 3])
            .add_default_memory(100, &[4, 5, 6, 7])
            I64Const32(100)
            I64Const32(20)
            I32Add
            I64Const32(3)
            I32Add
            Drop
        };
        let memory_section = instruction_set.memory_section.clone();
        let module = RwasmModule {
            code_section: instruction_set,
            memory_section,
            function_section: vec![],
            element_section: vec![],
        };
        let mut encoded_data = Vec::new();
        module.write_binary_to_vec(&mut encoded_data).unwrap();
        assert_eq!(module.encoded_length(), encoded_data.len());
        let module2 = RwasmModule::read_from_slice(&encoded_data).unwrap();
        assert_eq!(module, module2);
    }
}

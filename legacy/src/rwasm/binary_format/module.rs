use crate::{
    engine::bytecode::Instruction,
    rwasm::{BinaryFormat, BinaryFormatError, BinaryFormatReader, BinaryFormatWriter, RwasmModule},
};

/// Rwasm magic bytes 0xef52
pub const RWASM_MAGIC_BYTE_0: u8 = 0xef;
pub const RWASM_MAGIC_BYTE_1: u8 = 0x52;

/// Rwasm binary version that is equal to 'R' symbol (0x52 in hex)
pub const RWASM_VERSION_V1: u8 = 0x01;

/// Rwasm module encoding follows EIP-3540 standard but stores rWASM opcodes
/// instead of EVM.
///
/// https://eips.ethereum.org/EIPS/eip-3540
impl<'a> BinaryFormat<'a> for RwasmModule {
    type SelfType = RwasmModule;

    fn encoded_length(&self) -> usize {
        3 + // sig + version
            size_of::<u64>() + self.code_section.encoded_length() + // code section
            size_of::<u64>() + self.memory_section.len() + // memory section
            size_of::<u64>() + self.element_section.len() * size_of::<u32>() + // element section
            size_of::<u32>() + // source pc
            size_of::<u64>() + self.func_section.len() * size_of::<u32>() // decl section
    }

    fn write_binary(&self, sink: &mut BinaryFormatWriter<'a>) -> Result<usize, BinaryFormatError> {
        let mut n = 0;
        // magic prefix (0xef 0x52 0x01)
        n += sink.write_u8(RWASM_MAGIC_BYTE_0)?;
        n += sink.write_u8(RWASM_MAGIC_BYTE_1)?;
        n += sink.write_u8(RWASM_VERSION_V1)?;
        // code section
        let code_section_length = self.code_section.len() as u64;
        n += sink.write_u64_le(code_section_length)?;
        for opcode in self.code_section.instrs().iter() {
            n += opcode.write_binary(sink)?;
        }
        // memory section
        let memory_section_length = self.memory_section.len() as u64;
        n += sink.write_u64_le(memory_section_length)?;
        n += sink.write_bytes(&self.memory_section)?;
        // element section
        let element_section_length = self.element_section.len() as u64;
        n += sink.write_u64_le(element_section_length)?;
        for x in self.element_section.iter() {
            n += x.write_binary(sink)?;
        }
        // source pc
        n += sink.write_u32_le(self.source_pc)?;
        // func section
        let func_section_length = self.func_section.len() as u64;
        n += sink.write_u64_le(func_section_length)?;
        for x in self.func_section.iter() {
            n += x.write_binary(sink)?;
        }
        Ok(n)
    }

    fn read_binary(sink: &mut BinaryFormatReader<'a>) -> Result<Self::SelfType, BinaryFormatError> {
        let mut result = RwasmModule::default();
        // magic prefix (0xef 0x52)
        if sink.read_u8()? != RWASM_MAGIC_BYTE_0 || sink.read_u8()? != RWASM_MAGIC_BYTE_1 {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        // version check
        let version = sink.read_u8()?;
        if version != RWASM_VERSION_V1 {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        // code section header
        let code_section_length = sink.read_u64_le()?;
        for _ in 0..code_section_length {
            result.code_section.push(Instruction::read_binary(sink)?);
        }
        // memory section
        let memory_section_length = sink.read_u64_le()?;
        {
            result
                .memory_section
                .resize(memory_section_length as usize, 0u8);
            sink.read_bytes(&mut result.memory_section)?;
        }
        // element section
        let element_section_length = sink.read_u64_le()?;
        {
            for _ in 0..element_section_length {
                result.element_section.push(sink.read_u32_le()?);
            }
        }
        // source pc
        result.source_pc = sink.read_u32_le()?;
        // func section
        let decl_section_length = sink.read_u64_le()?;
        {
            for _ in 0..decl_section_length {
                result.func_section.push(sink.read_u32_le()?);
            }
        }
        // return the final module
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        instruction_set,
        rwasm::{BinaryFormat, RwasmModule},
    };

    #[test]
    fn test_module_encoding() {
        let module = RwasmModule {
            code_section: instruction_set! {
                I32Const(100)
                I32Const(20)
                I32Add
                I32Const(3)
                I32Add
                Drop
            },
            memory_section: Default::default(),
            func_section: vec![1, 2, 3],
            element_section: vec![5, 6, 7, 8, 9],
            source_pc: 0,
        };
        let mut encoded_data = Vec::new();
        module.write_binary_to_vec(&mut encoded_data).unwrap();
        assert_eq!(module.encoded_length(), encoded_data.len());
        let module2 = RwasmModule::read_from_slice(&encoded_data).unwrap();
        assert_eq!(module, module2);
    }
}

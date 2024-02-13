use crate::{
    BinaryFormat,
    BinaryFormatError,
    BinaryFormatReader,
    BinaryFormatWriter,
    InstructionSet,
    RwasmModule,
};

const RWASM_VERSION: u8 = 0x52;
const SECTION_TERMINATOR: u8 = 0x00;
const SECTION_CODE: u8 = 0x01;

/// Rwasm module encoding follows EIP-3540 standard but stores rWASM opcodes
/// instead of EVM.
///
/// https://eips.ethereum.org/EIPS/eip-3540
impl<'a> BinaryFormat<'a> for RwasmModule {
    type SelfType = RwasmModule;

    fn encoded_length(&self) -> usize {
        3 + 5 + 1 + self.instruction_set.encoded_length()
    }

    fn write_binary(&self, sink: &mut BinaryFormatWriter<'a>) -> Result<usize, BinaryFormatError> {
        // magic prefix (0xef 0x00)
        let mut n = sink.write_u8(0xef)?;
        n += sink.write_u8(0x00)?;
        // version (0x52 = R symbol)
        n += sink.write_u8(RWASM_VERSION)?;
        // code section header
        let code_section_length = self.instruction_set.encoded_length();
        n += sink.write_u8(SECTION_CODE)?;
        n += sink.write_u32_le(code_section_length as u32)?;
        // section terminator
        n += sink.write_u8(SECTION_TERMINATOR)?;
        // write code section
        n += self.instruction_set.write_binary(sink)?;
        Ok(n)
    }

    fn read_binary(sink: &mut BinaryFormatReader<'a>) -> Result<Self::SelfType, BinaryFormatError> {
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
        // section terminator
        if sink.read_u8()? != SECTION_TERMINATOR {
            return Err(BinaryFormatError::MalformedWasmModule);
        }
        // read code section
        let instruction_set = InstructionSet::read_binary(sink)?;
        assert_eq!(
            instruction_set.encoded_length(),
            code_section_length as usize
        );
        // return the final module
        Ok(RwasmModule { instruction_set })
    }
}

#[cfg(test)]
mod tests {
    use crate::{instruction_set, BinaryFormat, RwasmModule};

    #[test]
    fn test_module_encoding() {
        let instruction_set = instruction_set! {
            I32Const(100)
            I32Const(20)
            I32Add
            I32Const(3)
            I32Add
            Drop
        };
        let module = RwasmModule { instruction_set };
        let mut encoded_data = Vec::new();
        module.write_binary_to_vec(&mut encoded_data).unwrap();
        let module2 = RwasmModule::read_from_slice(&encoded_data).unwrap();
        assert_eq!(module, module2);
    }
}

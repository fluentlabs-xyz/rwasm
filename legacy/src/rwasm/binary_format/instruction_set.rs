use crate::{
    engine::bytecode::Instruction,
    rwasm::{
        binary_format::{
            reader_writer::{BinaryFormatReader, BinaryFormatWriter},
            BinaryFormat,
            BinaryFormatError,
        },
        instruction_set::InstructionSet,
    },
};

impl<'a> BinaryFormat<'a> for InstructionSet {
    type SelfType = InstructionSet;

    fn encoded_length(&self) -> usize {
        let mut n = 0;
        for opcode in self.instrs().iter() {
            n += opcode.encoded_length();
        }
        n
    }

    fn write_binary(&self, sink: &mut BinaryFormatWriter<'a>) -> Result<usize, BinaryFormatError> {
        let mut n = 0;
        for opcode in self.instrs().iter() {
            n += opcode.write_binary(sink)?;
        }
        Ok(n)
    }

    fn read_binary(sink: &mut BinaryFormatReader<'a>) -> Result<InstructionSet, BinaryFormatError> {
        let mut result = InstructionSet::new();
        while !sink.is_empty() {
            result.push(Instruction::read_binary(sink)?);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::rwasm::{BinaryFormat, InstructionSet};
    use hex_literal::hex;

    #[test]
    fn decode_code_section() {
        let code_section = hex!("0a01000000000000000a00000000000000001101000000000000000b00000000000000000a01000000000000000a00000000000000001105000000000000000b00000000000000000a04000000000000001302000000000000003e00000000000000001000000000000000000900000000000000000a07000000000000001302000000000000003e00000400000000003e0c000000000000001001000000000000003e00000000000000001000000000000000000900000000000000000a01000000000000003f00000400000000001700000000000000003f0c000400000000001701000000000000003f10000400000000001702000000000000003e05000000000000003000000000000000001400000000000000003e00000400000000003f00000000000000003f0c000000000000003300000000000000003401000000000000001102000000000000000001000000000000003e01000000000000004300000000000000000404000000000000001400000000000000001002000000000000000b00000000000000000001000000000000003e00000000000000004300000000000000000404000000000000001400000000000000001003000000000000000b00000000000000001400000000000000000b0000000000000000");
        let code_section = InstructionSet::read_from_slice(code_section.as_ref()).unwrap();
        println!("{:?}", code_section.instr);
    }
}

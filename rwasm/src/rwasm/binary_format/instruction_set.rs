use crate::{
    engine::bytecode::Instruction,
    rwasm::{
        binary_format::{
            reader_writer::{BinaryFormatReader, BinaryFormatWriter},
            BinaryFormat,
            BinaryFormatError,
        },
        instruction::INSTRUCTION_SIZE_BYTES,
        instruction_set::InstructionSet,
    },
};

impl<'a> BinaryFormat<'a> for InstructionSet {
    type SelfType = InstructionSet;

    fn encoded_length(&self) -> usize {
        self.len() * INSTRUCTION_SIZE_BYTES
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

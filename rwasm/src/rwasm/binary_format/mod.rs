mod drop_keep;
pub mod instruction;
mod instruction_set;
mod module;
mod number;
pub mod reader_writer;
mod utils;

pub use crate::rwasm::binary_format::reader_writer::{BinaryFormatReader, BinaryFormatWriter};
use alloc::vec::Vec;

#[derive(Debug, Copy, Clone)]
pub enum BinaryFormatError {
    ReachedUnreachable,
    NeedMore(usize),
    MalformedWasmModule,
    IllegalOpcode(u8),
}

pub trait BinaryFormat<'a> {
    type SelfType;

    fn encoded_length(&self) -> usize;

    fn write_binary_to_vec(&self, buffer: &'a mut Vec<u8>) -> Result<usize, BinaryFormatError> {
        buffer.resize(self.encoded_length(), 0u8);
        let mut sink = BinaryFormatWriter::<'a>::new(buffer.as_mut_slice());
        self.write_binary(&mut sink)
    }

    #[cfg(feature = "riscv_special_writer")]
    fn write_binary_riscv_special(&self, buffer: &'a mut Vec<u8>)
            -> Result<(usize, usize, usize, Vec<u8>, Vec<u8>, Vec<(u32,u32,u32)>), BinaryFormatError> {
        buffer.resize(self.encoded_length(), 0u8);
        let mut sink = BinaryFormatWriter::<'a>::new(buffer.as_mut_slice());
        let size = self.write_binary(&mut sink)?;
        println!("DEBUG {:#?}", &sink.unaligned);
        Ok((size, sink.aligned_code_section_len, sink.unaligned_code_section_len,
            sink.unaligned.mapped, sink.aligned, sink.au_jump_table))
    }

    fn write_binary(&self, sink: &mut BinaryFormatWriter<'a>) -> Result<usize, BinaryFormatError>;

    fn read_from_slice(sink: &'a [u8]) -> Result<Self::SelfType, BinaryFormatError> {
        let mut binary_format_reader = BinaryFormatReader::<'a>::new(sink);
        Self::read_binary(&mut binary_format_reader)
    }

    fn read_binary(sink: &mut BinaryFormatReader<'a>) -> Result<Self::SelfType, BinaryFormatError>;
}

use crate::{
    engine::DropKeep,
    rwasm::{BinaryFormat, BinaryFormatError, BinaryFormatReader, BinaryFormatWriter},
};

impl<'a> BinaryFormat<'a> for DropKeep {
    type SelfType = DropKeep;

    fn encoded_length(&self) -> usize {
        8
    }

    fn write_binary(&self, sink: &mut BinaryFormatWriter<'a>) -> Result<usize, BinaryFormatError> {
        let mut n = 0;
        n += sink.write_u16_le(self.drop())?;
        n += sink.write_u16_le(self.keep())?;
        Ok(n)
    }

    fn read_binary(sink: &mut BinaryFormatReader<'a>) -> Result<Self::SelfType, BinaryFormatError> {
        let drop = sink.read_u16_le()?;
        let keep = sink.read_u16_le()?;
        Ok(DropKeep::new(drop as usize, keep as usize).unwrap())
    }
}

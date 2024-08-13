use crate::rwasm::binary_format::BinaryFormatError;
use alloc::vec::Vec;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
        
#[cfg(feature = "riscv_special_writer")]
#[derive(Debug,Default,Clone,Copy,PartialEq)]
pub enum Kind {
    #[default]
    OneByte,
    FiveBytes,
    NineBytes,
}
        
#[cfg(feature = "riscv_special_writer")]
#[derive(Debug,Default,Clone,Copy)]
pub enum Flow {
    #[default]
    Unknown,
    CachedPos { pos: usize, kind: Option<Kind>, count: Option<usize> },
    Reduced { kind: Kind, count: usize },
}

#[cfg(feature = "riscv_special_writer")]
#[derive(Debug,Default,Clone)]
pub struct MappedFlow<T> {
    pub mapped: Vec<T>,
    pub flow: Vec<Flow>,
}

impl<T> MappedFlow<T> {

    pub fn reduce_flow(&mut self) {
        if let Some(last) = self.flow.pop() {
            if let Some(lprev) = self.flow.pop() {
                self.reduce_and_put_back(lprev, last);
            } else {
                self.flow.push(last);
            }
        }
    }

    pub fn reduce_and_put_back(&mut self, prev: Flow, cur: Flow) {
        use Kind::*;
        use Flow::*;
        match (prev, cur) {
            (CachedPos { pos: ppos, kind: pkind, count: pcount }, CachedPos { pos: cpos, kind: ckind, count: None }) => {
                let ckind = match cpos as isize - ppos as isize {
                    1 => Some(OneByte),
                    5 => Some(FiveBytes),
                    9 => Some(NineBytes),
                    _ => ckind,
                };
                if pkind.is_some() && pkind == ckind {
                    let count = match pcount {
                        Some(cnt) => cnt + 1,
                        None => 1,
                    };
                    self.flow.push(CachedPos { pos: cpos, kind: pkind, count: Some(count) });
                    return;
                }
                self.flow.push(CachedPos { pos: ppos, kind: pkind, count: pcount });
                self.flow.push(CachedPos { pos: cpos, kind: ckind, count: None });
            }
            _ => (),
        }
    }
}

pub struct BinaryFormatWriter<'a> {
    pub sink: &'a mut [u8],
    #[cfg(feature = "riscv_special_writer")]
    pub aligned: Vec<u8>,
    #[cfg(feature = "riscv_special_writer")]
    pub unaligned: MappedFlow<u8>,
    pos: usize,
    #[cfg(feature = "riscv_special_writer")]
    pub pos_aligned: usize,
    #[cfg(feature = "riscv_special_writer")]
    pub pos_unaligned: usize,
    #[cfg(feature = "riscv_special_writer")]
    pub unaligned_code_section_len: usize,
    #[cfg(feature = "riscv_special_writer")]
    pub aligned_code_section_len: usize,
}

macro_rules! append_aligned { ($self:ident, $bytes:literal, $Endian:ident :: $write:ident, $value:ident) => {
    #[cfg(feature = "riscv_special_writer")]
    {
        let mut buf = [0u8; $bytes];
        $Endian::$write(&mut buf, $value);
        $self.aligned.append(&mut buf.to_vec());
    }
}}

impl<'a> BinaryFormatWriter<'a> {

    #[cfg(not(feature = "riscv_special_writer"))]
    pub fn new(sink: &'a mut [u8]) -> Self {
        Self { sink, pos: 0 }
    }

    #[cfg(feature = "riscv_special_writer")]
    pub fn new(sink: &'a mut [u8]) -> Self {
        Self {
            sink,
            aligned: vec![],
            unaligned: MappedFlow::default(),
            pos: 0,
            pos_aligned: 0,
            pos_unaligned: 0,
            unaligned_code_section_len: 0,
            aligned_code_section_len: 0,
        }
    }

    pub fn write_u8(&mut self, value: u8) -> Result<usize, BinaryFormatError> {
        let n = self.require(1)?;
        self.sink[self.pos] = value;
        #[cfg(feature = "riscv_special_writer")]
        {
            self.unaligned.mapped.push(value);
            self.unaligned.flow.push(Flow::CachedPos { pos: self.pos, kind: None, count: None });
            self.unaligned.reduce_flow();
        }
        self.skip(n)
    }

    pub fn write_u16_be(&mut self, value: u16) -> Result<usize, BinaryFormatError> {
        let n = self.require(2)?;
        BigEndian::write_u16(&mut self.sink[self.pos..], value);
        self.skip(n)
    }

    pub fn write_u16_le(&mut self, value: u16) -> Result<usize, BinaryFormatError> {
        let n = self.require(2)?;
        LittleEndian::write_u16(&mut self.sink[self.pos..], value);
        self.skip(n)
    }

    pub fn write_i16_be(&mut self, value: i16) -> Result<usize, BinaryFormatError> {
        let n = self.require(2)?;
        BigEndian::write_i16(&mut self.sink[self.pos..], value);
        self.skip(n)
    }

    pub fn write_i16_le(&mut self, value: i16) -> Result<usize, BinaryFormatError> {
        let n = self.require(2)?;
        LittleEndian::write_i16(&mut self.sink[self.pos..], value);
        self.skip(n)
    }

    pub fn write_u32_be(&mut self, value: u32) -> Result<usize, BinaryFormatError> {
        let n = self.require(4)?;
        BigEndian::write_u32(&mut self.sink[self.pos..], value);
        append_aligned!(self, 4, BigEndian::write_u32, value);
        self.skip(n)
    }

    pub fn write_u32_le(&mut self, value: u32) -> Result<usize, BinaryFormatError> {
        let n = self.require(4)?;
        LittleEndian::write_u32(&mut self.sink[self.pos..], value);
        append_aligned!(self, 4, LittleEndian::write_u32, value);
        self.skip(n)
    }

    pub fn write_i32_be(&mut self, value: i32) -> Result<usize, BinaryFormatError> {
        let n = self.require(4)?;
        BigEndian::write_i32(&mut self.sink[self.pos..], value);
        append_aligned!(self, 4, BigEndian::write_i32, value);
        self.skip(n)
    }

    pub fn write_i32_le(&mut self, value: i32) -> Result<usize, BinaryFormatError> {
        let n = self.require(4)?;
        LittleEndian::write_i32(&mut self.sink[self.pos..], value);
        append_aligned!(self, 4, LittleEndian::write_i32, value);
        self.skip(n)
    }

    pub fn write_u64_be(&mut self, value: u64) -> Result<usize, BinaryFormatError> {
        let n = self.require(8)?;
        BigEndian::write_u64(&mut self.sink[self.pos..], value);
        append_aligned!(self, 8, BigEndian::write_u64, value);
        self.skip(n)
    }

    pub fn write_u64_le(&mut self, value: u64) -> Result<usize, BinaryFormatError> {
        let n = self.require(8)?;
        LittleEndian::write_u64(&mut self.sink[self.pos..], value);
        append_aligned!(self, 8, LittleEndian::write_u64, value);
        self.skip(n)
    }

    pub fn write_i64_be(&mut self, value: i64) -> Result<usize, BinaryFormatError> {
        let n = self.require(8)?;
        BigEndian::write_i64(&mut self.sink[self.pos..], value);
        append_aligned!(self, 8, BigEndian::write_i64, value);
        self.skip(n)
    }

    pub fn write_i64_le(&mut self, value: i64) -> Result<usize, BinaryFormatError> {
        let n = self.require(8)?;
        LittleEndian::write_i64(&mut self.sink[self.pos..], value);
        append_aligned!(self, 8, LittleEndian::write_i64, value);
        self.skip(n)
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<usize, BinaryFormatError> {
        let n = self.require(bytes.len())?;
        self.sink[self.pos..(self.pos + n)].copy_from_slice(bytes);
        if n == 4 || n == 8 {
            self.aligned.append(&mut bytes.clone().to_vec());
        }
        self.skip(n)
    }

    fn require(&self, n: usize) -> Result<usize, BinaryFormatError> {
        if self.sink.len() < self.pos + n {
            Err(BinaryFormatError::NeedMore(self.pos + n - self.sink.len()))
        } else {
            Ok(n)
        }
    }

    pub fn reset(&mut self) {
        self.pos = 0;
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn skip(&mut self, n: usize) -> Result<usize, BinaryFormatError> {
        assert!(self.sink.len() >= self.pos + n);
        self.pos += n;
        #[cfg(feature = "riscv_special_writer")]
        if n == 4 || n == 8 {
            self.pos_aligned += n;
        }
        #[cfg(feature = "riscv_special_writer")]
        if n == 1 {
            self.pos_unaligned += 1;
        }
        Ok(n)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.sink[0..self.pos].to_vec()
    }
}

pub struct BinaryFormatReader<'a> {
    pub sink: &'a [u8],
    pub pos: usize,
}

impl<'a> BinaryFormatReader<'a> {
    pub fn new(sink: &'a [u8]) -> Self {
        Self { sink, pos: 0 }
    }

    pub fn limit_with(&self, length: usize) -> Self {
        Self {
            sink: &self.sink[self.pos..(self.pos + length)],
            pos: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.sink.len()
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn assert_u8(&mut self, value: u8) -> Result<u8, BinaryFormatError> {
        if self.read_u8()? != value {
            Err(BinaryFormatError::MalformedWasmModule)
        } else {
            Ok(1)
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, BinaryFormatError> {
        self.require(1)?;
        let result = self.sink[self.pos];
        self.skip(1)?;
        Ok(result)
    }

    pub fn read_u16_be(&mut self) -> Result<u16, BinaryFormatError> {
        self.require(2)?;
        let result = BigEndian::read_u16(&self.sink[self.pos..]);
        self.skip(2)?;
        Ok(result)
    }

    pub fn read_u16_le(&mut self) -> Result<u16, BinaryFormatError> {
        self.require(2)?;
        let result = LittleEndian::read_u16(&self.sink[self.pos..]);
        self.skip(2)?;
        Ok(result)
    }

    pub fn read_i16_be(&mut self) -> Result<i16, BinaryFormatError> {
        self.require(2)?;
        let result = BigEndian::read_i16(&self.sink[self.pos..]);
        self.skip(2)?;
        Ok(result)
    }

    pub fn read_i16_le(&mut self) -> Result<i16, BinaryFormatError> {
        self.require(2)?;
        let result = LittleEndian::read_i16(&self.sink[self.pos..]);
        self.skip(2)?;
        Ok(result)
    }

    pub fn read_u32_be(&mut self) -> Result<u32, BinaryFormatError> {
        self.require(4)?;
        let result = BigEndian::read_u32(&self.sink[self.pos..]);
        self.skip(4)?;
        Ok(result)
    }

    pub fn read_u32_le(&mut self) -> Result<u32, BinaryFormatError> {
        self.require(4)?;
        let result = LittleEndian::read_u32(&self.sink[self.pos..]);
        self.skip(4)?;
        Ok(result)
    }

    pub fn read_i32_be(&mut self) -> Result<i32, BinaryFormatError> {
        self.require(4)?;
        let result = BigEndian::read_i32(&self.sink[self.pos..]);
        self.skip(4)?;
        Ok(result)
    }

    pub fn read_i32_le(&mut self) -> Result<i32, BinaryFormatError> {
        self.require(4)?;
        let result = LittleEndian::read_i32(&self.sink[self.pos..]);
        self.skip(4)?;
        Ok(result)
    }

    pub fn read_u64_be(&mut self) -> Result<u64, BinaryFormatError> {
        self.require(8)?;
        let result = BigEndian::read_u64(&self.sink[self.pos..]);
        self.skip(8)?;
        Ok(result)
    }

    pub fn read_u64_le(&mut self) -> Result<u64, BinaryFormatError> {
        self.require(8)?;
        let result = LittleEndian::read_u64(&self.sink[self.pos..]);
        self.skip(8)?;
        Ok(result)
    }

    pub fn read_i64_be(&mut self) -> Result<i64, BinaryFormatError> {
        self.require(8)?;
        let result = BigEndian::read_i64(&self.sink[self.pos..]);
        self.skip(8)?;
        Ok(result)
    }

    pub fn read_i64_le(&mut self) -> Result<i64, BinaryFormatError> {
        self.require(8)?;
        let result = LittleEndian::read_i64(&self.sink[self.pos..]);
        self.skip(8)?;
        Ok(result)
    }

    pub fn read_bytes(&mut self, bytes: &mut [u8]) -> Result<(), BinaryFormatError> {
        self.require(bytes.len())?;
        bytes.copy_from_slice(&self.sink[self.pos..(self.pos + bytes.len())]);
        self.skip(bytes.len())
    }

    fn require(&self, n: usize) -> Result<(), BinaryFormatError> {
        if self.sink.len() < self.pos + n {
            Err(BinaryFormatError::NeedMore(self.pos + n - self.sink.len()))
        } else {
            Ok(())
        }
    }

    fn skip(&mut self, n: usize) -> Result<(), BinaryFormatError> {
        assert!(self.sink.len() >= self.pos + n);
        self.pos += n;
        Ok(())
    }
}

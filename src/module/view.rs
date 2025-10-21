use crate::{Opcode, RwasmModule, RWASM_MAGIC_BYTE_0, RWASM_MAGIC_BYTE_1, RWASM_VERSION_V1};
use bincode::error::DecodeError;
use core::mem::size_of;

/// Zero-copy view over a compiled rWasm module binary.
///
/// This struct does not allocate. It keeps only the original binary slice and
/// offsets/lengths for each serialized section, and provides accessors and
/// iterators over the contents.
#[derive(Debug, Clone, Copy)]
pub struct RwasmModuleView<'a> {
    sink: &'a [u8],
    // Entire encoded Vec<Opcode> region [start..end)
    code_start: usize,
    code_end: usize,
    // Encoded Vec<u8> (data) payload region (without length header)
    data_payload_start: usize,
    data_payload_end: usize,
    // Encoded Vec<u32> payload region (without length header)
    elem_payload_start: usize,
    elem_payload_end: usize,
    // Encoded Vec<u8> (hint) payload region (without length header)
    hint_payload_start: usize,
    hint_payload_end: usize,
}

impl<'a> RwasmModuleView<'a> {
    /// Parses the rwasm binary and returns a zero-copy view plus number of bytes consumed.
    pub fn new_checked(sink: &'a [u8]) -> Result<(Self, usize), DecodeError> {
        let cfg = bincode::config::legacy();
        let mut pos = 0usize;

        // magic bytes
        let (sig0, n): (u8, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        let (sig1, n): (u8, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        if sig0 != RWASM_MAGIC_BYTE_0 || sig1 != RWASM_MAGIC_BYTE_1 {
            return Err(DecodeError::Other("rwasm: invalid magic bytes"));
        }
        // version
        let (version, n): (u8, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        if version != RWASM_VERSION_V1 {
            return Err(DecodeError::Other("rwasm: not supported version"));
        }

        // code_section: Vec<Opcode>
        let code_start = pos;
        // read length (u64 in legacy) to locate element start
        let (code_len, n): (u64, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        let elems_left = code_len as usize;
        let mut iter_pos = pos;
        for _ in 0..elems_left {
            let (_op, read): (Opcode, usize) = bincode::decode_from_slice(&sink[iter_pos..], cfg)?;
            iter_pos += read;
        }
        let code_end = iter_pos;
        pos = code_end;

        // data_section: Vec<u8> => [len: u64][bytes]
        let (data_len, n): (u64, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        let data_payload_start = pos;
        let data_payload_end = data_payload_start + (data_len as usize);
        if data_payload_end > sink.len() {
            return Err(DecodeError::Other("rwasm: malformed data section"));
        }
        pos = data_payload_end;

        // elem_section: Vec<u32> => [len: u64][u32 LE x len]
        let (elem_len, n): (u64, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        let elem_payload_start = pos;
        let elem_payload_end =
            elem_payload_start + (elem_len as usize) * core::mem::size_of::<u32>();
        if elem_payload_end > sink.len() {
            return Err(DecodeError::Other("rwasm: malformed elem section"));
        }
        pos = elem_payload_end;

        // hint_section: Vec<u8> => [len: u64][bytes]
        let (hint_len, n): (u64, usize) = bincode::decode_from_slice(&sink[pos..], cfg)?;
        pos += n;
        let hint_payload_start = pos;
        let hint_payload_end = hint_payload_start + (hint_len as usize);
        if hint_payload_end > sink.len() {
            return Err(DecodeError::Other("rwasm: malformed hint section"));
        }
        pos = hint_payload_end;

        let view = RwasmModuleView {
            sink,
            code_start,
            code_end,
            data_payload_start,
            data_payload_end,
            elem_payload_start,
            elem_payload_end,
            hint_payload_start,
            hint_payload_end,
        };
        Ok((view, pos))
    }

    /// Parse rWasm module (make it execution ready).
    pub fn to_module(&self) -> RwasmModule {
        let (module, _) = RwasmModule::new(self.sink);
        module
    }

    /// Like `new_checked` but panics on malformed module.
    pub fn new(sink: &'a [u8]) -> (Self, usize) {
        Self::new_checked(sink).unwrap_or_else(|_| unreachable!("rwasm: malformed rwasm binary"))
    }

    pub fn is_empty(&self) -> bool {
        self.code_start == 3
            && self.code_end == 3
            && self.data_payload_start == self.data_payload_end
            && self.elem_payload_start == self.elem_payload_end
            && self.hint_payload_start == self.hint_payload_end
    }

    /// Read-only data section (payload only).
    pub fn data_section(&self) -> &'a [u8] {
        &self.sink[self.data_payload_start..self.data_payload_end]
    }

    /// Element section iterator over function refs (u32 values).
    pub fn elem_section(&self) -> ElemIter<'a> {
        // The encoded Vec<u32> starts 8 bytes before the payload start.
        // We need length to know how many elements to yield.
        let cfg = bincode::config::legacy();
        let len_header_start = self.elem_payload_start - size_of::<u64>();
        let (len, _): (u64, usize) =
            bincode::decode_from_slice(&self.sink[len_header_start..], cfg).unwrap_or((0, 0));
        ElemIter {
            sink: self.sink,
            pos: self.elem_payload_start,
            remaining: len as usize,
        }
    }

    /// Hint section (payload only).
    pub fn hint_section(&self) -> &'a [u8] {
        &self.sink[self.hint_payload_start..self.hint_payload_end]
    }

    /// Instruction set view.
    pub fn code_section(&self) -> InstructionSetView<'a> {
        InstructionSetView::new(self.sink, self.code_start, self.code_end)
    }
}

/// Zero-copy view for the encoded InstructionSet (Vec<Opcode>)
#[derive(Debug, Clone, Copy)]
pub struct InstructionSetView<'a> {
    sink: &'a [u8],
    /// start of the encoded Vec<Opcode>
    start: usize,
    /// end of the encoded Vec<Opcode>
    end: usize,
    /// number of opcodes
    len: usize,
    /// element payload start (after length header)
    elems_start: usize,
}

impl<'a> InstructionSetView<'a> {
    fn new(sink: &'a [u8], start: usize, end: usize) -> Self {
        let cfg = bincode::config::legacy();
        let (len, n): (u64, usize) = bincode::decode_from_slice(&sink[start..], cfg).unwrap();
        let elems_start = start + n; // n should be 8 in legacy
        Self {
            sink,
            start,
            end,
            len: len as usize,
            elems_start,
        }
    }

    /// Returns the raw encoded bytes of the Vec<Opcode> as present in the module binary.
    pub fn raw(&self) -> &'a [u8] {
        &self.sink[self.start..self.end]
    }

    /// Returns an iterator over decoded opcodes.
    pub fn opcodes(&self) -> OpcodeIter<'a> {
        OpcodeIter {
            sink: self.sink,
            pos: self.elems_start,
            remaining: self.len,
        }
    }

    /// Number of opcodes.
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Iterator over u32 elements of the elem section
pub struct ElemIter<'a> {
    sink: &'a [u8],
    pos: usize,
    remaining: usize,
}

impl<'a> Iterator for ElemIter<'a> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let cfg = bincode::config::legacy();
        let (val, n): (u32, usize) =
            bincode::decode_from_slice(&self.sink[self.pos..], cfg).ok()?;
        self.pos += n;
        self.remaining -= 1;
        Some(val)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for ElemIter<'a> {}

/// Iterator over opcodes of the instruction set
pub struct OpcodeIter<'a> {
    sink: &'a [u8],
    pos: usize,
    remaining: usize,
}

impl<'a> Iterator for OpcodeIter<'a> {
    type Item = Opcode;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let cfg = bincode::config::legacy();
        let (op, n): (Opcode, usize) =
            bincode::decode_from_slice(&self.sink[self.pos..], cfg).ok()?;
        self.pos += n;
        self.remaining -= 1;
        Some(op)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for OpcodeIter<'a> {}

#[cfg(test)]
mod tests {
    use super::RwasmModuleView;
    use crate::{instruction_set, Opcode, RwasmModuleInner};

    #[test]
    fn view_parses_and_iterates_sections_correctly() {
        // Build a non-trivial module
        let module = RwasmModuleInner {
            code_section: instruction_set! {
                I32Const(1)
                I32Const(2)
                I32Add
                I32Const(3)
                I32Mul
                Drop
            },
            data_section: vec![0xde, 0xad, 0xbe, 0xef, 0x01, 0x02],
            elem_section: vec![10, 20, 30, 0, 0xffff_fffeu32],
            hint_section: b"hello-hint".to_vec(),
        };

        // Encode whole module using the same legacy bincode config
        let cfg = bincode::config::legacy();
        let encoded = bincode::encode_to_vec(&module, cfg).unwrap();

        // Create zero-copy view and ensure we consumed exactly all bytes
        let (view, read) =
            RwasmModuleView::new_checked(&encoded).expect("failed to parse module view");
        assert_eq!(read, encoded.len());

        // Data and hint sections must be exact payload slices
        assert_eq!(view.data_section(), module.data_section.as_slice());
        assert_eq!(view.hint_section(), module.hint_section.as_slice());

        // Elem iterator must decode values correctly and have exact size
        let elems: Vec<u32> = view.elem_section().collect();
        assert_eq!(elems.as_slice(), module.elem_section.as_slice());

        // Instruction opcodes iterator must match original
        let from_view: Vec<Opcode> = view.code_section().opcodes().collect();
        let original: Vec<Opcode> = module.code_section.iter().copied().collect();
        assert_eq!(from_view, original);

        // Raw instruction bytes must equal to independently encoded InstructionSet
        let encoded_code: Vec<u8> = bincode::encode_to_vec(&module.code_section, cfg).unwrap();
        assert_eq!(view.code_section().raw(), encoded_code.as_slice());
    }

    #[test]
    fn view_handles_empty_module() {
        let module = RwasmModuleInner::default();
        let cfg = bincode::config::legacy();
        let encoded = bincode::encode_to_vec(&module, cfg).unwrap();
        let (view, read) =
            RwasmModuleView::new_checked(&encoded).expect("failed to parse empty module");
        assert_eq!(read, encoded.len());
        assert!(view.data_section().is_empty());
        assert!(view.hint_section().is_empty());
        let code = view.code_section();
        assert_eq!(code.len(), 0);
        assert!(code.is_empty());
        let elems: Vec<u32> = view.elem_section().collect();
        assert_eq!(elems.len(), 0);
    }
}

use crate::{
    types::{
        AddressOffset,
        BlockFuel,
        BranchOffset,
        BranchTableTargets,
        CompiledFunc,
        DataSegmentIdx,
        ElementSegmentIdx,
        GlobalIdx,
        LocalDepth,
        MaxStackHeight,
        Opcode,
        SignatureIdx,
        TableIdx,
        UntypedValue,
    },
    CompilationError,
    SysFuncIdx,
};
use alloc::{vec, vec::Vec};
use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{AllowedEnumVariants, DecodeError, EncodeError},
    Decode,
    Encode,
};
use core::ops::{Deref, DerefMut};

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct InstructionSet {
    pub instr: Vec<Opcode>,
}

impl Deref for InstructionSet {
    type Target = Vec<Opcode>;

    fn deref(&self) -> &Self::Target {
        &self.instr
    }
}

impl DerefMut for InstructionSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.instr
    }
}

impl Default for InstructionSet {
    fn default() -> Self {
        Self { instr: vec![] }
    }
}

macro_rules! impl_opcode {
    ($opcode:ident($data_type:ident)) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >]<I: TryInto<$data_type>>(&mut self, value: I) -> u32 {
                self.push(Opcode::$opcode(value.try_into().unwrap_or_else(|_| unreachable!())))
            }
        }
    };
    ($opcode:ident) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >](&mut self) -> u32 {
                self.push(Opcode::$opcode)
            }
        }
    };
}

impl InstructionSet {
    pub fn new() -> Self {
        Self { instr: vec![] }
    }

    pub fn push(&mut self, opcode: Opcode) -> u32 {
        let idx = self.instr.len() as u32;
        self.instr.push(opcode);
        idx
    }

    pub fn clear(&mut self) {
        self.instr.clear();
    }

    pub fn is_return_last(&self) -> bool {
        self.instr
            .last()
            .map(|instr| match instr {
                Opcode::Return
                | Opcode::ReturnCall(_)
                | Opcode::ReturnCallInternal(_)
                | Opcode::ReturnCallIndirect(_) => true,
                _ => false,
            })
            .unwrap_or_default()
    }

    pub fn finalize(&mut self, inject_return: bool) {
        if inject_return && !self.is_return_last() {
            self.op_return();
        }
    }

    pub fn last_nth_mut(&mut self, offset: usize) -> Option<&mut Opcode> {
        self.instr.iter_mut().rev().nth(offset)
    }

    impl_opcode!(LocalGet(LocalDepth));
    impl_opcode!(LocalSet(LocalDepth));
    impl_opcode!(LocalTee(LocalDepth));
    impl_opcode!(Br(BranchOffset));
    impl_opcode!(BrIfEqz(BranchOffset));
    impl_opcode!(BrIfNez(BranchOffset));
    impl_opcode!(BrTable(BranchTableTargets));
    impl_opcode!(Unreachable);
    impl_opcode!(ConsumeFuel(BlockFuel));
    impl_opcode!(Return);
    impl_opcode!(ReturnCallInternal(CompiledFunc));
    impl_opcode!(ReturnCall(SysFuncIdx));
    impl_opcode!(ReturnCallIndirect(SignatureIdx));
    impl_opcode!(CallInternal(CompiledFunc));
    impl_opcode!(Call(SysFuncIdx));
    impl_opcode!(CallIndirect(SignatureIdx));
    impl_opcode!(SignatureCheck(SignatureIdx));
    impl_opcode!(StackCheck(MaxStackHeight));
    impl_opcode!(Drop);
    impl_opcode!(Select);
    impl_opcode!(GlobalGet(GlobalIdx));
    impl_opcode!(GlobalSet(GlobalIdx));
    impl_opcode!(I32Load(AddressOffset));
    impl_opcode!(I32Load8S(AddressOffset));
    impl_opcode!(I32Load8U(AddressOffset));
    impl_opcode!(I32Load16S(AddressOffset));
    impl_opcode!(I32Load16U(AddressOffset));
    impl_opcode!(I32Store(AddressOffset));
    impl_opcode!(I32Store8(AddressOffset));
    impl_opcode!(I32Store16(AddressOffset));
    impl_opcode!(MemorySize);
    impl_opcode!(MemoryGrow);
    impl_opcode!(MemoryFill);
    impl_opcode!(MemoryCopy);
    impl_opcode!(MemoryInit(DataSegmentIdx));
    impl_opcode!(DataDrop(DataSegmentIdx));
    impl_opcode!(TableSize(TableIdx));
    impl_opcode!(TableGrow(TableIdx));
    impl_opcode!(TableFill(TableIdx));
    impl_opcode!(TableGet(TableIdx));
    impl_opcode!(TableSet(TableIdx));
    impl_opcode!(TableCopy(TableIdx));
    impl_opcode!(TableInit(ElementSegmentIdx));
    impl_opcode!(ElemDrop(ElementSegmentIdx));
    impl_opcode!(RefFunc(CompiledFunc));
    impl_opcode!(I32Const(UntypedValue));
    impl_opcode!(I32Eqz);
    impl_opcode!(I32Eq);
    impl_opcode!(I32Ne);
    impl_opcode!(I32LtS);
    impl_opcode!(I32LtU);
    impl_opcode!(I32GtS);
    impl_opcode!(I32GtU);
    impl_opcode!(I32LeS);
    impl_opcode!(I32LeU);
    impl_opcode!(I32GeS);
    impl_opcode!(I32GeU);
    impl_opcode!(I32Clz);
    impl_opcode!(I32Ctz);
    impl_opcode!(I32Popcnt);
    impl_opcode!(I32Add);
    impl_opcode!(I32Sub);
    impl_opcode!(I32Mul);
    impl_opcode!(I32DivS);
    impl_opcode!(I32DivU);
    impl_opcode!(I32RemS);
    impl_opcode!(I32RemU);
    impl_opcode!(I32And);
    impl_opcode!(I32Or);
    impl_opcode!(I32Xor);
    impl_opcode!(I32Shl);
    impl_opcode!(I32ShrS);
    impl_opcode!(I32ShrU);
    impl_opcode!(I32Rotl);
    impl_opcode!(I32Rotr);
    impl_opcode!(I32WrapI64);
    impl_opcode!(I32Extend8S);
    impl_opcode!(I32Extend16S);

    impl_opcode!(F32Load(AddressOffset));
    impl_opcode!(F64Load(AddressOffset));
    impl_opcode!(F32Store(AddressOffset));
    impl_opcode!(F64Store(AddressOffset));
    impl_opcode!(F32Eq);
    impl_opcode!(F32Ne);
    impl_opcode!(F32Lt);
    impl_opcode!(F32Gt);
    impl_opcode!(F32Le);
    impl_opcode!(F32Ge);
    impl_opcode!(F64Eq);
    impl_opcode!(F64Ne);
    impl_opcode!(F64Lt);
    impl_opcode!(F64Gt);
    impl_opcode!(F64Le);
    impl_opcode!(F64Ge);
    impl_opcode!(F32Abs);
    impl_opcode!(F32Neg);
    impl_opcode!(F32Ceil);
    impl_opcode!(F32Floor);
    impl_opcode!(F32Trunc);
    impl_opcode!(F32Nearest);
    impl_opcode!(F32Sqrt);
    impl_opcode!(F32Add);
    impl_opcode!(F32Sub);
    impl_opcode!(F32Mul);
    impl_opcode!(F32Div);
    impl_opcode!(F32Min);
    impl_opcode!(F32Max);
    impl_opcode!(F32Copysign);
    impl_opcode!(F64Abs);
    impl_opcode!(F64Neg);
    impl_opcode!(F64Ceil);
    impl_opcode!(F64Floor);
    impl_opcode!(F64Trunc);
    impl_opcode!(F64Nearest);
    impl_opcode!(F64Sqrt);
    impl_opcode!(F64Add);
    impl_opcode!(F64Sub);
    impl_opcode!(F64Mul);
    impl_opcode!(F64Div);
    impl_opcode!(F64Min);
    impl_opcode!(F64Max);
    impl_opcode!(F64Copysign);
    impl_opcode!(I32TruncF32S);
    impl_opcode!(I32TruncF32U);
    impl_opcode!(I32TruncF64S);
    impl_opcode!(I32TruncF64U);
    impl_opcode!(I64TruncF32S);
    impl_opcode!(I64TruncF32U);
    impl_opcode!(I64TruncF64S);
    impl_opcode!(I64TruncF64U);
    impl_opcode!(F32ConvertI32S);
    impl_opcode!(F32ConvertI32U);
    impl_opcode!(F32ConvertI64S);
    impl_opcode!(F32ConvertI64U);
    impl_opcode!(F32DemoteF64);
    impl_opcode!(F64ConvertI32S);
    impl_opcode!(F64ConvertI32U);
    impl_opcode!(F64ConvertI64S);
    impl_opcode!(F64ConvertI64U);
    impl_opcode!(F64PromoteF32);
    impl_opcode!(I32TruncSatF32S);
    impl_opcode!(I32TruncSatF32U);
    impl_opcode!(I32TruncSatF64S);
    impl_opcode!(I32TruncSatF64U);
    impl_opcode!(I64TruncSatF32S);
    impl_opcode!(I64TruncSatF32U);
    impl_opcode!(I64TruncSatF64S);
    impl_opcode!(I64TruncSatF64U);

    /// Adds the given `delta` amount of fuel to the [`ConsumeFuel`] instruction `instr`.
    ///
    /// # Panics
    ///
    /// - If `instr` does not resolve to a [`ConsumeFuel`] instruction.
    /// - If the amount of consumed fuel for `instr` overflows.
    ///
    /// [`ConsumeFuel`]: enum.Instruction.html#variant.ConsumeFuel
    pub fn bump_fuel_consumption(
        &mut self,
        instr: u32,
        delta: u64,
    ) -> Result<(), CompilationError> {
        match &mut self.instr[instr as usize] {
            Opcode::ConsumeFuel(fuel) => fuel.bump_by(delta),
            _ => unreachable!("instruction {} is not a `ConsumeFuel` instruction", instr),
        }
    }
}

#[macro_export]
macro_rules! instruction_set_internal {
    // Nothing left to do
    ($code:ident, ) => {};
    ($code:ident, $x:ident ($v:expr) $($rest:tt)*) => {
        _ = crate::types::Opcode::$x;
        paste::paste! {
            $code.[< op_ $x:snake >]($v);
        }
        $crate::instruction_set_internal!($code, $($rest)*);
    };
    // Default opcode without any inputs
    ($code:ident, $x:ident $($rest:tt)*) => {
        _ = crate::types::Opcode::$x;
        paste::paste! {
            $code.[< op_ $x:snake >]();
        }
        $crate::instruction_set_internal!($code, $($rest)*);
    };
    // Function calls
    ($code:ident, .$function:ident ($($args:expr),* $(,)?) $($rest:tt)*) => {
        $code.$function($($args,)*);
        $crate::instruction_set_internal!($code, $($rest)*);
    };
}

#[macro_export]
macro_rules! instruction_set {
    ($($args:tt)*) => {{
        let mut code = $crate::types::InstructionSet::new();
        $crate::instruction_set_internal!(code, $($args)*);
        code
    }};
}

impl Encode for InstructionSet {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        let length = self.instr.len() as u64;
        Encode::encode(&length, encoder)?;
        for instr in &self.instr {
            Encode::encode(instr, encoder)?;
            // let discriminant = core::mem::discriminant(instr);
            // encode_instruction_data(&instr.1, encoder)?;
        }
        Ok(())
    }
}

impl<Context> Decode<Context> for InstructionSet {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        fn instruction_not_found_err(instr_value: u8) -> DecodeError {
            static RANGE: AllowedEnumVariants = AllowedEnumVariants::Range { min: 0, max: 0xc6 };
            DecodeError::UnexpectedVariant {
                type_name: "Instruction",
                allowed: &RANGE,
                found: instr_value as u32,
            }
        }
        let length: u64 = Decode::decode(decoder)?;
        let mut instr: Vec<Opcode> = Vec::with_capacity(length as usize);
        for _ in 0..length as usize {
            let opcode: Opcode = Decode::decode(decoder)?;
            instr.push(opcode);
        }
        Ok(Self { instr })
    }
}

impl core::fmt::Display for InstructionSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, instr) in self.instr.iter().enumerate() {
            writeln!(f, " - {:0>4x}: {}", i, instr)?;
        }
        Ok(())
    }
}

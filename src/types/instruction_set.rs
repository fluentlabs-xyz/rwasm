mod add_sub;
mod bitwise;
mod compare;
mod conv;
mod div_s;
mod div_u;
mod memory;
mod mul;
mod rem_s;
mod rem_u;
mod table;

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
    TrapCode,
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
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
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
    ($opcode:ident($data1_type:ident, $data2_type:ident)) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >]<I1: TryInto<$data1_type>, I2: TryInto<$data2_type>>(&mut self, value1: I1, value2: I2) {
                self.push(Opcode::$opcode(
                    value1.try_into().unwrap_or_else(|_| unreachable!()),
                    value2.try_into().unwrap_or_else(|_| unreachable!())
                ));
            }
        }
    };
    ($opcode:ident($data_type:ident)) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >]<I: TryInto<$data_type>>(&mut self, value: I) {
                self.push(Opcode::$opcode(value.try_into().unwrap_or_else(|_| unreachable!())));
            }
        }
    };
    ($opcode:ident) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >](&mut self) {
                self.push(Opcode::$opcode);
            }
        }
    };
}

macro_rules! impl_fpu_emitter {
    ($opcode:ident($data_type:ident)) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >]<I: TryInto<$data_type>>(&mut self, value: I) {
                #[cfg(feature = "fpu")]
                self.push(Opcode::$opcode(value.try_into().unwrap_or_else(|_| unreachable!())));
                #[cfg(not(feature = "fpu"))]
                self.push(Opcode::Trap(TrapCode::IllegalOpcode));
            }
        }
    };
    ($opcode:ident) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >](&mut self) {
                #[cfg(feature = "fpu")]
                self.push(Opcode::$opcode);
                #[cfg(not(feature = "fpu"))]
                self.push(Opcode::Trap(TrapCode::IllegalOpcode));
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

    pub fn loc(&self) -> u32 {
        self.instr.len() as u32
    }

    pub fn finalize(&mut self, inject_return: bool) {
        if inject_return && !self.is_return_last() {
            self.op_return();
        }
    }

    pub fn last_nth_mut(&mut self, offset: usize) -> Option<&mut Opcode> {
        self.instr.iter_mut().rev().nth(offset)
    }

    pub fn op_dup(&mut self) {
        self.op_local_get(1);
    }

    pub fn op_swap(&mut self) {
        self.op_local_get(2);
        self.op_local_get(2);
        self.op_local_set(3);
        self.op_local_set(1);
    }

    impl_opcode!(Unreachable);
    impl_opcode!(Trap(TrapCode));
    impl_opcode!(LocalGet(LocalDepth));
    impl_opcode!(LocalSet(LocalDepth));
    impl_opcode!(LocalTee(LocalDepth));
    impl_opcode!(Br(BranchOffset));
    impl_opcode!(BrIfEqz(BranchOffset));
    impl_opcode!(BrIfNez(BranchOffset));
    impl_opcode!(BrTable(BranchTableTargets));
    impl_opcode!(ConsumeFuel(BlockFuel));
    impl_opcode!(ConsumeFuelStack);
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
    impl_opcode!(TableCopy(TableIdx, TableIdx));
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
    impl_opcode!(I32Extend8S);
    impl_opcode!(I32Extend16S);

    // fpu opcodes (emits trap for disable fpu feature flag)
    impl_fpu_emitter!(F32Load(AddressOffset));
    impl_fpu_emitter!(F64Load(AddressOffset));
    impl_fpu_emitter!(F32Store(AddressOffset));
    impl_fpu_emitter!(F64Store(AddressOffset));
    impl_fpu_emitter!(F32Eq);
    impl_fpu_emitter!(F32Ne);
    impl_fpu_emitter!(F32Lt);
    impl_fpu_emitter!(F32Gt);
    impl_fpu_emitter!(F32Le);
    impl_fpu_emitter!(F32Ge);
    impl_fpu_emitter!(F64Eq);
    impl_fpu_emitter!(F64Ne);
    impl_fpu_emitter!(F64Lt);
    impl_fpu_emitter!(F64Gt);
    impl_fpu_emitter!(F64Le);
    impl_fpu_emitter!(F64Ge);
    impl_fpu_emitter!(F32Abs);
    impl_fpu_emitter!(F32Neg);
    impl_fpu_emitter!(F32Ceil);
    impl_fpu_emitter!(F32Floor);
    impl_fpu_emitter!(F32Trunc);
    impl_fpu_emitter!(F32Nearest);
    impl_fpu_emitter!(F32Sqrt);
    impl_fpu_emitter!(F32Add);
    impl_fpu_emitter!(F32Sub);
    impl_fpu_emitter!(F32Mul);
    impl_fpu_emitter!(F32Div);
    impl_fpu_emitter!(F32Min);
    impl_fpu_emitter!(F32Max);
    impl_fpu_emitter!(F32Copysign);
    impl_fpu_emitter!(F64Abs);
    impl_fpu_emitter!(F64Neg);
    impl_fpu_emitter!(F64Ceil);
    impl_fpu_emitter!(F64Floor);
    impl_fpu_emitter!(F64Trunc);
    impl_fpu_emitter!(F64Nearest);
    impl_fpu_emitter!(F64Sqrt);
    impl_fpu_emitter!(F64Add);
    impl_fpu_emitter!(F64Sub);
    impl_fpu_emitter!(F64Mul);
    impl_fpu_emitter!(F64Div);
    impl_fpu_emitter!(F64Min);
    impl_fpu_emitter!(F64Max);
    impl_fpu_emitter!(F64Copysign);
    impl_fpu_emitter!(I32TruncF32S);
    impl_fpu_emitter!(I32TruncF32U);
    impl_fpu_emitter!(I32TruncF64S);
    impl_fpu_emitter!(I32TruncF64U);
    impl_fpu_emitter!(I64TruncF32S);
    impl_fpu_emitter!(I64TruncF32U);
    impl_fpu_emitter!(I64TruncF64S);
    impl_fpu_emitter!(I64TruncF64U);
    impl_fpu_emitter!(F32ConvertI32S);
    impl_fpu_emitter!(F32ConvertI32U);
    impl_fpu_emitter!(F32ConvertI64S);
    impl_fpu_emitter!(F32ConvertI64U);
    impl_fpu_emitter!(F32DemoteF64);
    impl_fpu_emitter!(F64ConvertI32S);
    impl_fpu_emitter!(F64ConvertI32U);
    impl_fpu_emitter!(F64ConvertI64S);
    impl_fpu_emitter!(F64ConvertI64U);
    impl_fpu_emitter!(F64PromoteF32);
    impl_fpu_emitter!(I32TruncSatF32S);
    impl_fpu_emitter!(I32TruncSatF32U);
    impl_fpu_emitter!(I32TruncSatF64S);
    impl_fpu_emitter!(I32TruncSatF64U);
    impl_fpu_emitter!(I64TruncSatF32S);
    impl_fpu_emitter!(I64TruncSatF32U);
    impl_fpu_emitter!(I64TruncSatF64S);
    impl_fpu_emitter!(I64TruncSatF64U);

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
        delta: u32,
    ) -> Result<(), CompilationError> {
        let fuel = match &mut self.instr[instr as usize] {
            Opcode::ConsumeFuel(fuel) => fuel,
            _ => unreachable!("instruction {} is not a `ConsumeFuel` instruction", instr),
        };
        *fuel = fuel
            .checked_add(delta)
            .ok_or(CompilationError::BlockFuelOutOfBounds)?;
        Ok(())
    }
}

#[macro_export]
macro_rules! instruction_set_internal {
    // Nothing left to do
    ($code:ident, ) => {};
    ($code:ident, $x:ident ($v:expr) $($rest:tt)*) => {
        _ = $crate::Opcode::$x;
        paste::paste! {
            $code.[< op_ $x:snake >]($v);
        }
        $crate::instruction_set_internal!($code, $($rest)*);
    };
    // Default opcode without any inputs
    ($code:ident, $x:ident $($rest:tt)*) => {
        _ = $crate::Opcode::$x;
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
        let mut code = $crate::InstructionSet::new();
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
        }
        Ok(())
    }
}

impl<Context> Decode<Context> for InstructionSet {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        #[allow(dead_code)]
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

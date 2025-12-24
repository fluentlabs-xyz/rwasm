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
        AddressOffset, BlockFuel, BranchOffset, BranchTableTargets, CompiledFunc, DataSegmentIdx,
        ElementSegmentIdx, GlobalIdx, LocalDepth, MaxStackHeight, Opcode, SignatureIdx, TableIdx,
        UntypedValue,
    },
    CompilationError, SysFuncIdx, TrapCode,
};
use alloc::{vec, vec::Vec};
use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{DecodeError, EncodeError},
    Decode, Encode,
};
use core::ops::{Deref, DerefMut};

#[derive(Default, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Compact, linear sequence of rwasm opcodes produced by the compiler.
/// Acts as the executable bytecode for the interpreter and supports simple editing during lowering.
/// The layout is stable and indexable to allow cheap jumps and metadata lookups.
pub struct InstructionSet {
    /// Backing storage for encoded opcodes in program order.
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

macro_rules! impl_basic_opcode {
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

macro_rules! impl_fpu_opcode {
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

    pub fn get_nth_mut(&mut self, n: usize) -> Option<&mut Opcode> {
        self.instr.get_mut(n)
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

    // stack/system
    impl_basic_opcode!(Unreachable);
    impl_basic_opcode!(Trap(TrapCode));
    impl_basic_opcode!(LocalGet(LocalDepth));
    impl_basic_opcode!(LocalSet(LocalDepth));
    impl_basic_opcode!(LocalTee(LocalDepth));
    impl_basic_opcode!(Br(BranchOffset));
    impl_basic_opcode!(BrIfEqz(BranchOffset));
    impl_basic_opcode!(BrIfNez(BranchOffset));
    impl_basic_opcode!(BrTable(BranchTableTargets));
    impl_basic_opcode!(ConsumeFuel(BlockFuel));
    impl_basic_opcode!(ConsumeFuelStack);
    impl_basic_opcode!(Return);
    impl_basic_opcode!(ReturnCallInternal(CompiledFunc));
    impl_basic_opcode!(ReturnCall(SysFuncIdx));
    impl_basic_opcode!(ReturnCallIndirect(SignatureIdx));
    impl_basic_opcode!(CallInternal(CompiledFunc));
    impl_basic_opcode!(Call(SysFuncIdx));
    impl_basic_opcode!(CallIndirect(SignatureIdx));
    impl_basic_opcode!(SignatureCheck(SignatureIdx));
    impl_basic_opcode!(StackCheck(MaxStackHeight));
    impl_basic_opcode!(Drop);
    impl_basic_opcode!(Select);
    impl_basic_opcode!(GlobalGet(GlobalIdx));
    impl_basic_opcode!(GlobalSet(GlobalIdx));

    // memory
    impl_basic_opcode!(I32Load(AddressOffset));
    impl_basic_opcode!(I32Load8S(AddressOffset));
    impl_basic_opcode!(I32Load8U(AddressOffset));
    impl_basic_opcode!(I32Load16S(AddressOffset));
    impl_basic_opcode!(I32Load16U(AddressOffset));
    impl_basic_opcode!(I32Store(AddressOffset));
    impl_basic_opcode!(I32Store8(AddressOffset));
    impl_basic_opcode!(I32Store16(AddressOffset));
    impl_basic_opcode!(MemorySize);
    impl_basic_opcode!(MemoryGrow);
    impl_basic_opcode!(MemoryFill);
    impl_basic_opcode!(MemoryCopy);
    impl_basic_opcode!(MemoryInit(DataSegmentIdx));
    impl_basic_opcode!(DataDrop(DataSegmentIdx));

    // table
    impl_basic_opcode!(TableSize(TableIdx));
    impl_basic_opcode!(TableGrow(TableIdx));
    impl_basic_opcode!(TableFill(TableIdx));
    impl_basic_opcode!(TableGet(TableIdx));
    impl_basic_opcode!(TableSet(TableIdx));
    impl_basic_opcode!(TableCopy(TableIdx, TableIdx));
    impl_basic_opcode!(TableInit(ElementSegmentIdx));
    impl_basic_opcode!(ElemDrop(ElementSegmentIdx));
    impl_basic_opcode!(RefFunc(CompiledFunc));
    impl_basic_opcode!(I32Const(UntypedValue));

    // alu
    impl_basic_opcode!(I32Eqz);
    impl_basic_opcode!(I32Eq);
    impl_basic_opcode!(I32Ne);
    impl_basic_opcode!(I32LtS);
    impl_basic_opcode!(I32LtU);
    impl_basic_opcode!(I32GtS);
    impl_basic_opcode!(I32GtU);
    impl_basic_opcode!(I32LeS);
    impl_basic_opcode!(I32LeU);
    impl_basic_opcode!(I32GeS);
    impl_basic_opcode!(I32GeU);
    impl_basic_opcode!(I32Clz);
    impl_basic_opcode!(I32Ctz);
    impl_basic_opcode!(I32Popcnt);
    impl_basic_opcode!(I32Add);
    impl_basic_opcode!(I32Sub);
    impl_basic_opcode!(I32Mul);
    impl_basic_opcode!(I32DivS);
    impl_basic_opcode!(I32DivU);
    impl_basic_opcode!(I32RemS);
    impl_basic_opcode!(I32RemU);
    impl_basic_opcode!(I32And);
    impl_basic_opcode!(I32Or);
    impl_basic_opcode!(I32Xor);
    impl_basic_opcode!(I32Shl);
    impl_basic_opcode!(I32ShrS);
    impl_basic_opcode!(I32ShrU);
    impl_basic_opcode!(I32Rotl);
    impl_basic_opcode!(I32Rotr);
    impl_basic_opcode!(I32Extend8S);
    impl_basic_opcode!(I32Extend16S);
    impl_basic_opcode!(I32Mul64);
    impl_basic_opcode!(I32Add64);

    // fpu opcodes (emits trap for disable fpu feature flag)
    impl_fpu_opcode!(F32Load(AddressOffset));
    impl_fpu_opcode!(F64Load(AddressOffset));
    impl_fpu_opcode!(F32Store(AddressOffset));
    impl_fpu_opcode!(F64Store(AddressOffset));
    impl_fpu_opcode!(F32Eq);
    impl_fpu_opcode!(F32Ne);
    impl_fpu_opcode!(F32Lt);
    impl_fpu_opcode!(F32Gt);
    impl_fpu_opcode!(F32Le);
    impl_fpu_opcode!(F32Ge);
    impl_fpu_opcode!(F64Eq);
    impl_fpu_opcode!(F64Ne);
    impl_fpu_opcode!(F64Lt);
    impl_fpu_opcode!(F64Gt);
    impl_fpu_opcode!(F64Le);
    impl_fpu_opcode!(F64Ge);
    impl_fpu_opcode!(F32Abs);
    impl_fpu_opcode!(F32Neg);
    impl_fpu_opcode!(F32Ceil);
    impl_fpu_opcode!(F32Floor);
    impl_fpu_opcode!(F32Trunc);
    impl_fpu_opcode!(F32Nearest);
    impl_fpu_opcode!(F32Sqrt);
    impl_fpu_opcode!(F32Add);
    impl_fpu_opcode!(F32Sub);
    impl_fpu_opcode!(F32Mul);
    impl_fpu_opcode!(F32Div);
    impl_fpu_opcode!(F32Min);
    impl_fpu_opcode!(F32Max);
    impl_fpu_opcode!(F32Copysign);
    impl_fpu_opcode!(F64Abs);
    impl_fpu_opcode!(F64Neg);
    impl_fpu_opcode!(F64Ceil);
    impl_fpu_opcode!(F64Floor);
    impl_fpu_opcode!(F64Trunc);
    impl_fpu_opcode!(F64Nearest);
    impl_fpu_opcode!(F64Sqrt);
    impl_fpu_opcode!(F64Add);
    impl_fpu_opcode!(F64Sub);
    impl_fpu_opcode!(F64Mul);
    impl_fpu_opcode!(F64Div);
    impl_fpu_opcode!(F64Min);
    impl_fpu_opcode!(F64Max);
    impl_fpu_opcode!(F64Copysign);
    impl_fpu_opcode!(I32TruncF32S);
    impl_fpu_opcode!(I32TruncF32U);
    impl_fpu_opcode!(I32TruncF64S);
    impl_fpu_opcode!(I32TruncF64U);
    impl_fpu_opcode!(I64TruncF32S);
    impl_fpu_opcode!(I64TruncF32U);
    impl_fpu_opcode!(I64TruncF64S);
    impl_fpu_opcode!(I64TruncF64U);
    impl_fpu_opcode!(F32ConvertI32S);
    impl_fpu_opcode!(F32ConvertI32U);
    impl_fpu_opcode!(F32ConvertI64S);
    impl_fpu_opcode!(F32ConvertI64U);
    impl_fpu_opcode!(F32DemoteF64);
    impl_fpu_opcode!(F64ConvertI32S);
    impl_fpu_opcode!(F64ConvertI32U);
    impl_fpu_opcode!(F64ConvertI64S);
    impl_fpu_opcode!(F64ConvertI64U);
    impl_fpu_opcode!(F64PromoteF32);
    impl_fpu_opcode!(I32TruncSatF32S);
    impl_fpu_opcode!(I32TruncSatF32U);
    impl_fpu_opcode!(I32TruncSatF64S);
    impl_fpu_opcode!(I32TruncSatF64U);
    impl_fpu_opcode!(I64TruncSatF32S);
    impl_fpu_opcode!(I64TruncSatF32U);
    impl_fpu_opcode!(I64TruncSatF64S);
    impl_fpu_opcode!(I64TruncSatF64U);

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

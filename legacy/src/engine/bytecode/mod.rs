//! The instruction architecture of the `wasmi` interpreter.

mod utils;

mod instr_meta;
mod stack_height;
#[cfg(test)]
mod tests;

pub use self::utils::{
    AddressOffset,
    BlockFuel,
    BranchOffset,
    BranchTableTargets,
    DataSegmentIdx,
    DropKeep,
    DropKeepError,
    ElementSegmentIdx,
    F64Const32,
    FuncIdx,
    GlobalIdx,
    LocalDepth,
    SignatureIdx,
    TableIdx,
};
use super::{const_pool::ConstRef, CompiledFunc, TranslationError};
use crate::core::{UntypedValue, F32};
#[cfg(feature = "std")]
use core::{
    fmt,
    fmt::{Debug, Formatter},
};

/// The internal `wasmi` bytecode that is stored for Wasm functions.
///
/// # Note
///
/// This representation slightly differs from WebAssembly instructions.
///
/// For example the `BrTable` instruction is unrolled into separate instructions
/// each representing either the `BrTable` head or one of its branching targets.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(strum_macros::EnumIter))]
pub enum Instruction {
    LocalGet(LocalDepth),
    LocalSet(LocalDepth),
    LocalTee(LocalDepth),
    /// An unconditional branch.
    Br(BranchOffset),
    /// Branches if the top-most stack value is equal to zero.
    BrIfEqz(BranchOffset),
    /// Branches if the top-most stack value is _not_ equal to zero.
    BrIfNez(BranchOffset),
    /// An unconditional branch.
    ///
    /// This operation also adjust the underlying value stack if necessary.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by a [`Instruction::Return`]
    /// which stores information about the [`DropKeep`] behavior of the
    /// [`Instruction::Br`]. The [`Instruction::Return`] will never be executed
    /// and only acts as parameter storage for this instruction.
    BrAdjust(BranchOffset),
    /// Branches if the top-most stack value is _not_ equal to zero.
    ///
    /// This operation also adjust the underlying value stack if necessary.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by a [`Instruction::Return`]
    /// which stores information about the [`DropKeep`] behavior of the
    /// [`Instruction::BrIfNez`]. The [`Instruction::Return`] will never be executed
    /// and only acts as parameter storage for this instruction.
    BrAdjustIfNez(BranchOffset),
    /// Branch table with a set number of branching targets.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by exactly as many unconditional
    /// branch instructions as determined by [`BranchTableTargets`]. Branch
    /// instructions that may follow are [`Instruction::Br] and [`Instruction::Return`].
    BrTable(BranchTableTargets),
    Unreachable,
    ConsumeFuel(BlockFuel),
    Return(DropKeep),
    ReturnIfNez(DropKeep),
    /// Tail calls an internal (compiled) function.
    ///
    /// # Note
    ///
    /// This instruction can be used for calls to functions that are engine internal
    /// (or compiled) and acts as an optimization for those common cases.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by an [`Instruction::Return`] that
    /// encodes the [`DropKeep`] parameter. Note that the [`Instruction::Return`]
    /// only acts as a storage for the parameter of the [`Instruction::ReturnCall`]
    /// and will never be executed by itself.
    ReturnCallInternal(CompiledFunc),
    /// Tail calling `func`.
    ///
    /// # Note
    ///
    /// Since [`Instruction::ReturnCallInternal`] should be used for all functions internal
    /// (or compiled) to the engine this instruction should mainly be used for tail calling
    /// imported functions. However, it is a general form that can technically be used
    /// for both.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by an [`Instruction::Return`] that
    /// encodes the [`DropKeep`] parameter. Note that the [`Instruction::Return`]
    /// only acts as a storage for the parameter of the [`Instruction::ReturnCall`]
    /// and will never be executed by itself.
    ReturnCall(FuncIdx),
    /// Tail calling a function indirectly.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by an [`Instruction::Return`] that
    /// encodes the [`DropKeep`] parameter as well as an [`Instruction::TableGet`]
    /// that encodes the [`TableIdx`] parameter. Note that both, [`Instruction::Return`]
    /// and [`Instruction::TableGet`] only act as a storage for parameters to the
    /// [`Instruction::ReturnCallIndirect`] and will never be executed by themselves.
    ReturnCallIndirect(SignatureIdx),
    /// Calls an internal (compiled) function.
    ///
    /// # Note
    ///
    /// This instruction can be used for calls to functions that are engine internal
    /// (or compiled) and acts as an optimization for those common cases.
    CallInternal(CompiledFunc),
    /// Calls the function.
    ///
    /// # Note
    ///
    /// Since [`Instruction::CallInternal`] should be used for all functions internal
    /// (or compiled) to the engine this instruction should mainly be used for calling
    /// imported functions. However, it is a general form that can technically be used
    /// for both.
    Call(FuncIdx),
    /// Calling a function indirectly.
    ///
    /// # Encoding
    ///
    /// This [`Instruction`] must be followed by an [`Instruction::TableGet`]
    /// that encodes the [`TableIdx`] parameter. Note that the [`Instruction::TableGet`]
    /// only acts as a storage for the parameter of the [`Instruction::CallIndirect`]
    /// and will never be executed by itself.
    CallIndirect(SignatureIdx),
    SignatureCheck(SignatureIdx),
    StackAlloc {
        max_stack_height: u32,
    },
    Drop,
    Select,
    GlobalGet(GlobalIdx),
    GlobalSet(GlobalIdx),
    I32Load(AddressOffset),
    I64Load(AddressOffset),
    F32Load(AddressOffset),
    F64Load(AddressOffset),
    I32Load8S(AddressOffset),
    I32Load8U(AddressOffset),
    I32Load16S(AddressOffset),
    I32Load16U(AddressOffset),
    I64Load8S(AddressOffset),
    I64Load8U(AddressOffset),
    I64Load16S(AddressOffset),
    I64Load16U(AddressOffset),
    I64Load32S(AddressOffset),
    I64Load32U(AddressOffset),
    I32Store(AddressOffset),
    I64Store(AddressOffset),
    F32Store(AddressOffset),
    F64Store(AddressOffset),
    I32Store8(AddressOffset),
    I32Store16(AddressOffset),
    I64Store8(AddressOffset),
    I64Store16(AddressOffset),
    I64Store32(AddressOffset),
    MemorySize,
    MemoryGrow,
    MemoryFill,
    MemoryCopy,
    MemoryInit(DataSegmentIdx),
    DataDrop(DataSegmentIdx),
    TableSize(TableIdx),
    TableGrow(TableIdx),
    TableFill(TableIdx),
    TableGet(TableIdx),
    TableSet(TableIdx),
    /// Copies elements from one table to another.
    ///
    /// # Note
    ///
    /// It is also possible to copy elements within the same table.
    ///
    /// # Encoding
    ///
    /// The [`TableIdx`] referred to by the [`Instruction::TableCopy`]
    /// represents the `dst` (destination) table. The [`Instruction::TableCopy`]
    /// must be followed by an [`Instruction::TableGet`] which stores a
    /// [`TableIdx`] that refers to the `src` (source) table.
    TableCopy(TableIdx),
    /// Initializes a table given an [`ElementSegmentIdx`].
    ///
    /// # Encoding
    ///
    /// The [`Instruction::TableInit`] must be followed by an
    /// [`Instruction::TableGet`] which stores a [`TableIdx`]
    /// that refers to the table to be initialized.
    TableInit(ElementSegmentIdx),
    ElemDrop(ElementSegmentIdx),
    RefFunc(FuncIdx),
    /// A 32/64-bit constant value.
    I32Const(UntypedValue),
    I64Const(UntypedValue),
    /// A 64-bit float value losslessly encoded as 32-bit float.
    ///
    /// Upon execution the 32-bit float is promoted to the 64-bit float.
    ///
    /// # Note
    ///
    /// This is a space-optimized variant of [`Instruction::ConstRef`] but can
    /// only used for certain float values that fit into a 32-bit float value.
    F32Const(UntypedValue),
    F64Const(UntypedValue),
    /// Pushes a constant value onto the stack.
    ///
    /// The constant value is referred to indirectly by the [`ConstRef`].
    ConstRef(ConstRef),
    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I64Eqz,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,
    F32Abs,
    F32Neg,
    F32Ceil,
    F32Floor,
    F32Trunc,
    F32Nearest,
    F32Sqrt,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,
    F32Copysign,
    F64Abs,
    F64Neg,
    F64Ceil,
    F64Floor,
    F64Trunc,
    F64Nearest,
    F64Sqrt,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,
    F64Copysign,
    I32WrapI64,
    I32TruncF32S,
    I32TruncF32U,
    I32TruncF64S,
    I32TruncF64U,
    I64ExtendI32S,
    I64ExtendI32U,
    I64TruncF32S,
    I64TruncF32U,
    I64TruncF64S,
    I64TruncF64U,
    F32ConvertI32S,
    F32ConvertI32U,
    F32ConvertI64S,
    F32ConvertI64U,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI64U,
    F64PromoteF32,
    I32Extend8S,
    I32Extend16S,
    I64Extend8S,
    I64Extend16S,
    I64Extend32S,
    I32TruncSatF32S,
    I32TruncSatF32U,
    I32TruncSatF64S,
    I32TruncSatF64U,
    I64TruncSatF32S,
    I64TruncSatF32U,
    I64TruncSatF64S,
    I64TruncSatF64U,
}

#[cfg(feature = "std")]
impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name = format!("{:?}", self);
        let name: Vec<_> = name.split('(').collect();
        write!(f, "{}", name[0])
    }
}

impl Instruction {
    /// Creates an [`Instruction::Const32`] from the given `i32` constant value.
    pub fn i32_const(value: i32) -> Self {
        Self::I32Const(UntypedValue::from(i64::from(value)))
    }

    /// Creates an [`Instruction::Const32`] from the given `f32` constant value.
    pub fn f32_const(value: F32) -> Self {
        Self::F32Const(UntypedValue::from(value))
    }

    /// Creates a new `local.get` instruction from the given local depth.
    ///
    /// # Errors
    ///
    /// If the `local_depth` is out of bounds as local depth index.
    pub fn local_get(local_depth: u32) -> Result<Self, TranslationError> {
        Ok(Self::LocalGet(LocalDepth::from(local_depth)))
    }

    /// Creates a new `local.set` instruction from the given local depth.
    ///
    /// # Errors
    ///
    /// If the `local_depth` is out of bounds as local depth index.
    pub fn local_set(local_depth: u32) -> Result<Self, TranslationError> {
        Ok(Self::LocalSet(LocalDepth::from(local_depth)))
    }

    /// Creates a new `local.tee` instruction from the given local depth.
    ///
    /// # Errors
    ///
    /// If the `local_depth` is out of bounds as local depth index.
    pub fn local_tee(local_depth: u32) -> Result<Self, TranslationError> {
        Ok(Self::LocalTee(LocalDepth::from(local_depth)))
    }

    /// Convenience method to create a new `ConsumeFuel` instruction.
    pub fn consume_fuel(amount: u64) -> Result<Self, TranslationError> {
        let block_fuel = BlockFuel::try_from(amount)?;
        Ok(Self::ConsumeFuel(block_fuel))
    }

    pub fn is_supported(&self) -> bool {
        match self {
            Instruction::LocalGet(_)
            | Instruction::LocalSet(_)
            | Instruction::LocalTee(_)
            | Instruction::Br(_)
            | Instruction::BrIfEqz(_)
            | Instruction::BrIfNez(_)
            | Instruction::Unreachable
            | Instruction::ConsumeFuel(_)
            | Instruction::Return(_)
            | Instruction::ReturnIfNez(_)
            | Instruction::Call(_)
            | Instruction::Drop
            | Instruction::Select
            | Instruction::GlobalGet(_)
            | Instruction::GlobalSet(_)
            | Instruction::I32Load(_)
            | Instruction::I64Load(_)
            | Instruction::F32Load(_)
            | Instruction::F64Load(_)
            | Instruction::I32Load8S(_)
            | Instruction::I32Load8U(_)
            | Instruction::I32Load16S(_)
            | Instruction::I32Load16U(_)
            | Instruction::I64Load8S(_)
            | Instruction::I64Load8U(_)
            | Instruction::I64Load16S(_)
            | Instruction::I64Load16U(_)
            | Instruction::I64Load32S(_)
            | Instruction::I64Load32U(_)
            | Instruction::I32Store(_)
            | Instruction::I64Store(_)
            | Instruction::F32Store(_)
            | Instruction::F64Store(_)
            | Instruction::I32Store8(_)
            | Instruction::I32Store16(_)
            | Instruction::I64Store8(_)
            | Instruction::I64Store16(_)
            | Instruction::I64Store32(_)
            | Instruction::MemorySize
            | Instruction::MemoryGrow
            | Instruction::MemoryFill
            | Instruction::MemoryCopy
            | Instruction::MemoryInit(_)
            | Instruction::DataDrop(_)
            | Instruction::TableSize(_)
            | Instruction::TableGrow(_)
            | Instruction::TableFill(_)
            | Instruction::TableGet(_)
            | Instruction::TableSet(_)
            | Instruction::TableCopy(_)
            | Instruction::TableInit(_)
            | Instruction::ElemDrop(_)
            | Instruction::RefFunc(_)
            | Instruction::I32Const(_)
            | Instruction::I64Const(_)
            | Instruction::I32Eqz
            | Instruction::I32Eq
            | Instruction::I32Ne
            | Instruction::I32LtS
            | Instruction::I32LtU
            | Instruction::I32GtS
            | Instruction::I32GtU
            | Instruction::I32LeS
            | Instruction::I32LeU
            | Instruction::I32GeS
            | Instruction::I32GeU
            | Instruction::I64Eqz
            | Instruction::I64Eq
            | Instruction::I64Ne
            | Instruction::I64LtS
            | Instruction::I64LtU
            | Instruction::I64GtS
            | Instruction::I64GtU
            | Instruction::I64LeS
            | Instruction::I64LeU
            | Instruction::I64GeS
            | Instruction::I64GeU
            | Instruction::F32Eq
            | Instruction::F32Ne
            | Instruction::F32Lt
            | Instruction::F32Gt
            | Instruction::F32Le
            | Instruction::F32Ge
            | Instruction::F64Eq
            | Instruction::F64Ne
            | Instruction::F64Lt
            | Instruction::F64Gt
            | Instruction::F64Le
            | Instruction::F64Ge
            | Instruction::I32Clz
            | Instruction::I32Ctz
            | Instruction::I32Popcnt
            | Instruction::I32Add
            | Instruction::I32Sub
            | Instruction::I32Mul
            | Instruction::I32DivS
            | Instruction::I32DivU
            | Instruction::I32RemS
            | Instruction::I32RemU
            | Instruction::I32And
            | Instruction::I32Or
            | Instruction::I32Xor
            | Instruction::I32Shl
            | Instruction::I32ShrS
            | Instruction::I32ShrU
            | Instruction::I32Rotl
            | Instruction::I32Rotr
            | Instruction::I64Clz
            | Instruction::I64Ctz
            | Instruction::I64Popcnt
            | Instruction::I64Add
            | Instruction::I64Sub
            | Instruction::I64Mul
            | Instruction::I64DivS
            | Instruction::I64DivU
            | Instruction::I64RemS
            | Instruction::I64RemU
            | Instruction::I64And
            | Instruction::I64Or
            | Instruction::I64Xor
            | Instruction::I64Shl
            | Instruction::I64ShrS
            | Instruction::I64ShrU
            | Instruction::I64Rotl
            | Instruction::I64Rotr
            | Instruction::F32Abs
            | Instruction::F32Neg
            | Instruction::F32Ceil
            | Instruction::F32Floor
            | Instruction::F32Trunc
            | Instruction::F32Nearest
            | Instruction::F32Sqrt
            | Instruction::F32Add
            | Instruction::F32Sub
            | Instruction::F32Mul
            | Instruction::F32Div
            | Instruction::F32Min
            | Instruction::F32Max
            | Instruction::F32Copysign
            | Instruction::F64Abs
            | Instruction::F64Neg
            | Instruction::F64Ceil
            | Instruction::F64Floor
            | Instruction::F64Trunc
            | Instruction::F64Nearest
            | Instruction::F64Sqrt
            | Instruction::F64Add
            | Instruction::F64Sub
            | Instruction::F64Mul
            | Instruction::F64Div
            | Instruction::F64Min
            | Instruction::F64Max
            | Instruction::F64Copysign
            | Instruction::I32WrapI64
            | Instruction::I32TruncF32S
            | Instruction::I32TruncF32U
            | Instruction::I32TruncF64S
            | Instruction::I32TruncF64U
            | Instruction::I64ExtendI32S
            | Instruction::I64ExtendI32U
            | Instruction::I64TruncF32S
            | Instruction::I64TruncF32U
            | Instruction::I64TruncF64S
            | Instruction::I64TruncF64U
            | Instruction::F32ConvertI32S
            | Instruction::F32ConvertI32U
            | Instruction::F32ConvertI64S
            | Instruction::F32ConvertI64U
            | Instruction::F32DemoteF64
            | Instruction::F64ConvertI32S
            | Instruction::F64ConvertI32U
            | Instruction::F64ConvertI64S
            | Instruction::F64ConvertI64U
            | Instruction::F64PromoteF32
            | Instruction::I32Extend8S
            | Instruction::I32Extend16S
            | Instruction::I64Extend8S
            | Instruction::I64Extend16S
            | Instruction::I64Extend32S
            | Instruction::I32TruncSatF32S
            | Instruction::I32TruncSatF32U
            | Instruction::I32TruncSatF64S
            | Instruction::I32TruncSatF64U
            | Instruction::I64TruncSatF32S
            | Instruction::I64TruncSatF32U
            | Instruction::I64TruncSatF64S
            | Instruction::I64TruncSatF64U => true,
            _ => false,
        }
    }

    /// Increases the fuel consumption of the [`ConsumeFuel`] instruction by `delta`.
    ///
    /// # Panics
    ///
    /// - If `self` is not a [`ConsumeFuel`] instruction.
    /// - If the new fuel consumption overflows the internal `u64` value.
    ///
    /// [`ConsumeFuel`]: Instruction::ConsumeFuel
    pub fn bump_fuel_consumption(&mut self, delta: u64) -> Result<(), TranslationError> {
        match self {
            Self::ConsumeFuel(block_fuel) => block_fuel.bump_by(delta),
            instr => panic!("expected Instruction::ConsumeFuel but found: {:?}", instr),
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct InstrMeta(usize, u16, pub(crate) usize);

impl InstrMeta {
    pub fn new(pos: usize, code: u16, index: usize) -> Self {
        Self(pos, code, index)
    }

    pub fn offset(&self) -> usize {
        self.0
    }

    pub fn opcode(&self) -> u16 {
        self.1
    }

    pub fn index(&self) -> usize {
        self.2
    }
}

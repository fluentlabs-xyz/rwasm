use crate::{RwasmError, TrapCode};
use bincode::{Decode, Encode};

/// A 32-bit encoded `f64` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord)]
pub struct F64Const32(u32);

impl F64Const32 {
    /// Creates an [`Instruction::F64Const32`] from the given `f64` value if possible.
    ///
    /// [`Instruction::F64Const32`]: [`super::Instruction::F64Const32`]
    pub fn new(value: f64) -> Option<Self> {
        let demoted = value as f32;
        if f64::from(demoted).to_bits() != value.to_bits() {
            return None;
        }
        Some(Self(demoted.to_bits()))
    }

    /// Returns the 32-bit encoded `f64` value.
    pub fn to_f64(self) -> f64 {
        f64::from(f32::from_bits(self.0))
    }
}

/// A function index.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct FuncIdx(u32);

impl From<u16> for FuncIdx {
    fn from(index: u16) -> Self {
        Self(index as u32)
    }
}
impl From<u32> for FuncIdx {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl FuncIdx {
    /// Returns the index value as `u32`.
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

/// A table index.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct TableIdx(u32);

impl From<u32> for TableIdx {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl TableIdx {
    /// Returns the index value as `u32`.
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

/// An index of a unique function signature.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct SignatureIdx(u32);

impl From<u32> for SignatureIdx {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl SignatureIdx {
    /// Returns the index value as `u32`.
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

/// A local variable depth access index.
///
/// # Note
///
/// The depth refers to the relative position of a local
/// variable on the value stack with respect to the height
/// of the value stack at the time of access.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct LocalDepth(u32);

impl From<u32> for LocalDepth {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl LocalDepth {
    /// Returns the depth as `usize` index.
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

/// A global variable index.
///
/// # Note
///
/// Refers to a global variable of a [`Store`].
///
/// [`Store`]: [`crate::Store`]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct GlobalIdx(u32);

impl From<u32> for GlobalIdx {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl GlobalIdx {
    /// Returns the index value as `u32`.
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

/// A data segment index.
///
/// # Note
///
/// Refers to a data segment of a [`Store`].
///
/// [`Store`]: [`crate::Store`]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct DataSegmentIdx(u32);

impl From<u32> for DataSegmentIdx {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl DataSegmentIdx {
    /// Returns the index value as `u32`.
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

/// An element segment index.
///
/// # Note
///
/// Refers to a data segment of a [`Store`].
///
/// [`Store`]: [`crate::Store`]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct ElementSegmentIdx(u32);

impl From<u32> for ElementSegmentIdx {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl ElementSegmentIdx {
    /// Returns the index value as `u32`.
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

/// The number of branches of an [`Instruction::BrTable`].
///
/// [`Instruction::BrTable`]: [`super::Instruction::BrTable`]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct BranchTableTargets(u32);

impl TryFrom<usize> for BranchTableTargets {
    type Error = TrapCode;

    fn try_from(index: usize) -> Result<Self, Self::Error> {
        match u32::try_from(index) {
            Ok(index) => Ok(Self(index)),
            Err(_) => Err(TrapCode::BranchTableTargetsOutOfBounds),
        }
    }
}

impl From<u32> for BranchTableTargets {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl BranchTableTargets {
    /// Returns the index value as `usize`.
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct StackAlloc {
    pub max_stack_height: u32,
}

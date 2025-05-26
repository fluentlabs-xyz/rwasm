use bincode::{Decode, Encode};
use core::fmt::Formatter;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
pub enum TrapCode {
    MalformedBinary = 0x01,
    NotAllowedInFuelMode = 0x02,
    UnreachableCodeReached = 0x03,
    MemoryOutOfBounds = 0x04,
    TableOutOfBounds = 0x05,
    IndirectCallToNull = 0x06,
    IntegerDivisionByZero = 0x07,
    IntegerOverflow = 0x08,
    BadConversionToInteger = 0x09,
    StackOverflow = 0x0a,
    BadSignature = 0x0b,
    OutOfFuel = 0x0c,
    GrowthOperationLimited = 0x0d,
    UnresolvedFunction = 0x0e,
    BranchOffsetOutOfBounds = 0x0f,
    BlockFuelOutOfBounds = 0x10,
    BranchTableTargetsOutOfBounds = 0x11,
    UnknownExternalFunction = 0x12,
    ExecutionHalted = 0x13,
}

impl core::fmt::Display for TrapCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            TrapCode::MalformedBinary => write!(f, "malformed binary"),
            TrapCode::UnknownExternalFunction => write!(f, "unknown external function"),
            TrapCode::ExecutionHalted => write!(f, "execution halted"),
            TrapCode::NotAllowedInFuelMode => write!(f, "not allowed in fuel mode"),
            TrapCode::UnreachableCodeReached => write!(f, "unreachable code reached"),
            TrapCode::MemoryOutOfBounds => write!(f, "out of bounds memory access"),
            TrapCode::TableOutOfBounds => {
                write!(f, "undefined element: out of bounds table access")
            }
            TrapCode::IndirectCallToNull => write!(f, "uninitialized element 2"),
            TrapCode::IntegerDivisionByZero => write!(f, "integer divide by zero"),
            TrapCode::IntegerOverflow => write!(f, "integer overflow"),
            TrapCode::BadConversionToInteger => write!(f, "invalid conversion to integer"),
            TrapCode::StackOverflow => write!(f, "call stack exhausted"),
            TrapCode::BadSignature => write!(f, "indirect call type mismatch"),
            TrapCode::OutOfFuel => write!(f, "out of fuel"),
            TrapCode::GrowthOperationLimited => write!(f, "growth operation limited"),
            TrapCode::UnresolvedFunction => write!(f, "unresolved function"),
            TrapCode::BranchOffsetOutOfBounds => write!(f, "branch offset out of bounds"),
            TrapCode::BlockFuelOutOfBounds => write!(f, "block fuel out of bounds"),
            TrapCode::BranchTableTargetsOutOfBounds => {
                write!(f, "branch table targets are out of bounds")
            }
        }
    }
}

use core::fmt::Formatter;

#[derive(Debug)]
pub enum TrapCode {
    MalformedBinary,
    FloatsAreDisabled,
    NotAllowedInFuelMode,
    UnreachableCodeReached,
    MemoryOutOfBounds,
    TableOutOfBounds,
    IndirectCallToNull,
    IntegerDivisionByZero,
    IntegerOverflow,
    BadConversionToInteger,
    StackOverflow,
    BadSignature,
    OutOfFuel,
    GrowthOperationLimited,
    UnresolvedFunction,
    BranchOffsetOutOfBounds,
    BlockFuelOutOfBounds,
    BranchTableTargetsOutOfBounds,
    UnknownExternalFunction,
    ExecutionHalted,
}

impl core::fmt::Display for TrapCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            TrapCode::MalformedBinary => write!(f, "malformed binary"),
            TrapCode::UnknownExternalFunction => write!(f, "unknown external function"),
            TrapCode::ExecutionHalted => write!(f, "execution halted"),
            TrapCode::FloatsAreDisabled => write!(f, "floats are disabled"),
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

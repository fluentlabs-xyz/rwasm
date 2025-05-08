use crate::types::HostError;
use core::fmt::Formatter;

#[derive(Debug)]
pub enum RwasmError {
    MalformedBinary,
    UnknownExternalFunction(u32),
    ExecutionHalted(i32),
    HostInterruption(Box<dyn HostError>),
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
}

impl core::fmt::Display for RwasmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            RwasmError::MalformedBinary => write!(f, "malformed binary"),
            RwasmError::UnknownExternalFunction(_) => write!(f, "unknown external function"),
            RwasmError::ExecutionHalted(_) => write!(f, "execution halted"),
            RwasmError::HostInterruption(_) => write!(f, "host interruption"),
            RwasmError::FloatsAreDisabled => write!(f, "floats are disabled"),
            RwasmError::NotAllowedInFuelMode => write!(f, "not allowed in fuel mode"),
            RwasmError::UnreachableCodeReached => write!(f, "unreachable code reached"),
            RwasmError::MemoryOutOfBounds => write!(f, "out of bounds memory access"),
            RwasmError::TableOutOfBounds => {
                write!(f, "undefined element: out of bounds table access")
            }
            RwasmError::IndirectCallToNull => write!(f, "uninitialized element 2"),
            RwasmError::IntegerDivisionByZero => write!(f, "integer divide by zero"),
            RwasmError::IntegerOverflow => write!(f, "integer overflow"),
            RwasmError::BadConversionToInteger => write!(f, "invalid conversion to integer"),
            RwasmError::StackOverflow => write!(f, "call stack exhausted"),
            RwasmError::BadSignature => write!(f, "indirect call type mismatch"),
            RwasmError::OutOfFuel => write!(f, "out of fuel"),
            RwasmError::GrowthOperationLimited => write!(f, "growth operation limited"),
            RwasmError::UnresolvedFunction => write!(f, "unresolved function"),
            RwasmError::BranchOffsetOutOfBounds => write!(f, "branch offset out of bounds"),
            RwasmError::BlockFuelOutOfBounds => write!(f, "block fuel out of bounds"),
            RwasmError::BranchTableTargetsOutOfBounds => {
                write!(f, "branch table targets are out of bounds")
            }
        }
    }
}

impl RwasmError {
    pub fn unwrap_exit_code(&self) -> i32 {
        match self {
            RwasmError::ExecutionHalted(exit_code) => *exit_code,
            _ => unreachable!("runtime: can't unwrap exit code from error"),
        }
    }
}

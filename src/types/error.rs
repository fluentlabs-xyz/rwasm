use crate::types::HostError;
use alloc::boxed::Box;
use core::fmt::Formatter;

#[derive(Debug)]
pub enum TrapCode {
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
    ExecutionHalted,
    UnknownExternalFunction,
}

impl core::fmt::Display for TrapCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
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
            TrapCode::ExecutionHalted => write!(f, "execution halted"),
            TrapCode::UnknownExternalFunction => write!(f, "unknown external function"),
        }
    }
}

#[derive(Debug)]
pub enum RwasmError {
    MalformedBinary,
    UnknownExternalFunction(u32),
    ExecutionHalted(i32),
    HostInterruption(Box<dyn HostError>),
    FloatsAreDisabled,
    NotAllowedInFuelMode,
    TrapCode(TrapCode),
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
            RwasmError::TrapCode(err) => write!(f, "trap code: {err}"),
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

use bincode::{Decode, Encode};
use core::fmt::Formatter;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(u8)]
pub enum TrapCode {
    UnreachableCodeReached = 0x00,
    MemoryOutOfBounds = 0x01,
    TableOutOfBounds = 0x02,
    IndirectCallToNull = 0x03,
    IntegerDivisionByZero = 0x04,
    IntegerOverflow = 0x05,
    BadConversionToInteger = 0x06,
    StackOverflow = 0x07,
    BadSignature = 0x08,
    OutOfFuel = 0x09,
    UnknownExternalFunction = 0x0a,
    IllegalOpcode = 0x0b,
    // a special trap code for interrupting an execution,
    // it saves the latest registers for IP and SP in the call stack
    InterruptionCalled = 0x0c,
    // this trap code is only used for external calls to terminate the execution,
    // but this error can't be returned from an execution cycle
    ExecutionHalted = 0xff,
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
            TrapCode::UnknownExternalFunction => write!(f, "unknown external function"),
            TrapCode::IllegalOpcode => write!(f, "illegal opcode"),
            TrapCode::InterruptionCalled => write!(f, "interruption called"),
            TrapCode::ExecutionHalted => write!(f, "execution halted"),
        }
    }
}

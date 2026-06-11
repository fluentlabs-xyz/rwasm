use crate::TrapCode;
use wasmparser::ValType;
use wasmtime::Trap;

/// Maps errors coming from Wasmtime into an rWasm `TrapCode`.
///
/// - If the error is a Wasmtime `Trap`, it is mapped into the closest `TrapCode`.
/// - If the error already contains a `TrapCode`, it is returned as-is.
/// - Otherwise the error is treated as an illegal opcode (fallback).
pub(super) fn map_wasmtime_error(err: wasmtime::Error) -> TrapCode {
    if let Some(trap) = err.downcast_ref::<Trap>() {
        #[cfg(feature = "debug-print")]
        eprintln!("wasmtime trap: {:?}", trap);

        // Map Wasmtime trap codes into rWasm trap codes.
        use wasmtime::Trap;
        match trap {
            Trap::StackOverflow => TrapCode::StackOverflow,
            Trap::MemoryOutOfBounds => TrapCode::MemoryOutOfBounds,
            Trap::HeapMisaligned => TrapCode::MemoryOutOfBounds,
            Trap::TableOutOfBounds => TrapCode::TableOutOfBounds,
            Trap::IndirectCallToNull => TrapCode::IndirectCallToNull,
            Trap::BadSignature => TrapCode::BadSignature,
            Trap::IntegerOverflow => TrapCode::IntegerOverflow,
            Trap::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            Trap::BadConversionToInteger => TrapCode::BadConversionToInteger,
            Trap::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
            Trap::Interrupt => TrapCode::InterruptionCalled,
            Trap::OutOfFuel => TrapCode::OutOfFuel,
            Trap::NullReference => TrapCode::IndirectCallToNull,
            Trap::CastFailure => TrapCode::BadConversionToInteger,
            _ => TrapCode::IllegalOpcode,
        }
    } else if let Some(trap) = err.downcast_ref::<TrapCode>() {
        // Our own trap code was propagated through Wasmtime; pass it through.
        *trap
    } else {
        #[cfg(feature = "debug-print")]
        eprintln!("wasmtime: unknown error: {:?}", err);

        // TODO(dmitry123): Decide which trap code is the best fallback for unknown Wasmtime errors.
        TrapCode::IllegalOpcode
    }
}

/// Maps an rWasm `ValType` into a Wasmtime `ValType`.
///
/// System runtimes currently support only numeric scalar types.
pub(super) fn map_val_type(val_type: ValType) -> wasmtime::ValType {
    match val_type {
        ValType::I32 => wasmtime::ValType::I32,
        ValType::I64 => wasmtime::ValType::I64,
        ValType::F32 => wasmtime::ValType::F32,
        ValType::F64 => wasmtime::ValType::F64,
        _ => unreachable!("wasmtime: unsupported type: {:?}", val_type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_wasmtime_traps_to_rwasm_traps() {
        let trap = wasmtime::Error::new(Trap::OutOfFuel);
        assert_eq!(map_wasmtime_error(trap), TrapCode::OutOfFuel);
    }

    #[test]
    fn maps_unknown_wasmtime_error_to_illegal_opcode() {
        let trap = wasmtime::Error::msg("unknown wasmtime error");
        assert_eq!(map_wasmtime_error(trap), TrapCode::IllegalOpcode);
    }
}

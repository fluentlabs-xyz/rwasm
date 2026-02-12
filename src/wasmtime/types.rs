use crate::TrapCode;
use wasmparser::ValType;
use wasmtime::Trap;

/// Maps `anyhow::Error` coming from Wasmtime into an rWasm `TrapCode`.
///
/// - If the error is a Wasmtime `Trap`, it is mapped into the closest `TrapCode`.
/// - If the error already contains a `TrapCode`, it is returned as-is.
/// - Otherwise the error is treated as an illegal opcode (fallback).
pub(super) fn map_anyhow_error(err: anyhow::Error) -> TrapCode {
    if let Some(trap) = err.downcast_ref::<Trap>() {
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
            Trap::AlwaysTrapAdapter => unreachable!("component model is not supported"),
            Trap::OutOfFuel => TrapCode::OutOfFuel,
            Trap::AtomicWaitNonSharedMemory => unreachable!("atomics are not supported"),
            Trap::NullReference => TrapCode::IndirectCallToNull,
            Trap::ArrayOutOfBounds | Trap::AllocationTooLarge => {
                unreachable!("GC is not supported")
            }
            Trap::CastFailure => TrapCode::BadConversionToInteger,
            Trap::CannotEnterComponent => unreachable!("component model is not supported"),
            Trap::NoAsyncResult => unreachable!("async mode must be disabled"),
            _ => unreachable!("unknown Wasmtime trap"),
        }
    } else if let Some(trap) = err.downcast_ref::<TrapCode>() {
        // Our own trap code was propagated through anyhow; pass it through.
        *trap
    } else {
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

# Opcode Specification

This document is the canonical opcode inventory for `rwasm`.

- Source of truth: `src/types/opcode.rs` (`enum Opcode`)
- Runtime dispatch: `src/vm/executor.rs` (`RwasmExecutor::step`)
- Encoding note: `Opcode` is `#[repr(u16)]`; discriminants follow declaration order.

## Semantics model

- Opcodes operate on a typed value stack and call stack.
- Immediate operands (when present) are carried inside the enum variant.
- Memory/table/global operations use explicit index/offset immediates.
- Fuel and trap behavior is mediated by VM/store + host syscall policy.
- FPU opcodes exist in source behind feature flags, but are intentionally excluded from this production-facing opcode spec.

## Complete opcode catalog

### stack/system

| Code (`u16`) | Opcode | Immediate | Feature gate |
| ---: | --- | --- | --- |
| 0 | `Unreachable` | — | — |
| 1 | `Trap` | `TrapCode` | — |
| 2 | `LocalGet` | `LocalDepth` | — |
| 3 | `LocalSet` | `LocalDepth` | — |
| 4 | `LocalTee` | `LocalDepth` | — |
| 5 | `Br` | `BranchOffset` | — |
| 6 | `BrIfEqz` | `BranchOffset` | — |
| 7 | `BrIfNez` | `BranchOffset` | — |
| 8 | `BrTable` | `BranchTableTargets` | — |
| 9 | `ConsumeFuel` | `BlockFuel` | — |
| 10 | `ConsumeFuelStack` | — | — |
| 11 | `Return` | — | — |
| 12 | `ReturnCallInternal` | `CompiledFunc` | — |
| 13 | `ReturnCall` | `SysFuncIdx` | — |
| 14 | `ReturnCallIndirect` | `SignatureIdx` | — |
| 15 | `CallInternal` | `CompiledFunc` | — |
| 16 | `Call` | `SysFuncIdx` | — |
| 17 | `CallIndirect` | `SignatureIdx` | — |
| 18 | `SignatureCheck` | `SignatureIdx` | — |
| 19 | `StackCheck` | `MaxStackHeight` | — |
| 20 | `RefFunc` | `CompiledFunc` | — |
| 21 | `I32Const` | `UntypedValue` | — |
| 22 | `Drop` | — | — |
| 23 | `Select` | — | — |
| 24 | `GlobalGet` | `GlobalIdx` | — |
| 25 | `GlobalSet` | `GlobalIdx` | — |

### memory

| Code (`u16`) | Opcode | Immediate | Feature gate |
| ---: | --- | --- | --- |
| 26 | `I32Load` | `AddressOffset` | — |
| 27 | `I32Load8S` | `AddressOffset` | — |
| 28 | `I32Load8U` | `AddressOffset` | — |
| 29 | `I32Load16S` | `AddressOffset` | — |
| 30 | `I32Load16U` | `AddressOffset` | — |
| 31 | `I32Store` | `AddressOffset` | — |
| 32 | `I32Store8` | `AddressOffset` | — |
| 33 | `I32Store16` | `AddressOffset` | — |
| 34 | `MemorySize` | — | — |
| 35 | `MemoryGrow` | — | — |
| 36 | `MemoryFill` | — | — |
| 37 | `MemoryCopy` | — | — |
| 38 | `MemoryInit` | `DataSegmentIdx` | — |
| 39 | `DataDrop` | `DataSegmentIdx` | — |

### table

| Code (`u16`) | Opcode | Immediate | Feature gate |
| ---: | --- | --- | --- |
| 40 | `TableSize` | `TableIdx` | — |
| 41 | `TableGrow` | `TableIdx` | — |
| 42 | `TableFill` | `TableIdx` | — |
| 43 | `TableGet` | `TableIdx` | — |
| 44 | `TableSet` | `TableIdx` | — |
| 45 | `TableCopy` | `TableIdx, TableIdx` | — |
| 46 | `TableInit` | `ElementSegmentIdx` | — |
| 47 | `ElemDrop` | `ElementSegmentIdx` | — |

### alu

| Code (`u16`) | Opcode | Immediate | Feature gate |
| ---: | --- | --- | --- |
| 48 | `I32Eqz` | — | — |
| 49 | `I32Eq` | — | — |
| 50 | `I32Ne` | — | — |
| 51 | `I32LtS` | — | — |
| 52 | `I32LtU` | — | — |
| 53 | `I32GtS` | — | — |
| 54 | `I32GtU` | — | — |
| 55 | `I32LeS` | — | — |
| 56 | `I32LeU` | — | — |
| 57 | `I32GeS` | — | — |
| 58 | `I32GeU` | — | — |
| 59 | `I32Clz` | — | — |
| 60 | `I32Ctz` | — | — |
| 61 | `I32Popcnt` | — | — |
| 62 | `I32Add` | — | — |
| 63 | `I32Sub` | — | — |
| 64 | `I32Mul` | — | — |
| 65 | `I32DivS` | — | — |
| 66 | `I32DivU` | — | — |
| 67 | `I32RemS` | — | — |
| 68 | `I32RemU` | — | — |
| 69 | `I32And` | — | — |
| 70 | `I32Or` | — | — |
| 71 | `I32Xor` | — | — |
| 72 | `I32Shl` | — | — |
| 73 | `I32ShrS` | — | — |
| 74 | `I32ShrU` | — | — |
| 75 | `I32Rotl` | — | — |
| 76 | `I32Rotr` | — | — |
| 77 | `I32WrapI64` | — | — |
| 78 | `I32Extend8S` | — | — |
| 79 | `I32Extend16S` | — | — |
| 80 | `I32Mul64` | — | — |
| 81 | `I32Add64` | — | — |
| 82 | `BulkConst` | `NumLocals` | — |
| 83 | `BulkDrop` | `NumLocals` | — |

### fpu

FPU opcodes are intentionally **not listed here** as part of the supported production opcode surface.

They are currently retained in source mainly for wasm testsuite compatibility and internal validation paths,
not as a recommended/guaranteed opcode set for production integration.

## Stability and compatibility

- Opcode order is part of binary compatibility for encoded modules.
- Any opcode addition/removal/reordering is a format change and must be treated as such in release notes.
- Feature-gated variants (`fpu`) change the available opcode surface; pin feature set in production deployments.

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
- `fpu` opcodes are available only when the `fpu` feature is enabled.

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

| Code (`u16`) | Opcode | Immediate | Feature gate |
| ---: | --- | --- | --- |
| 84 | `F32Load` | `AddressOffset` | `fpu` |
| 85 | `F64Load` | `AddressOffset` | `fpu` |
| 86 | `F32Store` | `AddressOffset` | `fpu` |
| 87 | `F64Store` | `AddressOffset` | `fpu` |
| 88 | `F32Eq` | — | `fpu` |
| 89 | `F32Ne` | — | `fpu` |
| 90 | `F32Lt` | — | `fpu` |
| 91 | `F32Gt` | — | `fpu` |
| 92 | `F32Le` | — | `fpu` |
| 93 | `F32Ge` | — | `fpu` |
| 94 | `F64Eq` | — | `fpu` |
| 95 | `F64Ne` | — | `fpu` |
| 96 | `F64Lt` | — | `fpu` |
| 97 | `F64Gt` | — | `fpu` |
| 98 | `F64Le` | — | `fpu` |
| 99 | `F64Ge` | — | `fpu` |
| 100 | `F32Abs` | — | `fpu` |
| 101 | `F32Neg` | — | `fpu` |
| 102 | `F32Ceil` | — | `fpu` |
| 103 | `F32Floor` | — | `fpu` |
| 104 | `F32Trunc` | — | `fpu` |
| 105 | `F32Nearest` | — | `fpu` |
| 106 | `F32Sqrt` | — | `fpu` |
| 107 | `F32Add` | — | `fpu` |
| 108 | `F32Sub` | — | `fpu` |
| 109 | `F32Mul` | — | `fpu` |
| 110 | `F32Div` | — | `fpu` |
| 111 | `F32Min` | — | `fpu` |
| 112 | `F32Max` | — | `fpu` |
| 113 | `F32Copysign` | — | `fpu` |
| 114 | `F64Abs` | — | `fpu` |
| 115 | `F64Neg` | — | `fpu` |
| 116 | `F64Ceil` | — | `fpu` |
| 117 | `F64Floor` | — | `fpu` |
| 118 | `F64Trunc` | — | `fpu` |
| 119 | `F64Nearest` | — | `fpu` |
| 120 | `F64Sqrt` | — | `fpu` |
| 121 | `F64Add` | — | `fpu` |
| 122 | `F64Sub` | — | `fpu` |
| 123 | `F64Mul` | — | `fpu` |
| 124 | `F64Div` | — | `fpu` |
| 125 | `F64Min` | — | `fpu` |
| 126 | `F64Max` | — | `fpu` |
| 127 | `F64Copysign` | — | `fpu` |
| 128 | `I32TruncF32S` | — | `fpu` |
| 129 | `I32TruncF32U` | — | `fpu` |
| 130 | `I32TruncF64S` | — | `fpu` |
| 131 | `I32TruncF64U` | — | `fpu` |
| 132 | `I64TruncF32S` | — | `fpu` |
| 133 | `I64TruncF32U` | — | `fpu` |
| 134 | `I64TruncF64S` | — | `fpu` |
| 135 | `I64TruncF64U` | — | `fpu` |
| 136 | `F32ConvertI32S` | — | `fpu` |
| 137 | `F32ConvertI32U` | — | `fpu` |
| 138 | `F32ConvertI64S` | — | `fpu` |
| 139 | `F32ConvertI64U` | — | `fpu` |
| 140 | `F32DemoteF64` | — | `fpu` |
| 141 | `F64ConvertI32S` | — | `fpu` |
| 142 | `F64ConvertI32U` | — | `fpu` |
| 143 | `F64ConvertI64S` | — | `fpu` |
| 144 | `F64ConvertI64U` | — | `fpu` |
| 145 | `F64PromoteF32` | — | `fpu` |
| 146 | `I32TruncSatF32S` | — | `fpu` |
| 147 | `I32TruncSatF32U` | — | `fpu` |
| 148 | `I32TruncSatF64S` | — | `fpu` |
| 149 | `I32TruncSatF64U` | — | `fpu` |
| 150 | `I64TruncSatF32S` | — | `fpu` |
| 151 | `I64TruncSatF32U` | — | `fpu` |
| 152 | `I64TruncSatF64S` | — | `fpu` |
| 153 | `I64TruncSatF64U` | — | `fpu` |

## Stability and compatibility

- Opcode order is part of binary compatibility for encoded modules.
- Any opcode addition/removal/reordering is a format change and must be treated as such in release notes.
- Feature-gated variants (`fpu`) change the available opcode surface; pin feature set in production deployments.

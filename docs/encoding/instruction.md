# rWasm Instruction Encoding Specification

## Overview

This document describes the binary encoding format of rWasm instructions. Each instruction consists of an opcode,
followed by any required operands.
Each instruction has alignment and is padded to nine bytes, where 1 first byte is instruction code and eight bytes are
operand.

## Instruction Encoding Table

Each instruction in **rWasm** has a **16-bit opcode**, followed by **optional operands**.

### Core Instructions (Always Available)

| **Instruction**      | **Opcode** | **Operands**               |
|----------------------|------------|----------------------------|
| `Unreachable`        | `0x00`     | None                       |
| `Trap`               | `0x01`     | `TrapCode` (U16)           |
| `LocalGet`           | `0x10`     | `LocalDepth` (U32)         |
| `LocalSet`           | `0x11`     | `LocalDepth` (U32)         |
| `LocalTee`           | `0x12`     | `LocalDepth` (U32)         |
| `Br`                 | `0x20`     | `BranchOffset` (I32)       |
| `BrIfEqz`            | `0x21`     | `BranchOffset` (I32)       |
| `BrIfNez`            | `0x22`     | `BranchOffset` (I32)       |
| `BrTable`            | `0x23`     | `BranchTableTargets` (U32) |
| `ConsumeFuel`        | `0x30`     | `BlockFuel` (U32)          |
| `ConsumeFuelStack`   | `0x31`     | None                       |
| `Return`             | `0x40`     | None                       |
| `ReturnCallInternal` | `0x41`     | `CompiledFunc` (U32)       |
| `ReturnCall`         | `0x42`     | `SysFuncIdx` (U32)         |
| `ReturnCallIndirect` | `0x43`     | `SignatureIdx` (U32)       |
| `CallInternal`       | `0x44`     | `CompiledFunc` (U32)       |
| `Call`               | `0x45`     | `SysFuncIdx` (U32)         |
| `CallIndirect`       | `0x46`     | `SignatureIdx` (U32)       |
| `SignatureCheck`     | `0x50`     | `SignatureIdx` (U32)       |
| `StackCheck`         | `0x51`     | `MaxStackHeight` (U32)     |
| `RefFunc`            | `0x60`     | `CompiledFunc` (U32)       |
| `I32Const`           | `0x61`     | `UntypedValue` (U64)       |
| `Drop`               | `0x62`     | None                       |
| `Select`             | `0x63`     | None                       |
| `GlobalGet`          | `0x70`     | `GlobalIdx` (U32)          |
| `GlobalSet`          | `0x71`     | `GlobalIdx` (U32)          |

### Memory Instructions

| **Instruction** | **Opcode** | **Operands**           |
|-----------------|------------|------------------------|
| `I32Load`       | `0x80`     | `AddressOffset` (U32)  |
| `I32Load8S`     | `0x81`     | `AddressOffset` (U32)  |
| `I32Load8U`     | `0x82`     | `AddressOffset` (U32)  |
| `I32Load16S`    | `0x83`     | `AddressOffset` (U32)  |
| `I32Load16U`    | `0x84`     | `AddressOffset` (U32)  |
| `I32Store`      | `0x85`     | `AddressOffset` (U32)  |
| `I32Store8`     | `0x86`     | `AddressOffset` (U32)  |
| `I32Store16`    | `0x87`     | `AddressOffset` (U32)  |
| `MemorySize`    | `0x88`     | None                   |
| `MemoryGrow`    | `0x89`     | None                   |
| `MemoryFill`    | `0x8a`     | None                   |
| `MemoryCopy`    | `0x8b`     | None                   |
| `MemoryInit`    | `0x8c`     | `DataSegmentIdx` (U32) |
| `DataDrop`      | `0x8d`     | `DataSegmentIdx` (U32) |

### Table Instructions

| **Instruction** | **Opcode** | **Operands**                       |
|-----------------|------------|------------------------------------|
| `TableSize`     | `0x90`     | `TableIdx` (U16)                   |
| `TableGrow`     | `0x91`     | `TableIdx` (U16)                   |
| `TableFill`     | `0x92`     | `TableIdx` (U16)                   |
| `TableGet`      | `0x93`     | `TableIdx` (U16)                   |
| `TableSet`      | `0x94`     | `TableIdx` (U16)                   |
| `TableCopy`     | `0x95`     | `TableIdx` (U16), `TableIdx` (U16) |
| `TableInit`     | `0x96`     | `ElementSegmentIdx` (U32)          |
| `ElemDrop`      | `0x97`     | `ElementSegmentIdx` (U32)          |

### I32 Arithmetic and Logic Instructions

| **Instruction** | **Opcode** | **Operands** |
|-----------------|------------|--------------|
| `I32Eqz`        | `0xa0`     | None         |
| `I32Eq`         | `0xa1`     | None         |
| `I32Ne`         | `0xa2`     | None         |
| `I32LtS`        | `0xa3`     | None         |
| `I32LtU`        | `0xa4`     | None         |
| `I32GtS`        | `0xa5`     | None         |
| `I32GtU`        | `0xa6`     | None         |
| `I32LeS`        | `0xa7`     | None         |
| `I32LeU`        | `0xa8`     | None         |
| `I32GeS`        | `0xa9`     | None         |
| `I32GeU`        | `0xaa`     | None         |
| `I32Clz`        | `0xab`     | None         |
| `I32Ctz`        | `0xac`     | None         |
| `I32Popcnt`     | `0xad`     | None         |
| `I32Add`        | `0xae`     | None         |
| `I32Sub`        | `0xaf`     | None         |
| `I32Mul`        | `0xb0`     | None         |
| `I32DivS`       | `0xb1`     | None         |
| `I32DivU`       | `0xb2`     | None         |
| `I32RemS`       | `0xb3`     | None         |
| `I32RemU`       | `0xb4`     | None         |
| `I32And`        | `0xb5`     | None         |
| `I32Or`         | `0xb6`     | None         |
| `I32Xor`        | `0xb7`     | None         |
| `I32Shl`        | `0xb8`     | None         |
| `I32ShrS`       | `0xb9`     | None         |
| `I32ShrU`       | `0xba`     | None         |
| `I32Rotl`       | `0xbb`     | None         |
| `I32Rotr`       | `0xbc`     | None         |
| `I32WrapI64`    | `0xbd`     | None         |
| `I32Extend8S`   | `0xbe`     | None         |
| `I32Extend16S`  | `0xbf`     | None         |

### 64-bit Optimized Instructions

| **Instruction** | **Opcode** | **Operands** |
|-----------------|------------|--------------|
| `I32Mul64`      | `0xc0`     | None         |
| `I32Add64`      | `0xc1`     | None         |

### Floating Point Instructions (FPU Feature Only)

| **Instruction**   | **Opcode** | **Operands**          |
|-------------------|------------|-----------------------|
| `F32Load`         | `0xff00`   | `AddressOffset` (U32) |
| `F64Load`         | `0xff01`   | `AddressOffset` (U32) |
| `F32Store`        | `0xff02`   | `AddressOffset` (U32) |
| `F64Store`        | `0xff03`   | `AddressOffset` (U32) |
| `F32Eq`           | `0xff04`   | None                  |
| `F32Ne`           | `0xff05`   | None                  |
| `F32Lt`           | `0xff06`   | None                  |
| `F32Gt`           | `0xff07`   | None                  |
| `F32Le`           | `0xff08`   | None                  |
| `F32Ge`           | `0xff09`   | None                  |
| `F64Eq`           | `0xff0a`   | None                  |
| `F64Ne`           | `0xff0b`   | None                  |
| `F64Lt`           | `0xff0c`   | None                  |
| `F64Gt`           | `0xff0d`   | None                  |
| `F64Le`           | `0xff0e`   | None                  |
| `F64Ge`           | `0xff0f`   | None                  |
| `F32Abs`          | `0xff10`   | None                  |
| `F32Neg`          | `0xff11`   | None                  |
| `F32Ceil`         | `0xff12`   | None                  |
| `F32Floor`        | `0xff13`   | None                  |
| `F32Trunc`        | `0xff14`   | None                  |
| `F32Nearest`      | `0xff15`   | None                  |
| `F32Sqrt`         | `0xff16`   | None                  |
| `F32Add`          | `0xff17`   | None                  |
| `F32Sub`          | `0xff18`   | None                  |
| `F32Mul`          | `0xff19`   | None                  |
| `F32Div`          | `0xff1a`   | None                  |
| `F32Min`          | `0xff1b`   | None                  |
| `F32Max`          | `0xff1c`   | None                  |
| `F32Copysign`     | `0xff1d`   | None                  |
| `F64Abs`          | `0xff1e`   | None                  |
| `F64Neg`          | `0xff1f`   | None                  |
| `F64Ceil`         | `0xff20`   | None                  |
| `F64Floor`        | `0xff21`   | None                  |
| `F64Trunc`        | `0xff22`   | None                  |
| `F64Nearest`      | `0xff23`   | None                  |
| `F64Sqrt`         | `0xff24`   | None                  |
| `F64Add`          | `0xff25`   | None                  |
| `F64Sub`          | `0xff26`   | None                  |
| `F64Mul`          | `0xff27`   | None                  |
| `F64Div`          | `0xff28`   | None                  |
| `F64Min`          | `0xff29`   | None                  |
| `F64Max`          | `0xff2a`   | None                  |
| `F64Copysign`     | `0xff2b`   | None                  |
| `I32TruncF32S`    | `0xff2c`   | None                  |
| `I32TruncF32U`    | `0xff2d`   | None                  |
| `I32TruncF64S`    | `0xff2e`   | None                  |
| `I32TruncF64U`    | `0xff2f`   | None                  |
| `I64TruncF32S`    | `0xff30`   | None                  |
| `I64TruncF32U`    | `0xff31`   | None                  |
| `I64TruncF64S`    | `0xff32`   | None                  |
| `I64TruncF64U`    | `0xff33`   | None                  |
| `F32ConvertI32S`  | `0xff34`   | None                  |
| `F32ConvertI32U`  | `0xff35`   | None                  |
| `F32ConvertI64S`  | `0xff36`   | None                  |
| `F32ConvertI64U`  | `0xff37`   | None                  |
| `F32DemoteF64`    | `0xff38`   | None                  |
| `F64ConvertI32S`  | `0xff39`   | None                  |
| `F64ConvertI32U`  | `0xff3a`   | None                  |
| `F64ConvertI64S`  | `0xff3b`   | None                  |
| `F64ConvertI64U`  | `0xff3c`   | None                  |
| `F64PromoteF32`   | `0xff3d`   | None                  |
| `I32TruncSatF32S` | `0xff3e`   | None                  |
| `I32TruncSatF32U` | `0xff3f`   | None                  |
| `I32TruncSatF64S` | `0xff40`   | None                  |
| `I32TruncSatF64U` | `0xff41`   | None                  |
| `I64TruncSatF32S` | `0xff42`   | None                  |
| `I64TruncSatF32U` | `0xff43`   | None                  |
| `I64TruncSatF64S` | `0xff44`   | None                  |
| `I64TruncSatF64U` | `0xff45`   | None                  |

### I64 Instruction Emulation

**Note**: rWasm does not have dedicated I64 opcodes. Instead, I64 operations are emulated using I32 operations where
each I64 value occupies 2 stack slots. The compiler automatically handles this conversion during translation from
WebAssembly. The only I64-related opcodes are the conversion instructions listed above under the FPU feature.

## **Instruction Format**

Each instruction consists of:

1. **Opcode** (16-bit value, stored as u16)
2. **Operands** (0–8 bytes, depending on instruction)

The total instruction size is **8 bytes** (opcode + operands), stored using bincode serialization.

## **Integer and Floating-Point Encoding**

- **Unsigned Integers** (`U32`, `U64`) → Little Endian
- **Signed Integers** (`I32`, `I64`) → Little Endian (Two's Complement)
- **Floating Point (`F32`, `F64`)** → IEEE-754 Representation
- **UntypedValue** → 64-bit value that can represent i32, i64, f32, or f64

## **Encoding Details**

- Instructions are encoded using bincode with legacy configuration
- The opcode is a 16-bit discriminant value
- Operands are embedded within the enum variant structure
- FPU instructions use extended opcodes (0xff00-0xff45) when the FPU feature is enabled

## **Example Encoding**

### **Example: Encoding `i32.const 100`**

The instruction `I32Const(UntypedValue::from(100))` is encoded as:

```rust
// Opcode: 0x61 (I32Const)
// Operand: UntypedValue containing 100
let instruction = Opcode::I32Const(UntypedValue::from(100));
let encoded = bincode::encode_to_vec( & instruction, bincode::config::legacy()).unwrap();
```

The resulting binary representation contains:

- **Opcode**: `0x61` (I32Const discriminant)
- **Operand**: 64-bit UntypedValue containing the value 100

### **Example: Encoding `br 5`**

The instruction `Br(BranchOffset::from(5))` is encoded as:

```rust
// Opcode: 0x20 (Br)
// Operand: BranchOffset containing +5
let instruction = Opcode::Br(BranchOffset::from(5));
let encoded = bincode::encode_to_vec( & instruction, bincode::config::legacy()).unwrap();
```

## **Decoding rWasm Instructions**

Instructions are decoded using bincode deserialization:

```rust
let instruction: Opcode = bincode::decode_from_slice( & encoded_bytes, bincode::config::legacy()).unwrap();
match instruction {
Opcode::I32Const(value) => {
// Handle I32Const with UntypedValue
let i32_val = value.as_i32();
}
Opcode::Br(offset) => {
// Handle branch with relative offset
let branch_target = current_pc + offset.to_i32();
}
// ... other instructions
}
```

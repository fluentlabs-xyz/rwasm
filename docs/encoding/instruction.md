# RWasm Instruction Encoding Specification

## Overview

This document describes the binary encoding format of rWasm instructions. Each instruction consists of an opcode,
followed by any required operands.
Each instruction has alignment and is padded to nine bytes, where 1 first byte is instruction code and eight bytes are
operand.

## Instruction Encoding Table

Each instruction in **rWasm** has a **1-byte opcode**, followed by **optional operands**.

| **Instruction**      | **Opcode** | **Operands**               |
|----------------------|------------|----------------------------|
| `Unreachable`        | `0x00`     | None                       |
| `LocalGet`           | `0x01`     | `LocalDepth` (U32)         |
| `LocalSet`           | `0x02`     | `LocalDepth` (U32)         |
| `LocalTee`           | `0x03`     | `LocalDepth` (U32)         |
| `Br`                 | `0x04`     | `BranchOffset` (I32)       |
| `BrIfEqz`            | `0x05`     | `BranchOffset` (I32)       |
| `BrIfNez`            | `0x06`     | `BranchOffset` (I32)       |
| `BrAdjust`           | `0x07`     | `BranchOffset` (I32)       |
| `BrAdjustIfNez`      | `0x08`     | `BranchOffset` (I32)       |
| `BrTable`            | `0x09`     | `BranchTableTargets` (U32) |
| `ConsumeFuel`        | `0x0A`     | `BlockFuel` (U32)          |
| `Return`             | `0x0B`     | `DropKeep`                 |
| `ReturnIfNez`        | `0x0C`     | `DropKeep`                 |
| `ReturnCallInternal` | `0x0D`     | `CompiledFunc` (U32)       |
| `ReturnCall`         | `0x0E`     | `FuncIdx` (U32)            |
| `ReturnCallIndirect` | `0x0F`     | `SignatureIdx` (U32)       |
| `CallInternal`       | `0x10`     | `CompiledFunc` (U32)       |
| `Call`               | `0x11`     | `FuncIdx` (U32)            |
| `CallIndirect`       | `0x12`     | `SignatureIdx` (U32)       |
| `SignatureCheck`     | `0x13`     | `SignatureIdx` (U32)       |
| `Drop`               | `0x14`     | None                       |
| `Select`             | `0x15`     | None                       |
| `GlobalGet`          | `0x16`     | `GlobalIdx` (U32)          |
| `GlobalSet`          | `0x17`     | `GlobalIdx` (U32)          |
| `I32Load`            | `0x18`     | `AddressOffset` (U32)      |
| `I64Load`            | `0x19`     | `AddressOffset` (U32)      |
| `F32Load`            | `0x1A`     | `AddressOffset` (U32)      |
| `F64Load`            | `0x1B`     | `AddressOffset` (U32)      |
| `MemorySize`         | `0x2F`     | None                       |
| `MemoryGrow`         | `0x30`     | None                       |
| `MemoryFill`         | `0x31`     | None                       |
| `MemoryCopy`         | `0x32`     | None                       |
| `MemoryInit`         | `0x33`     | `DataSegmentIdx` (U32)     |
| `DataDrop`           | `0x34`     | `DataSegmentIdx` (U32)     |
| `TableSize`          | `0x35`     | `TableIdx` (U32)           |
| `TableGrow`          | `0x36`     | `TableIdx` (U32)           |
| `TableFill`          | `0x37`     | `TableIdx` (U32)           |
| `TableGet`           | `0x38`     | `TableIdx` (U32)           |
| `TableSet`           | `0x39`     | `TableIdx` (U32)           |
| `TableCopy`          | `0x3A`     | `TableIdx` (U32)           |
| `TableInit`          | `0x3B`     | `ElementSegmentIdx` (U32)  |
| `ElemDrop`           | `0x3C`     | `ElementSegmentIdx` (U32)  |
| `RefFunc`            | `0x3D`     | `FuncIdx` (U32)            |
| `I32Const`           | `0x3E`     | `i32` (U32)                |
| `I64Const`           | `0x3F`     | `i64` (U64)                |
| `F32Const`           | `0x40`     | `f32` (U32)                |
| `F64Const`           | `0x41`     | `f64` (U64)                |
| `I32Eqz`             | `0x42`     | None                       |
| `I32Eq`              | `0x43`     | None                       |
| `I32Ne`              | `0x44`     | None                       |
| `I32LtS`             | `0x45`     | None                       |
| `I32LtU`             | `0x46`     | None                       |
| `I32GtS`             | `0x47`     | None                       |
| `I32GtU`             | `0x48`     | None                       |
| `I32LeS`             | `0x49`     | None                       |
| `I32LeU`             | `0x4a`     | None                       |
| `I32GeS`             | `0x4b`     | None                       |
| `I32GeU`             | `0x4c`     | None                       |
| `I64Eqz`             | `0x4d`     | None                       |
| `I64Eq`              | `0x4e`     | None                       |
| `I64Ne`              | `0x4f`     | None                       |
| `I64LtS`             | `0x50`     | None                       |
| `I64LtU`             | `0x51`     | None                       |
| `I64GtS`             | `0x52`     | None                       |
| `I64GtU`             | `0x53`     | None                       |
| `I64LeS`             | `0x54`     | None                       |
| `I64LeU`             | `0x55`     | None                       |
| `I64GeS`             | `0x56`     | None                       |
| `I64GeU`             | `0x57`     | None                       |
| `F32Eq`              | `0x58`     | None                       |
| `F32Ne`              | `0x59`     | None                       |
| `F32Lt`              | `0x5a`     | None                       |
| `F32Gt`              | `0x5b`     | None                       |
| `F32Le`              | `0x5c`     | None                       |
| `F32Ge`              | `0x5d`     | None                       |
| `F64Eq`              | `0x5e`     | None                       |
| `F64Ne`              | `0x5f`     | None                       |
| `F64Lt`              | `0x60`     | None                       |
| `F64Gt`              | `0x61`     | None                       |
| `F64Le`              | `0x62`     | None                       |
| `F64Ge`              | `0x63`     | None                       |
| `I32Clz`             | `0x64`     | None                       |
| `I32Ctz`             | `0x65`     | None                       |
| `I32Popcnt`          | `0x66`     | None                       |
| `I32Add`             | `0x67`     | None                       |
| `I32Sub`             | `0x68`     | None                       |
| `I32Mul`             | `0x69`     | None                       |
| `I32DivS`            | `0x6a`     | None                       |
| `I32DivU`            | `0x6b`     | None                       |
| `I32RemS`            | `0x6c`     | None                       |
| `I32RemU`            | `0x6d`     | None                       |
| `I32And`             | `0x6e`     | None                       |
| `I32Or`              | `0x6f`     | None                       |
| `I32Xor`             | `0x70`     | None                       |
| `I32Shl`             | `0x71`     | None                       |
| `I32ShrS`            | `0x72`     | None                       |
| `I32ShrU`            | `0x73`     | None                       |
| `I32Rotl`            | `0x74`     | None                       |
| `I32Rotr`            | `0x75`     | None                       |
| `I64Clz`             | `0x76`     | None                       |
| `I64Ctz`             | `0x77`     | None                       |
| `I64Popcnt`          | `0x78`     | None                       |
| `I64Add`             | `0x79`     | None                       |
| `I64Sub`             | `0x7a`     | None                       |
| `I64Mul`             | `0x7b`     | None                       |
| `I64DivS`            | `0x7c`     | None                       |
| `I64DivU`            | `0x7d`     | None                       |
| `I64RemS`            | `0x7e`     | None                       |
| `I64RemU`            | `0x7f`     | None                       |
| `I64And`             | `0x80`     | None                       |
| `I64Or`              | `0x81`     | None                       |
| `I64Xor`             | `0x82`     | None                       |
| `I64Shl`             | `0x83`     | None                       |
| `I64ShrS`            | `0x84`     | None                       |
| `I64ShrU`            | `0x85`     | None                       |
| `I64Rotl`            | `0x86`     | None                       |
| `I64Rotr`            | `0x87`     | None                       |
| `F32Abs`             | `0x88`     | None                       |
| `F32Neg`             | `0x89`     | None                       |
| `F32Ceil`            | `0x8a`     | None                       |
| `F32Floor`           | `0x8b`     | None                       |
| `F32Trunc`           | `0x8c`     | None                       |
| `F32Nearest`         | `0x8d`     | None                       |
| `F32Sqrt`            | `0x8e`     | None                       |
| `F32Add`             | `0x8f`     | None                       |
| `F32Sub`             | `0x90`     | None                       |
| `F32Mul`             | `0x91`     | None                       |
| `F32Div`             | `0x92`     | None                       |
| `F32Min`             | `0x93`     | None                       |
| `F32Max`             | `0x94`     | None                       |
| `F32Copysign`        | `0x95`     | None                       |
| `F64Abs`             | `0x96`     | None                       |
| `F64Neg`             | `0x97`     | None                       |
| `F64Ceil`            | `0x98`     | None                       |
| `F64Floor`           | `0x99`     | None                       |
| `F64Trunc`           | `0x9a`     | None                       |
| `F64Nearest`         | `0x9b`     | None                       |
| `F64Sqrt`            | `0x9c`     | None                       |
| `F64Add`             | `0x9d`     | None                       |
| `F64Sub`             | `0x9e`     | None                       |
| `F64Mul`             | `0x9f`     | None                       |
| `F64Div`             | `0xa0`     | None                       |
| `F64Min`             | `0xa1`     | None                       |
| `F64Max`             | `0xa2`     | None                       |
| `F64Copysign`        | `0xa3`     | None                       |
| `I32WrapI64`         | `0xa4`     | None                       |
| `I32TruncF32S`       | `0xa5`     | None                       |
| `I32TruncF32U`       | `0xa6`     | None                       |
| `I32TruncF64S`       | `0xa7`     | None                       |
| `I32TruncF64U`       | `0xa8`     | None                       |
| `I64ExtendI32S`      | `0xa9`     | None                       |
| `I64ExtendI32U`      | `0xaa`     | None                       |
| `I64TruncF32S`       | `0xab`     | None                       |
| `I64TruncF32U`       | `0xac`     | None                       |
| `I64TruncF64S`       | `0xad`     | None                       |
| `I64TruncF64U`       | `0xae`     | None                       |
| `F32ConvertI32S`     | `0xaf`     | None                       |
| `F32ConvertI32U`     | `0xb0`     | None                       |
| `F32ConvertI64S`     | `0xb1`     | None                       |
| `F32ConvertI64U`     | `0xb2`     | None                       |
| `F32DemoteF64`       | `0xb3`     | None                       |
| `F64ConvertI32S`     | `0xb4`     | None                       |
| `F64ConvertI32U`     | `0xb5`     | None                       |
| `F64ConvertI64S`     | `0xb6`     | None                       |
| `F64ConvertI64U`     | `0xb7`     | None                       |
| `F64PromoteF32`      | `0xb8`     | None                       |
| `I32Extend8S`        | `0xb9`     | None                       |
| `I32Extend16S`       | `0xba`     | None                       |
| `I64Extend8S`        | `0xbb`     | None                       |
| `I64Extend16S`       | `0xbc`     | None                       |
| `I64Extend32S`       | `0xbd`     | None                       |
| `I32TruncSatF32S`    | `0xbe`     | None                       |
| `I32TruncSatF32U`    | `0xbf`     | None                       |
| `I32TruncSatF64S`    | `0xc0`     | None                       |
| `I32TruncSatF64U`    | `0xc1`     | None                       |
| `I64TruncSatF32S`    | `0xc2`     | None                       |
| `I64TruncSatF32U`    | `0xc3`     | None                       |
| `I64TruncSatF64S`    | `0xc4`     | None                       |
| `I64TruncSatF64U`    | `0xc5`     | None                       |

## **Instruction Format**

Each instruction consists of:

1. **Opcode** (1 byte)
2. **Operands** (0–8 bytes, depending on instruction)

## **Integer and Floating-Point Encoding**

- **Unsigned Integers** (`U32`, `U64`) → Little Endian
- **Signed Integers** (`I32`, `I64`) → Little Endian (Two’s Complement)
- **Floating Point (`F32`, `F64`)** → IEEE-754 Representation

## **Alignment & Padding**

- Maximum instruction size is **9 bytes** (1-byte opcode + 8-byte operand).
- Instructions **are padded** to align to 9 bytes when necessary.

## **Example Encoding**

### **Example: Encoding `i32.const 100`**

| Byte Offset | Value                 | Description                        |
|-------------|-----------------------|------------------------------------|
| `0x00`      | `0x3E`                | Opcode for `I32Const`              |
| `0x01-0x04` | `0x64 0x00 0x00 0x00` | Little-Endian `100`                |
| `0x05-0x08` | `0x00 0x00 0x00 0x00` | Padding (ensures 9-byte alignment) |

## **Decoding rWasm Instructions**

### **Example: Decoding `i32.const`**

1. Read the **opcode** (`0x3E`).
2. Read the **next 4 bytes** as a **little-endian** `i32` (`0x64 0x00 0x00 0x00` → `100`).
3. Skip any **alignment padding** (`0x00 0x00 0x00 0x00`).

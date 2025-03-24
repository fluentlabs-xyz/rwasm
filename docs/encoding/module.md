## **1. Module Structure**

An rWasm module is a **binary format** that consists of a **header**, **sections**, and an **end marker**.
rWasm binary format follows the [EIP-3540 standard](https://eips.ethereum.org/EIPS/eip-3540).
It uses EIP-3540 prefix `0xEF` where the second byte is `0x52` (in ASCII stands for `R` - *reduced*)

### **Module Layout**

### **Magic Bytes and Version**

| **Field**                   | **Size (bytes)** | **Value**   | **Description**                           |
|-----------------------------|------------------|-------------|-------------------------------------------|
| **Magic Bytes**             | 2                | `0xEF 0x52` | Identifies an rWasm binary.               |
| **Version**                 | 1                | `0x01`      | Version of rWasm format.                  |
| **Code Section**            | 1                | `0x01`      | Indicator of code section.                |
| **Code Section Length**     | 4                | `U32 LE`    | Length of code section in 4 bytes LE.     |
| **Memory Section**          | 1                | `0x02`      | Indicator of memory section.              |
| **Memory Section Length**   | 4                | `U32 LE`    | Length of memory section in 4 bytes LE.   |
| **Function Section**        | 1                | `0x02`      | Indicator of function section.            |
| **Function Section Length** | 4                | `U32 LE`    | Length of function section in 4 bytes LE. |
| **Element Section**         | 1                | `0x02`      | Indicator of element section.             |
| **Element Section Length**  | 4                | `U32 LE`    | Length of element section in 4 bytes LE.  |
| **End Flag**                | 1                | `0x00`      | Indicates end of the header.              |
| **Code Section Body**       | Variable         |             | Body of the code section.                 |
| **Memory Section Body**     | Variable         |             | Body of the memory section.               |
| **Function Section Body**   | Variable         |             | Body of the function section.             |
| **Element Section Body**    | Variable         |             | Body of the element section.              |

---

## **3. Encoding of Each Section**

### **3.1 Code Section**

The **code section** contains the compiled bytecode for functions. Each function consists of instructions encoded in a
binary format.

#### **Example: Code Section Encoding**

```hex
3E 64 00 00 00 00 00 00 00
3E 14 00 00 00 00 00 00 00
67 00 00 00 00 00 00 00 00
```

#### **Breakdown**

- `3E` → `i32.const`
- `64 00 00 00` → U32 encoding of `100`
- `00 00 00 00` → instruction padding
- `3E` → `i32.const`
- `14 00 00 00` → U32 encoding of `20`
- `00 00 00 00` → instruction padding
- `67` → `i32.add`
- `00 00 00 00` → instruction padding
- `00 00 00 00` → instruction padding

---

### **3.2 Memory Section**

The **memory section** defines the **linear memory** for the module,
it contains concatenation of all data segments from WebAssembly binary.

#### **Example: Memory Section Encoding**

```hex
48 65 6C 6C 6F 2C 20 57 6F 72 6C 64 // Hello, World
49 74 27 73 20 70 61 6E 69 63 20 74 69 6D 65 // It's panic time
```

#### **Breakdown**

- `48 65 6C 6C 6F 2C 20 57 6F 72 6C 64` → Data of the first segment
- `49 74 27 73 20 70 61 6E 69 63 20 74 69 6D 65` → Data of the second segment

---

### **3.3 Function Section**

The **function section** stores lengths of each function.

#### **Example: Function Section Encoding**

```hex
07 00 00 00 // function 0 has 7 instructions
03 00 00 00 // function 1 has 3 instructions
```

#### **Breakdown**

- `07 00 00 00` → Function 0 has seven instructions
- `03 00 00 00` → Function 1 has three instructions

---

### **3.4 Element Section**

The **element section** defines **function table elements**.
It contains function indices that are used to initialize tables.
Instruction `table_init` use this section to load tables.

#### **Example: Element Section Encoding**

```hex
01 00 00 00
02 00 00 00
03 00 00 00
```

#### **Breakdown**

- `01 00 00 00` → The first function is 1
- `02 00 00 00` → The second function is 2
- `03 00 00 00` → The third function is 3

---

## **5. Example: Complete rWasm Module Encoding**

The following example encodes an **rWasm module**

```webassembly
(module $fluentbase_example_greeting.wasm
  (type (;0;) (func (param i32 i32)))
  (type (;1;) (func))
  (import "fluentbase_v1preview" "_write" (func $_ZN14fluentbase_sdk8bindings6_write17hf99e1d2b50d9dcb9E (type 0)))
  (func $deploy (type 1))
  (func $main (type 1)
    i32.const 262144
    i32.const 12
    call $_ZN14fluentbase_sdk8bindings6_write17hf99e1d2b50d9dcb9E)
  (memory (;0;) 5)
  (global $__stack_pointer (mut i32) (i32.const 262144))
  (global (;1;) i32 (i32.const 262156))
  (global (;2;) i32 (i32.const 262160))
  (export "memory" (memory 0))
  (export "deploy" (func $deploy))
  (export "main" (func $main))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (data $.rodata (i32.const 262144) "Hello, World"))
```

The module header:

| **Field**              | **Size (bytes)** | **Value**             |
|------------------------|------------------|-----------------------|
| **Magic Bytes**        | 2                | `0xEF 0x52`           |
| **Version**            | 1                | `0x01`                |
| **Code Signature**     | 1                | `0x01`                |
| **Code Length**        | 4                | `0xdd 0x01 0x00 0x00` |
| **Memory Signature**   | 1                | `0x02`                |
| **Memory Length**      | 4                | `0x0c 0x00 0x00 0x00` |
| **Function Signature** | 1                | `0x03`                |
| **Function Length**    | 4                | `0x14 0x00 0x00 0x00` |
| **Element Signature**  | 1                | `0x04`                |
| **Element Length**     | 4                | `0x00 0x00 0x00 0x00` |
| **Header End**         | 1                | `0x00`                |

The code section:

| **Field**                | **Size (bytes)** | **Value**                                      | **Description** |
|--------------------------|------------------|------------------------------------------------|-----------------|
| `ConsumeFuel(1)`         | 1                | `0x0a 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` | Function 0      |
| `ConsumeFuel(0)`         | 1                | `0x0a 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Call(1)`                | 1                | `0x11 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Return(drop=0, keep=1)` | 1                | `0x0b 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `ConsumeFuel(1)`         | 1                | `0x0a 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` | Function 1      |
| `ConsumeFuel(0)`         | 1                | `0x0a 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Call(5)`                | 1                | `0x11 0x05 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Return(drop=0, keep=1)` | 1                | `0x0b 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `ConsumeFuel(4)`         | 1                | `0x0a 0x04 0x00 0x00 0x00 0x00 0x00 0x00 0x00` | Function 2      |
| `SignatureCheck(2)`      | 1                | `0x13 0x02 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(0)`            | 1                | `0x3e 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Callinternal(0)`        | 1                | `0x10 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `BrTable(0)`             | 1                | `0x09 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `ConsumeFuel(7)`         | 1                | `0x0a 0x07 0x00 0x00 0x00 0x00 0x00 0x00 0x00` | Function 3      |
| `SignatureCheck(2)`      | 1                | `0x13 0x02 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(262144)`       | 1                | `0x3e 0x00 0x00 0x04 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(12)`           | 1                | `0x3e 0x0c 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `CallInternal(1)`        | 1                | `0x10 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(0)`            | 1                | `0x3e 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `CallInternal(0)`        | 1                | `0x10 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `BrTable(0)`             | 1                | `0x09 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `ConsumeFuel(1)`         | 1                | `0x0a 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` | Entrypoint      |
| `I64Const(262144)`       | 1                | `0x3f 0x00 0x00 0x04 0x00 0x00 0x00 0x00 0x00` |                 |
| `GlobalSet(0)`           | 1                | `0x17 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I64Const(262156)`       | 1                | `0x3f 0x0c 0x00 0x04 0x00 0x00 0x00 0x00 0x00` |                 |
| `GlobalSet(1)`           | 1                | `0x17 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I64Const(262160)`       | 1                | `0x3f 0x10 0x00 0x04 0x00 0x00 0x00 0x00 0x00` |                 |
| `GlobalSet(2)`           | 1                | `0x17 0x02 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(5)`            | 1                | `0x3e 0x05 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `MemoryGrow`             | 1                | `0x30 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Drop`                   | 1                | `0x14 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(262144)`       | 1                | `0x3e 0x00 0x00 0x04 0x00 0x00 0x00 0x00 0x00` |                 |
| `I64Const(0)`            | 1                | `0x3f 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I64Const(12)`           | 1                | `0x3f 0x0c 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `MemoryInit(0)`          | 1                | `0x33 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `DataDrop(1)`            | 1                | `0x34 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Call(FuncIdx(2))`       | 1                | `0x11 0x02 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Unreachable`            | 1                | `0x00 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(1)`            | 1                | `0x3e 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Eq`                  | 1                | `0x43 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Br(4)`                  | 1                | `0x04 0x04 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Drop`                   | 1                | `0x14 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `CallInternal(2)`        | 1                | `0x10 0x02 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Return(drop=0, keep=0)` | 1                | `0x0b 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Unreachable`            | 1                | `0x00 0x01 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Const(0)`            | 1                | `0x3e 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `I32Eq`                  | 1                | `0x43 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Br(4)`                  | 1                | `0x04 0x04 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Drop`                   | 1                | `0x14 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `CallInternal(3)`        | 1                | `0x10 0x03 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Return(drop=0, keep=0)` | 1                | `0x0b 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Drop`                   | 1                | `0x14 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |
| `Return(drop=0, keep=0)` | 1                | `0x0b 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00` |                 |

The memory section:

| **Field**      | **Size (bytes)** | **Value**                                                     |
|----------------|------------------|---------------------------------------------------------------|
| "Hello, World" | 12               | `0x48 0x65 0x6c 0x6c 0x6f 0x2c 0x20 0x57 0x6f 0x72 0x6c 0x64` |

The function section:

| **Field**       | **Size (bytes)** | **Value**             |
|-----------------|------------------|-----------------------|
| Function Length | 4                | `0x04 0x00 0x00 0x00` |
| Function Length | 4                | `0x04 0x00 0x00 0x00` |
| Function Length | 4                | `0x05 0x00 0x00 0x00` |
| Function Length | 4                | `0x08 0x00 0x00 0x00` |
| Function Length | 4                | `0x20 0x00 0x00 0x00` |
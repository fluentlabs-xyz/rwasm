## **1. Module Structure**

An rWasm module is a **binary format** that consists of a **header** followed by **sections** encoded using bincode.
The format uses magic bytes `0xEF 0x52` where `0x52` stands for `R` (reduced WebAssembly).

### **Module Layout**

The rWasm module consists of the following components in order:

1. **Magic Bytes** (2 bytes)
2. **Version** (1 byte)
3. **Code Section** (variable length)
4. **Data Section** (variable length)
5. **Element Section** (variable length)
6. **WASM Section** (variable length)

### **Header Format**

| **Field**                   | **Size (bytes)** | **Value**   | **Description**                           |
|-----------------------------|------------------|-------------|-------------------------------------------|
| **Magic Byte 0**            | 1                | `0xEF`      | First magic byte                          |
| **Magic Byte 1**            | 1                | `0x52`      | Second magic byte (`R` in ASCII)         |
| **Version**                 | 1                | `0x01`      | Version of rWasm format                   |

### **Module Structure in Rust**

```rust
pub struct RwasmModule {
    /// The main instruction set (bytecode) for this module that includes an entrypoint
    /// and all required functions.
    pub code_section: InstructionSet,
    
    /// Linear read-only memory data initialized when the module is instantiated.
    pub data_section: Vec<u8>,
    
    /// Table initializers, function refs for the module's table section.
    pub elem_section: Vec<u32>,
    
    /// An original Wasm bytecode used during compilation
    pub wasm_section: Vec<u8>,
}
```

All sections are encoded using bincode with legacy configuration for deterministic serialization.

---

## **3. Encoding of Each Section**

### **3.1 Code Section**

The **code section** contains the compiled bytecode as an `InstructionSet`. The InstructionSet is a vector of `Opcode` enums, where each instruction is encoded using bincode.

#### **Structure**

```rust
pub struct InstructionSet {
    instructions: Vec<Opcode>,
}
```

#### **Example: Code Section Content**

```rust
let code_section = instruction_set! {
    I32Const(100)
    I32Const(20)
    I32Add
    I32Const(3)
    I32Add
    Drop
};
```

This represents:
- `I32Const(UntypedValue::from(100))` → Load constant 100
- `I32Const(UntypedValue::from(20))` → Load constant 20
- `I32Add` → Add two values
- `I32Const(UntypedValue::from(3))` → Load constant 3
- `I32Add` → Add two values
- `Drop` → Drop result

#### **Encoding**

Each instruction is encoded as a bincode-serialized `Opcode` enum with embedded operands.

---

### **3.2 Data Section**

The **data section** contains **linear memory data** for the module. It's a concatenation of all data segments from the original WebAssembly binary.

#### **Structure**

```rust
pub data_section: Vec<u8>
```

#### **Example: Data Section Content**

```rust
let data_section = vec![
    0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x2C, 0x20, 0x57, 0x6F, 0x72, 0x6C, 0x64, // "Hello, World"
];
```

This contains raw bytes that will be copied into linear memory during module instantiation.

---

### **3.3 Element Section**

The **element section** defines **function table elements**. It contains function indices that are used to initialize tables via the `table_init` instruction.

#### **Structure**

```rust
pub elem_section: Vec<u32>
```

#### **Example: Element Section Content**

```rust
let elem_section = vec![1, 2, 3];
```

This defines:
- Element 0 → Function index 1
- Element 1 → Function index 2
- Element 2 → Function index 3

---

### **3.4 WASM Section**

The **WASM section** stores the **original WebAssembly bytecode** used during compilation. This is kept for reference and debugging purposes.

#### **Structure**

```rust
pub wasm_section: Vec<u8>
```

#### **Example: WASM Section Content**

```rust
let wasm_section = include_bytes!("original_module.wasm").to_vec();
```

This contains the original WebAssembly binary that was compiled to produce this rWasm module.

---

## **5. Example: Complete rWasm Module**

Here's a practical example of creating and encoding an rWasm module:

### **Example Module Creation**

```rust
use crate::{instruction_set, types::RwasmModule, UntypedValue};

let module = RwasmModule {
    code_section: instruction_set! {
        ConsumeFuel(1)
        I32Const(UntypedValue::from(100))
        I32Const(UntypedValue::from(20))
        I32Add
        Drop
    },
    data_section: b"Hello, World".to_vec(),
    elem_section: vec![1, 2, 3],
    wasm_section: vec![], // Empty for this example
};
```

### **Serialization Process**

```rust
// Serialize the module to binary format
let binary = module.serialize();

// The binary format will contain:
// 1. Magic bytes: [0xEF, 0x52]
// 2. Version: [0x01]
// 3. Code section (bincode-encoded InstructionSet)
// 4. Data section (bincode-encoded Vec<u8>)
// 5. Element section (bincode-encoded Vec<u32>)
// 6. WASM section (bincode-encoded Vec<u8>)
```

### **Deserialization Process**

```rust
// Deserialize the module from binary format
let (deserialized_module, _bytes_read) = RwasmModule::new(&binary);

// The deserialization process:
// 1. Validates magic bytes (0xEF, 0x52)
// 2. Validates version (0x01)
// 3. Decodes each section using bincode
// 4. Returns the reconstructed RwasmModule
```

### **Module Display Format**

When displayed, the module shows a readable representation:

```
RwasmModule {
 .function_begin_0 (#0)
  0000: ConsumeFuel(1)
  0001: I32Const(100)
  0002: I32Const(20)
  0003: I32Add
  0004: Drop
 .function_end

 .ro_data: [48, 65, 6c, 6c, 6f, 2c, 20, 57, 6f, 72, 6c, 64],
 .ro_elem: [1, 2, 3],
}
```

### **Binary Format Details**

The resulting binary uses:
- **Magic bytes**: `0xEF 0x52` (identifies as rWasm)
- **Version**: `0x01` (version 1)
- **Bincode encoding**: Deterministic serialization using legacy configuration
- **Little-endian**: All multi-byte values are little-endian
- **Variable length**: Each section is variable length based on content

The exact binary representation depends on the bincode serialization format and is optimized for efficient loading and execution rather than human readability.
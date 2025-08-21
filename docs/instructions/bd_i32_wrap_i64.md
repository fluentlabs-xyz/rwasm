## `i32_wrap_i64`

### **Description**

The `i32_wrap_i64` instruction **converts a 64-bit integer to a 32-bit integer by wrapping**. It pops a 64-bit integer from the stack, takes the lower 32 bits, and pushes the result as a 32-bit integer onto the stack.

### **Behavior**

1. **Pops** a 64-bit integer from the stack.
2. **Extracts** the lower 32 bits of the value.
3. **Pushes** the result as a 32-bit integer onto the stack.
4. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: **Unchanged** (one value is popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | i64_value | sp ]
```

#### **After Execution:**

```
[ ... | i32_result | sp ]
```

Where `i32_result` is `(int32_t)(i64_value & 0xFFFFFFFF)`.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **unary operation** that consumes one stack value and produces one result.
- **No traps** can occur during execution of this instruction.
- Only the **lower 32 bits** are preserved; the upper 32 bits are discarded.
- The operation is **lossy** - information in the upper 32 bits is lost.
- Equivalent to the expression `(int32_t)i64_value` in high-level languages.
- This is the standard way to convert from 64-bit to 32-bit integers in WebAssembly.
- The sign of the result depends on the value of bit 31 (the new sign bit).
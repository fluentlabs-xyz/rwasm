## `i32_extend8_s`

### **Description**

The `i32_extend8_s` instruction **sign-extends the lower 8 bits of a 32-bit integer**. It pops a 32-bit integer from the stack, treats the lower 8 bits as a signed 8-bit integer, extends it to 32 bits with sign extension, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** a 32-bit integer from the stack.
2. **Extracts** the lower 8 bits of the value.
3. **Performs** sign extension from 8 bits to 32 bits.
4. **Pushes** the result onto the stack.
5. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: **Unchanged** (one value is popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value | sp ]
```

#### **After Execution:**

```
[ ... | result | sp ]
```

Where `result` is `(int32_t)(int8_t)(value & 0xFF)`.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **unary operation** that consumes one stack value and produces one result.
- **No traps** can occur during execution of this instruction.
- **Sign extension** means that if bit 7 (the sign bit of the 8-bit value) is 1, the upper 24 bits are filled with 1s.
- If bit 7 is 0, the upper 24 bits are filled with 0s.
- The upper 24 bits of the input value are ignored.
- Equivalent to the expression `(int32_t)(int8_t)(value & 0xFF)` in high-level languages.
- Useful for working with 8-bit signed values stored in 32-bit registers.
- Range of results: [-128, 127].
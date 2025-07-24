## `i32_shr_u`

### **Description**

The `i32_shr_u` instruction **performs unsigned (logical) right shift operation on a 32-bit integer**. It pops two 32-bit integers from the stack (value and shift amount), shifts the value right by the specified number of bits with zero extension, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the shift amount from the stack.
2. **Pops** the value to be shifted from the stack.
3. **Masks** the shift amount to 5 bits (shift_amount & 0x1F) to ensure it's in range [0, 31].
4. **Performs** logical right shift operation on the value (filling with zeros).
5. **Pushes** the result onto the stack.
6. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Decreased by 1 (two values are popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value | shift_amount | sp ]
```

#### **After Execution:**

```
[ ... | result | sp ]
```

Where `result` is `(unsigned)value >> (shift_amount & 0x1F)` with zero extension.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- The shift amount is automatically **masked to 5 bits** to prevent undefined behavior.
- **No traps** can occur during execution of this instruction.
- Uses **zero extension** - zeros are always shifted in from the left, regardless of the sign bit.
- Equivalent to the expression `(uint32_t)value >> (shift_amount & 0x1F)` in high-level languages.
- Logical right shift by n bits is equivalent to unsigned division by 2^n (truncated).
- Does not preserve the sign of the original value.
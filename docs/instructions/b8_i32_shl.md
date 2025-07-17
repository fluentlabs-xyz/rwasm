## `i32_shl`

### **Description**

The `i32_shl` instruction **performs left shift operation on a 32-bit integer**. It pops two 32-bit integers from the stack (value and shift amount), shifts the value left by the specified number of bits, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the shift amount from the stack.
2. **Pops** the value to be shifted from the stack.
3. **Masks** the shift amount to 5 bits (shift_amount & 0x1F) to ensure it's in range [0, 31].
4. **Performs** left shift operation on the value.
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

Where `result` is `value << (shift_amount & 0x1F)`.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- The shift amount is automatically **masked to 5 bits** to prevent undefined behavior.
- **No traps** can occur during execution of this instruction.
- Equivalent to the expression `value << (shift_amount & 0x1F)` in high-level languages.
- Left shift by n bits is equivalent to multiplying by 2^n (with overflow wrapping).
- Bits shifted out of the left side are discarded, zeros are shifted in from the right.
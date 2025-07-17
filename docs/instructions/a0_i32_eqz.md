## `i32_eqz`

### **Description**

The `i32_eqz` instruction **tests if a 32-bit integer is equal to zero**. It pops a 32-bit integer from the stack and pushes 1 if the value is zero, or 0 if the value is non-zero.

### **Behavior**

1. **Pops** a 32-bit integer from the stack.
2. **Compares** the value to zero.
3. **Pushes** 1 (true) if the value is zero, 0 (false) if the value is non-zero.
4. **Increments** the instruction pointer (`ip`) by 1.

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

Where `result` is 1 if `value == 0`, otherwise 0.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **unary operation** that consumes one stack value and produces one result.
- The result is always either 0 or 1, making it suitable for conditional branching.
- Equivalent to the expression `value == 0` in high-level languages.
- **No traps** can occur during execution of this instruction.
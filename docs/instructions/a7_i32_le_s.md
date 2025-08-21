## `i32_le_s`

### **Description**

The `i32_le_s` instruction **compares two 32-bit integers for signed less-than-or-equal**. It pops two 32-bit integers from the stack, interprets them as signed integers, and pushes 1 if the first operand is less than or equal to the second, or 0 otherwise.

### **Behavior**

1. **Pops** the second operand (right-hand side) from the stack.
2. **Pops** the first operand (left-hand side) from the stack.
3. **Interprets** both values as signed 32-bit integers.
4. **Compares** the first operand with the second operand.
5. **Pushes** 1 (true) if `lhs <= rhs`, 0 (false) otherwise.
6. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Decreased by 1 (two values are popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | lhs | rhs | sp ]
```

#### **After Execution:**

```
[ ... | result | sp ]
```

Where `result` is 1 if `(signed)lhs <= (signed)rhs`, otherwise 0.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- The comparison is performed using **signed interpretation** of the 32-bit integers.
- Values are interpreted in two's complement representation.
- The result is always either 0 or 1, making it suitable for conditional branching.
- **No traps** can occur during execution of this instruction.
- Equivalent to the expression `(int32_t)lhs <= (int32_t)rhs` in high-level languages.
## `i32_eq`

### **Description**

The `i32_eq` instruction **compares two 32-bit integers for equality**. It pops two 32-bit integers from the stack and pushes 1 if they are equal, or 0 if they are not equal.

### **Behavior**

1. **Pops** the second operand (right-hand side) from the stack.
2. **Pops** the first operand (left-hand side) from the stack.
3. **Compares** the two values for equality.
4. **Pushes** 1 (true) if the values are equal, 0 (false) if they are not equal.
5. **Increments** the instruction pointer (`ip`) by 1.

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

Where `result` is 1 if `lhs == rhs`, otherwise 0.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- The comparison is performed using **bitwise equality**.
- The result is always either 0 or 1, making it suitable for conditional branching.
- **No traps** can occur during execution of this instruction.
- Equivalent to the expression `lhs == rhs` in high-level languages.
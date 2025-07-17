## `i32_add`

### **Description**

The `i32_add` instruction **adds two 32-bit integers**. It pops two 32-bit integers from the stack, performs addition, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the second operand (right-hand side) from the stack.
2. **Pops** the first operand (left-hand side) from the stack.
3. **Performs** wrapping addition of the two values.
4. **Pushes** the result onto the stack.
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

Where `result` is `lhs + rhs` (with wrapping overflow).

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- Addition is performed with **wrapping overflow** - if the result exceeds 2^32-1, it wraps around.
- **No traps** can occur during execution of this instruction.
- Equivalent to the expression `(int32_t)((uint32_t)lhs + (uint32_t)rhs)` in high-level languages.
- The operation is **commutative**: `a + b = b + a`.
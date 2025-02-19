## `i32_const`

### **Description**

The `i32_const` instruction **pushes a constant 32-bit integer onto the stack**.

### **Behavior**

1. **Pushes** the immediate value (`untyped_value`) onto the stack.
2. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Incremented by `1`** (stores the constant value).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (constants are stored on the stack, not in memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | constant_value | sp ]
```

(The stack pointer moves up by `1`, and the constant value is pushed onto the stack.)

### **Operands**

- `untyped_value` (i32): The 32-bit integer constant to be pushed onto the stack.

### **Notes**

- This instruction **does not modify memory** and is only used to load immediate values onto the stack.
- Commonly used in expressions and arithmetic operations.
- The instruction **does not modify `ms`** or consume fuel.
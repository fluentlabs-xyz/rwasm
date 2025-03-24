## `drop`

### **Description**

The `drop` instruction **removes the top value from the stack** without using it. It is typically used to discard unused
computation results.

### **Behavior**

1. **Pops** the top value from the stack.
2. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `1`** (top stack value is removed).
- **memory**: **Unchanged** (this instruction does not interact with memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value_to_drop | sp ]
```

#### **After Execution:**

```
[ ... | sp ]
```

(`sp` moves down by `1`, effectively removing the top value.)

### **Operands**

- **None** (operates on the top value of the stack).

### **Notes**

- This instruction is **used to discard values** that are no longer needed.
- If the stack is empty before execution, **behavior is undefined**.
- It does **not modify memory** or consume fuel.
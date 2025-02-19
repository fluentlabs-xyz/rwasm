## `global_set`

### **Description**

The `global_set` instruction **updates the value of a global variable** using a value popped from the stack.

### **Behavior**

1. **Pops** the top value from the stack.
2. **Stores** the popped value into the global variable at index `global_idx`.
3. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `1`** (removes the top stack value).
- **memory**: **Updated** (modifies the specified global variable).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | new_value | sp ]
```

#### **After Execution:**

```
[ ... | sp ]
```

(`sp` moves down by `1`, as `new_value` is removed and stored in the global variable.)

### **Operands**

- `global_idx` (integer): The index of the global variable to update.

### **Notes**

- If `global_idx` refers to an **immutable** global, **behavior is undefined**.
- If the stack is **empty** before execution, **behavior is undefined**.
- This instruction **modifies memory** by updating global storage.
## `global_get`

### **Description**

The `global_get` instruction **retrieves the value of a global variable** and pushes it onto the stack.

### **Behavior**

1. **Fetches** the value of the global variable at index `global_idx`.
2. **Pushes** the retrieved value onto the stack.
3. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Incremented by `1`** (adds the retrieved global value).
- **memory**: **Unchanged** (global variables are stored separately).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | global_value | sp ]
```

(`sp` moves up by `1`, and the retrieved global value is now on top.)

### **Operands**

- `global_idx` (integer): The index of the global variable to fetch.

### **Notes**

- If `global_idx` is invalid, **behavior is undefined**.
- If the global variable is **not initialized**, it returns a **default value**.
- This instruction does **not modify memory** but only reads from global storage.
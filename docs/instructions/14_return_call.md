## `return_call`

### **Description**

The `return_call` instruction **returns from the current function and immediately calls an external function**
identified by `func_idx`. This is commonly used for tail-call optimizations.

### **Behavior**

1. **Increments** the instruction pointer (`ip`) by `2` before making the function call.
2. **Calls** the external function referenced by `func_idx`.
3. Execution transitions to the external function.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Increased by `2`** before calling the external function.
    - **Execution is transferred to the external function.**
- **stack pointer (`sp`)**:
    - **Unchanged** (stack modifications are handled within the external function).
- **memory**: **Unchanged** (this instruction does not directly modify memory).
- **fuel counter (`fc`)**: **Unchanged** (fuel consumption depends on the called function).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | sp ]
```

#### **After Execution (External Function Called):**

```
[ ... | new function locals | sp ]
```

(`sp` is updated within the external function, and `ip` moves to the function’s first instruction.)

### **Operands**

- `func_idx` (integer): Specifies the external function to call.

### **Notes**

- The **instruction pointer is incremented before calling the function** to prevent unexpected interruptions.
- Used for **efficient tail-call optimizations** in WebAssembly execution.
- The function call is **external**, meaning it may interact with system APIs or host functions.
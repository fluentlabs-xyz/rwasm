## `call`

### **Description**

The `call` instruction performs a **direct call to an external function** identified by `func_idx`. It ensures proper
stack synchronization and updates the instruction pointer (`ip`) before the call to prevent interruptions.

### **Behavior**

1. **Synchronizes** the stack pointer (`sp`) with the value stack.
2. **Increments** the instruction pointer (`ip`) by `1` before making the function call to prevent execution
   interruptions.
3. **Calls** the external function identified by `func_idx`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Increased by `1`** before calling the function.
    - **Execution is transferred to the external function**.
- **stack pointer (`sp`)**:
    - **Unchanged** (stack modifications occur inside the called function).
- **memory**: **Unchanged** (this instruction does not directly modify memory).
- **fuel counter (`fc`)**: **Unchanged** (fuel consumption depends on the execution of the called function).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | sp ]
```

#### **After Execution (Function Call Success):**

```
[ ... | new function locals | sp ]
```

(`sp` is updated within the external function, and `ip` moves to the function’s first instruction.)

### **Operands**

- `func_idx` (integer): Specifies the external function to call.

### **Notes**

- The **instruction pointer is incremented before calling the function** to prevent execution from being interrupted
  unexpectedly.
- This instruction enables **interaction with host functions and system APIs**.
- The function execution is handled externally and may cause an **interruption** depending on system behavior.
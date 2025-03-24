## `return_call_internal`

### **Description**

The `return_call_internal` instruction performs a **return followed by an internal function call**. It first adjusts the
instruction pointer (`ip`) to transition from the current function, then calls the internal function identified by
`func_idx`.

### **Behavior**

1. Increments the instruction pointer (`ip`) by `2` to prepare for the function call.
2. Retrieves the function reference (`instr_ref`) corresponding to `func_idx`.
3. Prepares the stack for the function call.
4. Updates `sp` to the new function's stack pointer.
5. Sets `ip` to the instruction reference (`instr_ref`), transferring control to the target function.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Increased by `2`** before transitioning to the new function.
    - **Updated to the function’s instruction reference (`instr_ref`)** to begin execution of the internal function.
- **stack pointer (`sp`)**:
    - **Updated to the new function’s stack pointer** after preparing for the call.
- **memory**: **Unchanged** (no read or write operations).
- **fuel counter (`fc`)**: **Unchanged** (this instruction does not consume fuel).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | sp ]
```

#### **After Execution (Function Call Transition):**

```
[ ... | new function locals | sp ]
```

(`sp` is updated to the new function’s stack pointer, and `ip` jumps to the function’s first instruction.)

### **Operands**

- `func_idx` (integer): Specifies the internal function to call.

### **Notes**

- This instruction **combines returning from the current function and calling a new one** in a single step.
- Used for efficient tail-call optimizations within WebAssembly execution.
- If `func_idx` is invalid, execution panics.
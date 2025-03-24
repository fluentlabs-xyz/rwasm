## `call_internal`

### **Description**

The `call_internal` instruction performs a **direct call to an internal function** identified by `func_idx`. It ensures
proper stack and instruction pointer (`ip`) management while preventing excessive recursion depth.

### **Behavior**

1. **Increments** the instruction pointer (`ip`) by `1` before making the function call.
2. **Synchronizes** the stack pointer (`sp`) with the value stack.
3. **Checks** if the call stack exceeds the maximum recursion depth:
    - If it does, execution traps (`TrapCode::StackOverflow`).
4. **Pushes** the current instruction pointer (`ip`) onto the call stack.
5. **Fetches** the instruction reference (`instr_ref`) for the function from the function table.
6. **Prepares** the value stack for the function call.
7. **Updates** the stack pointer (`sp`) for the new function’s execution.
8. **Sets** the instruction pointer (`ip`) to `instr_ref` to begin execution of the function.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Increased by `1`** before transitioning to the function.
    - **Updated to the function’s instruction reference (`instr_ref`)** upon execution.
- **stack pointer (`sp`)**:
    - **Updated** to reflect the new function’s stack frame.
- **memory**: **Unchanged** (this instruction does not directly modify memory).
- **fuel counter (`fc`)**: **Unchanged** (fuel consumption depends on function execution).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | sp ]
```

#### **After Execution (Function Call Success):**

```
[ ... | new function locals | sp ]
```

(`sp` is updated to the new function’s stack frame, and `ip` jumps to the function’s first instruction.)

#### **After Execution (Recursion Depth Exceeded—Trap):**

- **Execution halts due to a `StackOverflow` trap.**

### **Operands**

- `func_idx` (integer): Specifies the internal function to call.

### **Notes**

- If the recursion depth exceeds `N_MAX_RECURSION_DEPTH`, execution **traps**.
- This instruction **does not consume fuel** directly but affects overall execution.
- Used for **efficient function calls within WebAssembly**.
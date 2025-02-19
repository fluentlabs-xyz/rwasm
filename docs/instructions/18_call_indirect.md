## `call_indirect`

### **Description**

The `call_indirect` instruction **performs an indirect function call** by looking up a function index in a function
table. The function to be called is determined dynamically based on an index provided on the stack. If the index is out
of bounds or points to a `null` entry, execution traps.

### **Behavior**

1. **Fetches** the function table index from memory.
2. **Pops** the function index (`func_index`) from the stack.
3. **Stores** the expected function signature (`signature_idx`) for validation.
4. **Resolves** the function index from the table:
    - If the index is out of bounds, execution **traps** (`TrapCode::TableOutOfBounds`).
    - If the index is `null`, execution **traps** (`TrapCode::IndirectCallToNull`).
5. **Adjusts** the function index for internal function reference (`func_idx`).
6. **Increments** the instruction pointer (`ip`) by `2`.
7. **Synchronizes** the stack pointer (`sp`) with the value stack.
8. **Checks** recursion depth:
    - If it exceeds `N_MAX_RECURSION_DEPTH`, execution **traps** (`TrapCode::StackOverflow`).
9. **Pushes** the current instruction pointer (`ip`) onto the call stack.
10. **Fetches** the function's instruction reference (`instr_ref`).
11. **Prepares** the stack for the function call.
12. **Updates** the stack pointer (`sp`) for the new function.
13. **Transfers** execution to the resolved function.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Increased by `2`** before transitioning to the function.
    - **Updated to the resolved function’s instruction reference (`instr_ref`)**.
- **stack pointer (`sp`)**:
    - **Decremented by `1`** due to popping the function index.
    - **Updated** to reflect the new function’s stack frame.
- **memory**: **Unchanged** (this instruction does not directly modify memory).
- **fuel counter (`fc`)**: **Unchanged** (fuel consumption depends on function execution).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | function index | sp ]
```

#### **After Execution (Function Call Success):**

```
[ ... | new function locals | sp ]
```

(`sp` is updated to the new function’s stack frame, and `ip` jumps to the function’s first instruction.)

#### **After Execution (Invalid Function Index - Trap):**

- **Execution halts due to a `TableOutOfBounds` or `IndirectCallToNull` trap.**

#### **After Execution (Recursion Depth Exceeded—Trap):**

- **Execution halts due to a `StackOverflow` trap.**

### **Operands**

- `signature_idx` (integer): Specifies the function signature for validation.
- `func_index` (stack value): The function index retrieved from the table.

### **Notes**

- Enables **dynamic function dispatch** in WebAssembly by calling functions through a function table.
- If the function index is invalid or the table reference is `null`, execution **traps**.
- The function call **must** match the expected signature; otherwise, behavior is undefined.
- If recursion depth exceeds `N_MAX_RECURSION_DEPTH`, execution **traps**.
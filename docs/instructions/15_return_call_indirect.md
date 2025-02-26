## `return_call_indirect`

### **Description**

The `return_call_indirect` instruction **returns from the current function and immediately calls an internal function
indirectly**. The function to be called is determined dynamically from a **table lookup** using an index provided on the
stack. If the index is invalid or refers to `null`, execution traps.

### **Behavior**

1. **Retrieves** the function index (`func_index`) from the stack.
2. **Fetches** the corresponding function reference from the table.
3. **Validates** the function reference:
    - If `func_index` is out of bounds, execution traps (`TrapCode::TableOutOfBounds`).
    - If `func_index` is `null`, execution traps (`TrapCode::IndirectCallToNull`).
4. **Adjusts** the function index to match the internal function reference system.
5. **Calls** the resolved function using an indirect call mechanism.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Set to the resolved function’s instruction pointer.**
- **stack pointer (`sp`)**:
    - **Decremented by `1`** due to popping the function index.
- **memory**: **Unchanged** (this instruction does not directly modify memory).
- **fuel counter (`fc`)**: **Unchanged** (fuel consumption depends on the called function).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | function index | sp ]
```

#### **After Execution (Function Call Success):**

```
[ ... | new function locals | sp ]
```

(`sp` is updated within the called function, and `ip` moves to the function’s first instruction.)

#### **After Execution (Invalid Function Index - Trap):**

- **Execution halts due to a trap (`TableOutOfBounds` or `IndirectCallToNull`).**

### **Operands**

- `signature_idx` (integer): Specifies the function signature for validation.
- `func_index` (stack value): The function index retrieved from the table.

### **Notes**

- This instruction enables **dynamic function dispatch** within WebAssembly.
- If the function index is invalid or the table reference is `null`, execution **traps**.
- Used in scenarios where function calls need to be dynamically resolved.
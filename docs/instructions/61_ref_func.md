## `ref_func`

### **Description**

The `ref_func` instruction **pushes a reference to a function onto the stack**. This function reference can be used for
indirect function calls or stored in a table.

### **Behavior**

1. **Computes** the function reference by adding `FUNC_REF_OFFSET` to `func_idx`.
2. **Pushes** the computed function reference onto the stack.
3. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Incremented by `1`** (stores the function reference).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (function references do not modify memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | function_reference | sp ]
```

(The stack pointer moves up by `1`, and the function reference is pushed onto the stack.)

### **Operands**

- `func_idx` (integer): The index of the function whose reference is being retrieved.

### **Notes**

- **Function references are used in indirect function calls or stored in tables**.
- This instruction does **not** call the function, only pushes its reference.
- The instruction **does not modify `ms`** or memory, as function references exist separately.
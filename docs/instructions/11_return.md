## `return`

### **Description**

The `return` instruction exits the current function and resumes execution at the caller's instruction pointer (`ip`). If
no caller exists, execution terminates with an exit code of `0`.

### **Behavior**

1. Pops the caller’s instruction pointer (`ip`) from the call stack.
2. If a caller exists, execution resumes at the caller’s `ip`.
3. If no caller exists, execution terminates with an exit code of `0`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Set to the caller’s `ip`** if returning.
    - **If no caller exists, execution stops with an exit code of `0`.**
- **stack pointer (`sp`)**: **Unchanged** (stack cleanup logic is ignored as per instructions).
- **memory**: **Unchanged** (no read or write operations).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function locals | return values | sp ]
```

#### **After Execution (Caller Exists):**

```
[ ... | caller state | sp ]
```

(`ip` is set to the caller’s `ip`.)

#### **After Execution (No Caller - Program Exit):**

```
Execution stops with exit code `0`.
```

### **Operands**

- **None** (return operation is implicit).

### **Notes**

- If there is no caller, execution **terminates** upon return.
- This instruction is essential for function control flow in WebAssembly.
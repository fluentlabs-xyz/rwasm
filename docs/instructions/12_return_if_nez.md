## `return_if_nez`

### **Description**

The `return_if_nez` instruction conditionally returns from the current function if the top stack value is
**nonzero (`true`)**. If the function returns, the instruction pointer (`ip`) is restored to the caller’s location. If
there is no caller, execution terminates with an exit code of `0`. If the condition is `false`, execution proceeds to
the next instruction.

### **Behavior**

1. Pops the top value from the stack and interprets it as a **boolean condition**.
2. If the condition is **nonzero (`true`)**:
    - Pops the caller's instruction pointer (`ip`) from the call stack.
    - If a caller exists, execution resumes at the caller’s `ip`.
    - If no caller exists, execution terminates with an exit code of `0`.
3. If the condition is **zero (`false`)**, increments the instruction pointer (`ip`) by `1` and continues execution.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Set to the caller’s `ip`** if returning.
    - **Increased by `1`** if the condition is `false`.
- **stack pointer (`sp`)**:
    - **Decremented by `1`** due to popping the condition.
- **memory**: **Unchanged** (no read or write operations).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | return values | condition | sp ]
```

#### **After Execution (Condition = `true`, Caller Exists):**

```
[ ... | caller state | sp ]
```

(`sp` decreases by `1`, and execution resumes at the caller’s `ip`.)

#### **After Execution (Condition = `true`, No Caller - Program Exit):**

```
Execution stops with exit code `0`.
```

#### **After Execution (Condition = `false`):**

```
[ ... | return values | sp ]
```

(`sp` decreases by `1`, and `ip` moves to the next instruction.)

### **Operands**

- **None** (condition value is popped from the stack).

### **Notes**

- If there is no caller in the call stack, execution **terminates** when returning.
- This instruction enables conditional function returns based on runtime conditions.
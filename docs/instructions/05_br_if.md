## `br_if`

### **Description**

The `br_if` instruction performs a **conditional branch** based on the value at the top of the stack. If the condition
is `true`, execution continues with the next instruction. If `false`, execution jumps to the target instruction
specified by `branch_offset`.

### **Behavior**

1. Pops a boolean condition from the stack.
2. If the condition is `true`, the instruction pointer (`ip`) is incremented by `1` to continue execution normally.
3. If the condition is `false`, the instruction pointer (`ip`) is modified by adding `branch_offset`, causing a jump.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**:
    - **Increased by `1`** if the condition is `true`.
    - **Offset by `branch_offset`** if the condition is `false`.
- **Stack Pointer (`SP`)**: **Decremented by `1`** (as the condition value is popped from the stack).
- **Memory**: **Unchanged** (no read or write operations).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | condition | SP ]
```

(`SP` points to the latest uninitialized stack position.)

#### **After Execution (Condition = `true`):**

```
[ ... | SP ]
```

(`SP` decreases by `1`, and `ip` moves to the next instruction.)

#### **After Execution (Condition = `false`):**

```
[ ... | SP ]
```

(`SP` decreases by `1`, and `ip` jumps by `branch_offset`.)

### **Operands**

- `branch_offset` (signed integer): Specifies the number of instructions to jump forward or backward if the condition is
  `false`.

### **Notes**

- Unlike `br`, this instruction only branches when the condition is `false`.
- It is useful for implementing conditional control flow structures like `if` statements.
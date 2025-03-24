## `br_if_nez`

### **Description**

The `br_if_nez` instruction performs a **conditional branch** based on the value at the top of the stack. If the
condition is **nonzero (`true`)**, execution jumps to the instruction at the target offset (`branch_offset`). Otherwise,
execution continues with the next instruction.

### **Behavior**

1. Pops a boolean condition from the stack.
2. If the condition is **nonzero (`true`)**, the instruction pointer (`ip`) is modified by adding `branch_offset`,
   causing a jump.
3. If the condition is **zero (`false`)**, the instruction pointer (`ip`) is incremented by `1` to continue execution
   normally.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**:
    - **Offset by `branch_offset`** if the condition is `true`.
    - **Increased by `1`** if the condition is `false`.
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

(`SP` decreases by `1`, and `ip` jumps by `branch_offset`.)

#### **After Execution (Condition = `false`):**

```
[ ... | SP ]
```

(`SP` decreases by `1`, and `ip` moves to the next instruction.)

### **Operands**

- `branch_offset` (signed integer): Specifies the number of instructions to jump forward or backward if the condition is
  `true`.

### **Notes**

- This instruction branches **only when the condition is nonzero (`true`)**.
- It is commonly used for loop continuation or conditional jumps where `0` is treated as `false` and any nonzero value
  as `true`.
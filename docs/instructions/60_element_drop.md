## `element_drop`

### **Description**

The `element_drop` instruction **marks a specified element segment as dropped**, making it unavailable for future table
initialization operations.

### **Behavior**

1. **Resolves** the element segment using `element_segment_idx`.
2. **Drops** the contents of the element segment, making it inaccessible.
3. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Unchanged** (this instruction does not modify the stack).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (only marks the element segment as dropped).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | sp ]
```

(The stack remains unchanged.)

### **Operands**

- `element_segment_idx` (integer): The index of the element segment to drop.

### **Notes**

- **Dropped element segments can no longer be used in `table_init` operations**.
- The instruction does **not** modify memory but only marks the element segment as unavailable.
- Attempting to use a dropped element segment **may result in a trap** in future instructions.
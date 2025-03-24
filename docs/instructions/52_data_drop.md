## `data_drop`

### **Description**

The `data_drop` instruction **marks a specified data segment as dropped**, making it unavailable for future memory
initialization operations.

### **Behavior**

1. **Resolves** the data segment using `data_segment_idx`.
2. **Drops** the contents of the data segment, making it inaccessible.
3. **Increments** the instruction pointer (`ip`) by `1` to proceed to the next instruction.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Unchanged** (this instruction does not modify the stack).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (only marks the data segment as dropped).
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

- `data_segment_idx` (integer): The index of the data segment to drop.

### **Notes**

- **Dropped data segments can no longer be used in `memory_init` operations**.
- The instruction does **not** modify memory but only marks data as unavailable.
- Attempting to use a dropped data segment behaves as empty data segments.
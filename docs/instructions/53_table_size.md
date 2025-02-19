## `table_size`

### **Description**

The `table_size` instruction **retrieves the current size of a specified table** and pushes it onto the stack.

### **Behavior**

1. **Fetches** the table at `table_idx`.
    - If the table does not exist, execution **traps** (`unresolved table segment`).
2. **Retrieves** the current size of the table.
3. **Pushes** the table size onto the stack.
4. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Incremented by `1`** (stores the table size).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (tables are separate from linear memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | table_size | sp ]
```

(The stack pointer moves up by `1`, and the table size is pushed onto the stack.)

### **Operands**

- `table_idx` (integer): The index of the table to fetch the size from.

### **Notes**

- If `table_idx` refers to an **uninitialized table**, execution **traps**.
- This instruction **does not modify memory or table contents**, only retrieves the size.
- The returned size is measured in **table elements**, not bytes.
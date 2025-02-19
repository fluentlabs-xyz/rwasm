## `table_set`

### **Description**

The `table_set` instruction **stores a value in a specified table** at a given index. If the index is out of bounds,
execution traps.

### **Behavior**

1. **Pops** two values from the stack:
    - `index`: The **table index** where the value will be stored.
    - `value`: The **value** to store in the table.
2. **Attempts** to set `value` at `index` in the table identified by `table_idx`:
    - If `index` is **out of bounds**, execution **traps** (`TrapCode::TableOutOfBounds`).
    - If successful, `value` is stored at `index`.
3. **Logs** the table change if tracing is enabled.
4. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `2`** (pops `index` and `value`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (tables are separate from linear memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | index | value | sp ]
```

#### **After Execution (Successful Set):**

```
[ ... | sp ]
```

(`sp` moves down by `2`, as the parameters are removed.)

#### **After Execution (Table Out of Bounds - Trap):**

- **Execution halts due to a `TableOutOfBounds` trap.**

### **Operands**

- `table_idx` (integer): The index of the table to store the value.

### **Notes**

- **Writing outside the table bounds results in a trap**.
- The instruction **does not modify `ms`** but operates on table storage.
- Used for **storing function references or other values** in tables.
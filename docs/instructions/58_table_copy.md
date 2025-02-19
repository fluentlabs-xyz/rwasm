## `table_copy`

### **Description**

The `table_copy` instruction **copies elements from one table to another (or within the same table)**. It reads the
destination index, source index, and count from the stack and performs the copy operation.

### **Behavior**

1. **Fetches** the source table index (`src_table_idx`) from memory.
2. **Pops** three values from the stack:
    - `d`: The **destination index** in the destination table.
    - `s`: The **source index** in the source table.
    - `n`: The **number of table elements** to copy.
3. **Converts** `s`, `d`, and `n` to `u32`.
4. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `n` elements.
    - If fuel is insufficient, execution **traps**.
5. **Validates** the source and destination indices:
    - If `s + n` or `d + n` **exceeds the table size**, execution **traps**.
6. **Performs** the table copy:
    - If the source and destination tables **are different**, it copies elements between tables.
    - If they **are the same**, it performs an internal `copy_within` operation.
7. **Increments** the instruction pointer (`ip`) by `2`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `2`**.
- **stack pointer (`sp`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (tables are separate from linear memory).
- **fuel counter (`fc`)**:
    - **Decreased** based on `n` elements if fuel metering is enabled.
    - **Unchanged** if fuel metering is disabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | count | sp ]
```

#### **After Execution (Successful Copy):**

```
[ ... | sp ]
```

(`sp` moves down by `3`, as the parameters are removed.)

#### **After Execution (Table Out of Bounds - Trap):**

- **Execution halts due to a table access trap.**

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- `dst_table_idx` (integer): The index of the destination table.
- `src_table_idx` (retrieved from memory): The index of the source table.

### **Notes**

- **Copying outside allocated table bounds results in a trap**.
- If `n` is `0`, **no table elements are modified**.
- If fuel metering is enabled and there is **not enough fuel for `n` elements, execution traps**.
- The instruction **does not modify `ms`** but affects table storage.
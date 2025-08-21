## `table_copy`

### **Description**

The `table_copy` instruction **copies elements from one table to another (or within the same table)**. It reads the destination index, source index, and count from the stack and performs the copy operation.

### **Behavior**

1. **Pops** three values from the stack:
   - `d`: The **destination index** in the destination table.
   - `s`: The **source index** in the source table.
   - `n`: The **number of table elements** to copy.
2. **Converts** `s`, `d`, and `n` to `u32`.
3. **Validates** the source and destination indices:
   - If `s + n` or `d + n` **exceeds the table size**, execution **traps**.
4. **Performs** the table copy:
   - If the source and destination tables **are different**, it copies elements between tables.
   - If they **are the same**, it performs an internal `copy_within` operation.
5. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **Memory**: **Unchanged** (tables are separate from linear memory).
- **Tables**: **Modified** according to the copy operation.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | count | SP ]
```

#### **After Execution (Successful Copy):**

```
[ ... | SP ]
```

(`SP` moves down by `3`, as the parameters are removed.)

#### **After Execution (Table Out of Bounds - Trap):**

- **Execution halts due to a table access trap.**

### **Operands**

- `dst_table_idx` (TableIdx): The index of the destination table.
- `src_table_idx` (TableIdx): The index of the source table.

### **Notes**

- **Copying outside allocated table bounds results in a trap**.
- If `n` is `0`, **no table elements are modified**.
- The instruction handles both inter-table and intra-table copying efficiently.
- When copying within the same table, the operation uses `copy_within` for safe overlapping copies.
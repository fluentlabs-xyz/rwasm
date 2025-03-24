## `table_fill`

### **Description**

The `table_fill` instruction **fills a specified range of elements in a table** with a given value. It reads the
starting index, value, and count from the stack and performs the fill operation.

### **Behavior**

1. **Pops** three values from the stack:
    - `i`: The **starting index** in the table.
    - `val`: The **value** to be written into the table.
    - `n`: The **number of table entries** to fill.
2. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `n` elements.
    - If fuel is insufficient, execution **traps**.
3. **Attempts** to fill `n` table entries starting from index `i` with `val`:
    - If `i + n` **exceeds the table size**, execution **traps**.
    - If successful, the table entries are updated.
4. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `3`** (pops `i`, `val`, and `n`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (tables are separate from linear memory).
- **fuel counter (`fc`)**:
    - **Decreased** based on `n` elements if fuel metering is enabled.
    - **Unchanged** if fuel metering is disabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | index | value | count | sp ]
```

#### **After Execution (Successful Fill):**

```
[ ... | sp ]
```

(`sp` moves down by `3`, as the parameters are removed.)

#### **After Execution (Table Out of Bounds - Trap):**

- **Execution halts due to a table access trap.**

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- `table_idx` (integer): The index of the table to fill.

### **Notes**

- **Writing outside the table bounds results in a trap**.
- If `n` is `0`, **no table entries are modified**.
- If fuel metering is enabled and there is **not enough fuel for `n` elements, execution traps**.
- The instruction **does not modify `ms`** but affects table storage.
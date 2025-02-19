## `table_grow`

### **Description**

The `table_grow` instruction **attempts to increase the size of a table** by a specified number of elements. If
successful, it pushes the previous table size onto the stack; otherwise, it pushes `u32::MAX`.

### **Behavior**

1. **Pops** two values from the stack:
    - `init`: The initial value to fill the new table entries.
    - `delta`: The number of elements to add to the table.
2. **Converts** `delta` to `u32`.
3. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `delta` elements.
    - If fuel is insufficient, execution **traps**.
4. **Attempts** to grow the table:
    - If successful, **pushes the previous table size** onto the stack.
    - If unsuccessful due to an invalid growth request, **pushes `u32::MAX`**.
    - If growth fails due to a trap condition, execution **traps**.
5. **Logs** the table size change if tracing is enabled.
6. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Unchanged in count** (pops `delta` and `init`, then pushes the previous table size or
  `u32::MAX`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (table storage is separate).
- **fuel counter (`fc`)**:
    - **Decreased** based on `delta` elements if fuel metering is enabled.
    - **Unchanged** if fuel metering is disabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | init | delta | sp ]
```

#### **After Execution (Successful Growth):**

```
[ ... | previous_table_size | sp ]
```

(The previous table size is stored, `sp` remains at the same level.)

#### **After Execution (Failed Growth - Invalid Request):**

```
[ ... | u32::MAX | sp ]
```

(`sp` remains at the same level, and `u32::MAX` is pushed to indicate failure.)

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

#### **After Execution (Trap Condition - Table Growth Failure):**

- **Execution halts due to a table-related trap.**

### **Operands**

- `table_idx` (integer): The index of the table to grow.

### **Notes**

- **Attempting to grow beyond the maximum table size results in failure (`u32::MAX`).**
- If `delta` is `0`, **no changes occur, and the previous table size is returned**.
- If fuel metering is enabled and there is **not enough fuel for `delta` elements, execution traps**.
- The instruction **does not modify `ms`** but affects table storage.
## `table_init`

### **Description**

The `table_init` instruction **copies elements from an element segment into a table**. It reads the destination index,
source index, and number of elements from the stack and initializes the table.

### **Behavior**

1. **Fetches** the table index (`table_idx`) from memory.
2. **Pops** three values from the stack:
    - `d`: The **destination index** in the table.
    - `s`: The **source index** in the element segment.
    - `n`: The **number of elements** to copy.
3. **Converts** `s`, `d`, and `n` to `u32`.
4. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `n` elements.
    - If fuel is insufficient, execution **traps**.
5. **Validates** the element segment:
    - If the segment is **empty**, an empty placeholder is used.
    - If the element segment has been dropped, execution **traps**.
6. **Validates** the source and destination indices:
    - If `s + n` or `d + n` **exceeds the segment or table size**, execution **traps**.
7. **Copies** `n` elements from the element segment into the table.
8. **Increments** the instruction pointer (`ip`) by `2`.

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

#### **After Execution (Successful Initialization):**

```
[ ... | sp ]
```

(`sp` moves down by `3`, as the parameters are removed.)

#### **After Execution (Table or Segment Out of Bounds - Trap):**

- **Execution halts due to a table or segment access trap.**

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- `element_segment_idx` (integer): The index of the element segment to initialize the table.

### **Notes**

- **Copying outside allocated table bounds or element segment bounds results in a trap**.
- If `n` is `0`, **no table elements are modified**.
- If fuel metering is enabled and there is **not enough fuel for `n` elements, execution traps**.
- **Dropped element segments cannot be accessed**.
- The instruction **does not modify `ms`** but affects table storage.
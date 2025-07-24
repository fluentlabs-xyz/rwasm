## `table_init`

### **Description**

The `table_init` instruction **initializes a table region with elements from an element segment**. It reads the destination index, source index, and count from the stack and copies elements from the specified element segment to the table.

### **Behavior**

1. **Fetches** the destination table index from memory.
2. **Pops** three values from the stack:
   - `d`: The **destination index** in the table.
   - `s`: The **source index** in the element segment.
   - `n`: The **number of elements** to copy.
3. **Converts** `s`, `d`, and `n` to `u32`.
4. **Checks** if the element segment is empty (dropped):
   - If dropped, uses an empty segment for the operation.
5. **Validates** the indices and performs the initialization:
   - If bounds are exceeded, execution **traps**.
6. **Copies** elements from the element segment to the table.
7. **Increments** the instruction pointer (`ip`) by `2`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `2`**.
- **Stack Pointer (`SP`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **Memory**: **Unchanged** (tables are separate from linear memory).
- **Tables**: **Modified** with elements from the element segment.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | count | SP ]
```

#### **After Execution (Successful Initialization):**

```
[ ... | SP ]
```

(`SP` moves down by `3`, as the parameters are removed.)

#### **After Execution (Table Out of Bounds - Trap):**

- **Execution halts due to a table access trap.**

### **Operands**

- `element_segment_idx` (ElementSegmentIdx): The index of the element segment to copy from.

### **Notes**

- **Copying outside allocated table bounds results in a trap**.
- If `n` is `0`, **no table elements are modified**.
- If the element segment has been dropped, the operation uses an empty segment.
- The element segment index refers to the original segment number, even though elements are stored in segment 0.
- The instruction pointer is incremented by `2` to account for the additional operand.
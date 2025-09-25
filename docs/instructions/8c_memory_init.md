## `memory_init`

### **Description**

The `memory_init` instruction **initializes a memory region with data from a data segment**. It reads the destination address, source offset, and length from the stack and copies data from the specified data segment to linear memory.

### **Behavior**

1. **Checks** if the data segment is empty (dropped).
2. **Pops** three values from the stack:
   - `d`: The **destination offset** in memory.
   - `s`: The **source offset** in the data segment.
   - `n`: The **number of bytes** to copy.
3. **Converts** `d`, `s`, and `n` to `usize`.
4. **Validates** the destination memory range:
   - If the range **exceeds available memory**, execution **traps** (`TrapCode::MemoryOutOfBounds`).
5. **Validates** the source data segment range:
   - If the range **exceeds the data segment**, execution **traps** (`TrapCode::MemoryOutOfBounds`).
6. **Copies** data from the segment to memory.
7. **Logs** memory changes if tracing is enabled.
8. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **Memory**: **Modified** in the destination range.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | length | SP ]
```

#### **After Execution (Successful Initialization):**

```
[ ... | SP ]
```

(`SP` moves down by `3`, as the parameters are removed.)

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts due to a `MemoryOutOfBounds` trap.**

### **Operands**

- `data_segment_idx` (DataSegmentIdx): The index of the data segment to copy from.

### **Notes**

- **Copying outside allocated memory results in a trap**.
- If `n` is `0`, **no memory is modified**.
- If the data segment has been dropped, the operation treats it as empty.
- Both memory destination and data segment source ranges are validated.
- Memory tracing is performed if the tracing feature is enabled.
## `memory_copy`

### **Description**

The `memory_copy` instruction **copies a region of memory from one location to another**. It reads the destination address, source address, and length from the stack and performs the copy operation within linear memory.

### **Behavior**

1. **Pops** three values from the stack:
   - `d`: The **destination offset** in memory.
   - `s`: The **source offset** in memory.
   - `n`: The **number of bytes** to copy.
2. **Converts** `d`, `s`, and `n` to `usize`.
3. **Validates** both source and destination memory ranges:
   - If either range **exceeds available memory**, execution **traps** (`TrapCode::MemoryOutOfBounds`).
4. **Copies** memory from `[s..s+n]` to `[d..d+n]` using `copy_within` for safe overlapping copies.
5. **Logs** memory changes if tracing is enabled.
6. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **Memory**: **Modified** in the destination range.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | length | SP ]
```

#### **After Execution (Successful Copy):**

```
[ ... | SP ]
```

(`SP` moves down by `3`, as the parameters are removed.)

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts due to a `MemoryOutOfBounds` trap.**

### **Operands**

- **None** (operates based on the top three stack values).

### **Notes**

- **Copying outside allocated memory results in a trap**.
- If `n` is `0`, **no memory is modified**.
- The instruction **handles overlapping memory regions safely** using `copy_within`.
- Both source and destination ranges are validated before the copy operation.
- Memory tracing is performed if the tracing feature is enabled.
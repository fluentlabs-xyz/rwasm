## `memory_init`

### **Description**

The `memory_init` instruction **copies data from a specified data segment into linear memory**. It reads the destination
offset, source offset, and length from the stack and performs a memory initialization operation.

### **Behavior**

1. **Checks** if the data segment at `data_segment_idx` is empty.
2. **Pops** three values from the stack:
    - `d`: The **destination offset** in memory.
    - `s`: The **source offset** in the data segment.
    - `n`: The **number of bytes** to copy.
3. **Converts** `s`, `d`, and `n` to `usize`.
4. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `n` bytes.
    - If fuel is insufficient, execution **traps**.
5. **Validates** the memory range:
    - If the destination range **exceeds available memory**, execution **traps** (`TrapCode::MemoryOutOfBounds`).
    - If the source offset **exceeds the data segment bounds**, execution **traps**.
6. **Copies** `n` bytes from the data segment into memory at `[d..d+n]`.
7. **Logs** memory changes if tracing is enabled.
8. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Modified** in the destination range.
- **fuel counter (`fc`)**:
    - **Decreased** based on `n` bytes if fuel metering is enabled.
    - **Unchanged** if fuel metering is disabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | length | sp ]
```

#### **After Execution (Successful Initialization):**

```
[ ... | sp ]
```

(`sp` moves down by `3`, as the parameters are removed.)

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts due to a `MemoryOutOfBounds` trap.**

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- `data_segment_idx` (integer): Specifies the data segment from which to copy.

### **Notes**

- **Copying outside allocated memory or data segment bounds results in a trap**.
- If `n` is `0`, **no memory is modified**.
- If the data segment is **empty**, the instruction reads an empty buffer.
- The instruction **does not modify `ms`** but writes directly to memory.